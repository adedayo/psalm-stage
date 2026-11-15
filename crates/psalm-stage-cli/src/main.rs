mod mcp;

use std::io::{self, BufReader, Read};
use std::process::ExitCode;

use psalm_audio_core::library::{list_managed_setlists, match_library_songs, LibraryEntryStatus};
use psalm_audio_core::percussion::DEFAULT_PERCUSSION_ROOM_MIX;
use psalm_audio_core::percussion_kit::{
    default_percussion_kits_root, default_percussion_loops_root, export_percussion_loop_bundle,
    generate_studio_percussion_kit, list_percussion_kits, list_percussion_loops,
    ExportLoopBundleRequest, StudioKitProfile,
};
use psalm_audio_core::percussion_patterns::{
    CustomPercussionPattern, GrooveIntensity, PercussionStyle,
};
use psalm_workbench::{
    build_sunday_setlist, detect_toolchain_status, plan_sunday_setlist, resolve_app_config_dir,
    NoopProgressSink, RequestedSetlistSong, SeparationEngine, SetlistBuildRequest,
    SetlistPlanRequest, StderrProgressSink, StemPreset, WorkbenchManager, BUNDLED_SKILL_MD,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum SetlistInputFile {
    Plan(SetlistPlanRequest),
    SongsOnly(Vec<RequestedSetlistSong>),
}

fn read_input_json(path_or_dash: &str) -> Result<String, String> {
    if path_or_dash == "-" {
        let mut buf = String::new();
        io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("Failed to read stdin: {e}"))?;
        Ok(buf)
    } else {
        std::fs::read_to_string(path_or_dash)
            .map_err(|e| format!("Failed to read input file '{path_or_dash}': {e}"))
    }
}

fn parse_songs_payload(raw: &str, default_name: Option<&str>) -> Result<(String, Vec<RequestedSetlistSong>), String> {
    let parsed: SetlistInputFile =
        serde_json::from_str(raw).map_err(|e| format!("Invalid setlist input JSON: {e}"))?;
    match parsed {
        SetlistInputFile::Plan(p) => Ok((
            default_name
                .map(str::to_string)
                .unwrap_or(p.setlist_name),
            p.songs,
        )),
        SetlistInputFile::SongsOnly(songs) => Ok((
            default_name
                .unwrap_or("Sunday Service Set")
                .to_string(),
            songs,
        )),
    }
}

fn flag_value(args: &[String], flag: &str) -> Option<String> {
    args.windows(2)
        .find(|w| w[0] == flag)
        .map(|w| w[1].clone())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|a| a == flag)
}

fn print_help() {
    eprintln!(
        "psalm-stage-cli — Headless CLI & MCP server for Psalm Stage\n\
         \n\
         USAGE:\n\
         \tpsalm-stage-cli status [--json]\n\
         \tpsalm-stage-cli library list [--query <q>] [--status <ready|needs_setup|missing_stems>] [--json]\n\
         \tpsalm-stage-cli library match --title <title> [--artist <artist>] [--url <youtube_url>] [--json]\n\
         \tpsalm-stage-cli youtube search <query> [--limit <n>] [--json]\n\
         \tpsalm-stage-cli setlist plan --input <file|-> [--auto-approve] [--json]\n\
         \tpsalm-stage-cli setlist build --input <file|-> [--name <name>] [--output <path>] [--auto-approve] [--separation <auto|none|uvr|demucs>] [--stems <2|4|6>] [--force] [--json]\n\
         \tpsalm-stage-cli setlist list [--json]\n\
         \tpsalm-stage-cli loop kit generate [--profile <studio|makossa|acoustic>] [--out <dir>] [--json]\n\
         \tpsalm-stage-cli loop kit list [--json]\n\
         \tpsalm-stage-cli loop list [--json]\n\
         \tpsalm-stage-cli loop export --title <title> [--style <Makossa|Afrobeat|Woro|Worship44|...>] [--bpm <129>] [--bars <16>] [--intensity <low|medium|high>] [--static] [--kit <dir>] [--out <dir>] [--json]\n\
         \tpsalm-stage-cli skill [--json]\n\
         \tpsalm-stage-cli mcp\n"
    );
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || has_flag(&args, "--help") || has_flag(&args, "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }

    if args[0] == "mcp" {
        let stdin = io::stdin();
        let stdout = io::stdout();
        if let Err(err) = mcp::run_mcp_stdio(BufReader::new(stdin.lock()), stdout.lock()) {
            eprintln!("MCP server error: {err}");
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    }

    let json_mode = has_flag(&args, "--json");
    let config_dir = resolve_app_config_dir();
    let manager = WorkbenchManager::new(config_dir);
    let lib_cfg = manager.get_library_config();
    let wb_cfg = manager.get_workbench_config();

    match args.first().map(String::as_str) {
        Some("status") => {
            let tools = detect_toolchain_status(&wb_cfg);
            let payload = json!({
                "config_dir": manager.config_dir().to_string_lossy(),
                "library_root": lib_cfg.library_root,
                "setlists_root": lib_cfg.setlists_root,
                "workbench_config": wb_cfg,
                "toolchain": tools,
            });
            if json_mode {
                println!("{}", serde_json::to_string_pretty(&payload).unwrap());
            } else {
                println!("Config dir:   {}", manager.config_dir().display());
                println!("Library root: {}", lib_cfg.library_root);
                println!("Setlists dir: {}", lib_cfg.setlists_root);
                println!("yt-dlp:       {}", tools.yt_dlp.available);
                println!("ffmpeg:       {}", tools.ffmpeg.available);
                println!("UVR:          {}", tools.audio_separator.available);
                println!("Demucs:       {}", tools.demucs.available);
            }
            ExitCode::SUCCESS
        }
        Some("skill") => {
            if json_mode {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "skill_name": "psalm-stage-setlist",
                        "markdown": BUNDLED_SKILL_MD
                    }))
                    .unwrap()
                );
            } else {
                print!("{BUNDLED_SKILL_MD}");
            }
            ExitCode::SUCCESS
        }
        Some("library") => match args.get(1).map(String::as_str) {
            Some("list") => {
                let query = flag_value(&args, "--query").map(|s| s.to_ascii_lowercase());
                let status_filter = flag_value(&args, "--status").and_then(|s| match s.as_str() {
                    "ready" | "Ready" => Some(LibraryEntryStatus::Ready),
                    "needs_setup" | "NeedsSetup" => Some(LibraryEntryStatus::NeedsSetup),
                    "missing_stems" | "MissingStems" => Some(LibraryEntryStatus::MissingStems),
                    _ => None,
                });
                let entries: Vec<_> = manager
                    .scan_library()
                    .into_iter()
                    .filter(|e| status_filter.is_none_or(|st| e.status == st))
                    .filter(|e| {
                        query.as_ref().is_none_or(|q| {
                            e.title.to_ascii_lowercase().contains(q)
                                || e.artist.to_ascii_lowercase().contains(q)
                                || e.original_key.to_ascii_lowercase().contains(q)
                        })
                    })
                    .collect();
                if json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({ "count": entries.len(), "entries": entries }))
                            .unwrap()
                    );
                } else {
                    for e in entries {
                        println!(
                            "[{:?}] {} ({}) — {} BPM | Key {} | {}",
                            e.status, e.title, e.artist, e.bpm, e.original_key, e.dir
                        );
                    }
                }
                ExitCode::SUCCESS
            }
            Some("match") => {
                let Some(title) = flag_value(&args, "--title") else {
                    eprintln!("Missing --title <title>");
                    return ExitCode::FAILURE;
                };
                let artist = flag_value(&args, "--artist");
                let url = flag_value(&args, "--url");
                let entries = manager.scan_library();
                let matches =
                    match_library_songs(&entries, &title, artist.as_deref(), url.as_deref());
                if json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({ "matches": matches })).unwrap()
                    );
                } else {
                    for m in matches {
                        println!(
                            "{:.2} ({:?}) {} — {} [{}]",
                            m.score, m.reason, m.entry.title, m.entry.original_key, m.entry.dir
                        );
                    }
                }
                ExitCode::SUCCESS
            }
            _ => {
                print_help();
                ExitCode::FAILURE
            }
        },
        Some("youtube") if args.get(1).map(String::as_str) == Some("search") => {
            let Some(query) = args.get(2).filter(|s| !s.starts_with('-')) else {
                eprintln!("Missing search query");
                return ExitCode::FAILURE;
            };
            let limit = flag_value(&args, "--limit")
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(5);
            match manager.search_youtube(query, limit) {
                Ok(candidates) => {
                    if json_mode {
                        println!(
                            "{}",
                            serde_json::to_string_pretty(&json!({
                                "query": query,
                                "candidates": candidates
                            }))
                            .unwrap()
                        );
                    } else {
                        for (idx, c) in candidates.iter().enumerate() {
                            println!(
                                "{}. [score {:.2}] {} ({}, {:.0}s) -> {}",
                                idx + 1,
                                c.score,
                                c.title,
                                c.channel,
                                c.duration_sec,
                                c.webpage_url
                            );
                        }
                    }
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("Error: {err}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("setlist") => match args.get(1).map(String::as_str) {
            Some("list") => {
                let list = list_managed_setlists(&lib_cfg);
                if json_mode {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json!({ "count": list.len(), "setlists": list }))
                            .unwrap()
                    );
                } else {
                    for s in list {
                        println!("{} ({} songs) -> {}", s.name, s.song_count, s.path);
                    }
                }
                ExitCode::SUCCESS
            }
            Some("plan") => {
                let Some(input_path) = flag_value(&args, "--input") else {
                    eprintln!("Missing --input <file|->");
                    return ExitCode::FAILURE;
                };
                let raw = match read_input_json(&input_path) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                };
                let (name, songs) = match parse_songs_payload(&raw, flag_value(&args, "--name").as_deref()) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                };
                let plan = plan_sunday_setlist(
                    &SetlistPlanRequest {
                        setlist_name: name,
                        songs,
                        auto_approve: has_flag(&args, "--auto-approve"),
                        search_limit: 5,
                    },
                    &lib_cfg,
                    &wb_cfg,
                );
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&plan).unwrap());
                } else {
                    println!(
                        "Setlist Plan '{}' — ready_to_build={}",
                        plan.setlist_name, plan.ready_to_build
                    );
                    for item in &plan.items {
                        println!(
                            "  {}. {} => {:?} ({} st)",
                            item.slot + 1,
                            item.requested.title,
                            item.status,
                            item.key_shift_semitones
                        );
                    }
                }
                ExitCode::SUCCESS
            }
            Some("build") => {
                let Some(input_path) = flag_value(&args, "--input") else {
                    eprintln!("Missing --input <file|->");
                    return ExitCode::FAILURE;
                };
                let raw = match read_input_json(&input_path) {
                    Ok(r) => r,
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                };
                let (name, songs) = match parse_songs_payload(&raw, flag_value(&args, "--name").as_deref()) {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::FAILURE;
                    }
                };
                let sep_engine = flag_value(&args, "--separation").and_then(|s| match s.as_str() {
                    "none" | "None" => Some(SeparationEngine::None),
                    "uvr" | "AudioSeparator" => Some(SeparationEngine::AudioSeparator),
                    "demucs" | "Demucs" => Some(SeparationEngine::Demucs),
                    _ => None,
                });
                let stem_preset = flag_value(&args, "--stems").and_then(|s| match s.as_str() {
                    "2" | "TwoStems" => Some(StemPreset::TwoStems),
                    "4" | "FourStems" => Some(StemPreset::FourStems),
                    "6" | "SixStems" => Some(StemPreset::SixStems),
                    _ => None,
                });
                let req = SetlistBuildRequest {
                    setlist_name: name,
                    songs,
                    auto_approve: has_flag(&args, "--auto-approve"),
                    output_path: flag_value(&args, "--output"),
                    separation_engine: sep_engine,
                    stem_preset,
                    custom_model: None,
                    force_reextract: has_flag(&args, "--force"),
                };
                let res = if json_mode {
                    build_sunday_setlist(
                        &req,
                        &lib_cfg,
                        &wb_cfg,
                        Some(manager.config_dir()),
                        &NoopProgressSink,
                    )
                } else {
                    build_sunday_setlist(
                        &req,
                        &lib_cfg,
                        &wb_cfg,
                        Some(manager.config_dir()),
                        &StderrProgressSink,
                    )
                };
                match res {
                    Ok(build_res) => {
                        if json_mode {
                            println!("{}", serde_json::to_string_pretty(&build_res).unwrap());
                        } else if build_res.needs_confirmation {
                            eprintln!(
                                "Confirmation required for {} song(s). Re-run with confirmed youtube_url or --auto-approve.",
                                build_res.plan.needs_confirmation_count
                            );
                        } else if let Some(ref path) = build_res.setlist_path {
                            println!("Saved setlist to {path}");
                        }
                        if build_res.needs_confirmation || !build_res.built {
                            ExitCode::from(2)
                        } else {
                            ExitCode::SUCCESS
                        }
                    }
                    Err(err) => {
                        if json_mode {
                            println!("{}", json!({ "error": err }));
                        } else {
                            eprintln!("Build failed: {err}");
                        }
                        ExitCode::FAILURE
                    }
                }
            }
            _ => {
                print_help();
                ExitCode::FAILURE
            }
        },
        Some("loop") => match args.get(1).map(String::as_str) {
            Some("kit") => match args.get(2).map(String::as_str) {
                Some("generate") => {
                    let profile = match flag_value(&args, "--profile")
                        .as_deref()
                        .unwrap_or("studio")
                        .to_ascii_lowercase()
                        .as_str()
                    {
                        "makossa" | "african" => StudioKitProfile::MakossaLive,
                        "acoustic" | "cajon" => StudioKitProfile::AcousticCajon,
                        _ => StudioKitProfile::StudioPraise,
                    };
                    let root = flag_value(&args, "--out")
                        .map(std::path::PathBuf::from)
                        .unwrap_or_else(default_percussion_kits_root);
                    match generate_studio_percussion_kit(&root, profile) {
                        Ok(info) => {
                            if json_mode {
                                println!("{}", serde_json::to_string_pretty(&info).unwrap());
                            } else {
                                println!(
                                    "Generated {} ({} WAV samples) at {}",
                                    info.name, info.sample_count, info.dir
                                );
                            }
                            ExitCode::SUCCESS
                        }
                        Err(e) => {
                            if json_mode {
                                println!("{}", json!({ "error": e.to_string() }));
                            } else {
                                eprintln!("Kit generation failed: {e}");
                            }
                            ExitCode::FAILURE
                        }
                    }
                }
                Some("list") => {
                    let kits = list_percussion_kits(&default_percussion_kits_root());
                    if json_mode {
                        println!("{}", serde_json::to_string_pretty(&kits).unwrap());
                    } else if kits.is_empty() {
                        println!(
                            "No percussion kits found in {}. Run `psalm-stage-cli loop kit generate`.",
                            default_percussion_kits_root().display()
                        );
                    } else {
                        for k in kits {
                            println!("{} — {} ({} samples) [{}]", k.id, k.name, k.sample_count, k.dir);
                        }
                    }
                    ExitCode::SUCCESS
                }
                _ => {
                    print_help();
                    ExitCode::FAILURE
                }
            },
            Some("list") => {
                let loops = list_percussion_loops(&default_percussion_loops_root());
                if json_mode {
                    println!("{}", serde_json::to_string_pretty(&loops).unwrap());
                } else {
                    for lp in loops {
                        println!(
                            "{} — {} ({} steps x {} bars, {} hits)",
                            lp.id,
                            lp.name,
                            lp.steps_per_bar,
                            lp.bars_in_cycle,
                            lp.hits.len()
                        );
                    }
                }
                ExitCode::SUCCESS
            }
            Some("export") => {
                let Some(title) = flag_value(&args, "--title") else {
                    eprintln!("Missing --title for loop export");
                    return ExitCode::FAILURE;
                };
                let style = flag_value(&args, "--style")
                    .and_then(|s| serde_json::from_str::<PercussionStyle>(&format!("\"{s}\"")).ok())
                    .unwrap_or(PercussionStyle::Makossa);
                let custom_pattern = flag_value(&args, "--loop-file").and_then(|p| {
                    std::fs::read_to_string(&p)
                        .ok()
                        .and_then(|raw| serde_json::from_str::<CustomPercussionPattern>(&raw).ok())
                });
                let bpm = flag_value(&args, "--bpm")
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(128.0);
                let bars = flag_value(&args, "--bars")
                    .and_then(|v| v.parse::<u32>().ok())
                    .unwrap_or(16);
                let intensity = match flag_value(&args, "--intensity")
                    .as_deref()
                    .unwrap_or("high")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "low" => GrooveIntensity::Low,
                    "medium" | "med" => GrooveIntensity::Medium,
                    _ => GrooveIntensity::High,
                };
                let ts = if matches!(style, PercussionStyle::Woro | PercussionStyle::Worship68) {
                    [6, 8]
                } else {
                    [4, 4]
                };
                let req = ExportLoopBundleRequest {
                    title,
                    output_dir: flag_value(&args, "--out"),
                    bpm,
                    time_signature: ts,
                    bars,
                    dynamic_arrangement: !has_flag(&args, "--static"),
                    intensity,
                    room_mix: flag_value(&args, "--room")
                        .and_then(|v| v.parse::<f32>().ok())
                        .unwrap_or(DEFAULT_PERCUSSION_ROOM_MIX),
                    style,
                    custom_pattern,
                    kit_dir: flag_value(&args, "--kit"),
                };
                match export_percussion_loop_bundle(std::path::Path::new(&lib_cfg.library_root), req) {
                    Ok(res) => {
                        if json_mode {
                            println!("{}", serde_json::to_string_pretty(&res).unwrap());
                        } else {
                            println!(
                                "Exported multi-stem loop '{}' ({:.1}s @ {} BPM, {} stems) -> {}",
                                res.title,
                                res.duration_sec,
                                res.bpm,
                                res.stem_files.len(),
                                res.song_dir
                            );
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        if json_mode {
                            println!("{}", json!({ "error": e.to_string() }));
                        } else {
                            eprintln!("Loop export failed: {e}");
                        }
                        ExitCode::FAILURE
                    }
                }
            }
            _ => {
                print_help();
                ExitCode::FAILURE
            }
        },
        _ => {
            print_help();
            ExitCode::FAILURE
        }
    }
}

