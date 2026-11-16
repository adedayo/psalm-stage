use std::path::{Path, PathBuf};

use psalm_audio_core::library::{
    compute_semitone_shift, match_library_songs, scan_song_library, LibraryConfig,
    LibraryEntryStatus, LibraryMatchResult, LibrarySongEntry,
};
use psalm_audio_core::loader::save_setlist_manifest;
use psalm_audio_core::manifest::{
    canonicalize_source_ref, ClickMode, PadLockConfig, SetlistItem, SetlistManifest,
    SetlistMixerConfig, SetlistSection, SetlistSlotKind, SongManifest, StemMixOverride,
    TransitionMode,
};
use psalm_audio_core::pad_presets::{factory_pad_presets, list_all_pad_presets};
use psalm_audio_core::percussion::PercussionLockConfig;
use serde::{Deserialize, Serialize};

use crate::config::{save_library_config, SeparationEngine, StemPreset, WorkbenchConfig};
use crate::pipeline::{
    detect_toolchain_status, find_cached_library_bundle, ingest_songs_batch,
    resolve_effective_separation, BatchIngestRequest, BatchIngestResult, ImportJobRequest,
    ImportSource, ProgressSink,
};
use crate::youtube::{search_youtube, YouTubeSearchCandidate};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestedSetlistSong {
    pub title: String,
    #[serde(default)]
    pub artist: Option<String>,
    #[serde(default)]
    pub youtube_url: Option<String>,
    #[serde(default)]
    pub local_file: Option<String>,
    #[serde(default)]
    pub target_key: Option<String>,
    #[serde(default)]
    pub bpm_override: Option<f64>,
    #[serde(default)]
    pub transition_mode: Option<TransitionMode>,
    #[serde(default)]
    pub crossfade_sec: Option<f32>,
    #[serde(default)]
    pub ambient_pad_enabled: Option<bool>,
    #[serde(default)]
    pub pad_key_override: Option<String>,
    #[serde(default)]
    pub click_mode: Option<ClickMode>,
    #[serde(default)]
    pub slot_kind: Option<SetlistSlotKind>,
    #[serde(default)]
    pub section_id: Option<String>,
    #[serde(default)]
    pub time_signature: Option<[u8; 2]>,
    #[serde(default)]
    pub pad_preset_id: Option<String>,
    #[serde(default)]
    pub pad_lock: Option<PadLockConfig>,
    #[serde(default)]
    pub percussion_lock: Option<PercussionLockConfig>,
    #[serde(default)]
    pub stem_mix: Vec<StemMixOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistPlanRequest {
    #[serde(default = "default_setlist_name")]
    pub setlist_name: String,
    #[serde(default)]
    pub sections: Vec<SetlistSection>,
    pub songs: Vec<RequestedSetlistSong>,
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default = "default_search_limit")]
    pub search_limit: usize,
}

fn default_setlist_name() -> String {
    "Sunday Service Set".to_string()
}

fn default_search_limit() -> usize {
    5
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SongResolutionStatus {
    ReadyFromLibrary,
    ReadyToIngest,
    NeedsConfirmation,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedSongResolution {
    pub slot: usize,
    pub requested: RequestedSetlistSong,
    pub status: SongResolutionStatus,
    pub matched_library_dir: Option<String>,
    pub matched_song_id: Option<String>,
    pub matched_title: Option<String>,
    pub original_key: Option<String>,
    pub effective_key: Option<String>,
    pub key_shift_semitones: i32,
    pub bpm: Option<f64>,
    pub selected_youtube_url: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub library_candidates: Vec<LibraryMatchResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub youtube_candidates: Vec<YouTubeSearchCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistPlanResult {
    pub setlist_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sections: Vec<SetlistSection>,
    pub ready_to_build: bool,
    pub from_library_count: usize,
    pub to_ingest_count: usize,
    pub needs_confirmation_count: usize,
    pub blocked_count: usize,
    pub items: Vec<PlannedSongResolution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistBuildRequest {
    #[serde(default = "default_setlist_name")]
    pub setlist_name: String,
    #[serde(default)]
    pub sections: Vec<SetlistSection>,
    pub songs: Vec<RequestedSetlistSong>,
    #[serde(default)]
    pub mixer: Option<SetlistMixerConfig>,
    #[serde(default)]
    pub auto_approve: bool,
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default)]
    pub separation_engine: Option<SeparationEngine>,
    #[serde(default)]
    pub stem_preset: Option<StemPreset>,
    #[serde(default)]
    pub custom_model: Option<String>,
    #[serde(default)]
    pub force_reextract: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistBuildResult {
    pub built: bool,
    pub needs_confirmation: bool,
    pub setlist_path: Option<String>,
    pub setlist: Option<SetlistManifest>,
    pub plan: SetlistPlanResult,
    pub ingest: Option<BatchIngestResult>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_required_songs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ManualSetlistItemInput {
    #[serde(default)]
    pub song_dir: Option<String>,
    #[serde(default)]
    pub song_id: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub original_key: Option<String>,
    #[serde(default)]
    pub base_bpm: Option<f64>,
    #[serde(default)]
    pub target_key: Option<String>,
    #[serde(default)]
    pub key_shift_semitones: Option<i32>,
    #[serde(default)]
    pub bpm_override: Option<f64>,
    #[serde(default)]
    pub transition_mode: Option<TransitionMode>,
    #[serde(default)]
    pub crossfade_sec: Option<f32>,
    #[serde(default)]
    pub ambient_pad_enabled: Option<bool>,
    #[serde(default)]
    pub pad_key_override: Option<String>,
    #[serde(default)]
    pub click_mode: Option<ClickMode>,
    #[serde(default)]
    pub slot_kind: Option<SetlistSlotKind>,
    #[serde(default)]
    pub section_id: Option<String>,
    #[serde(default)]
    pub time_signature: Option<[u8; 2]>,
    #[serde(default)]
    pub pad_preset_id: Option<String>,
    #[serde(default)]
    pub pad_lock: Option<PadLockConfig>,
    #[serde(default)]
    pub percussion_lock: Option<PercussionLockConfig>,
    #[serde(default)]
    pub stem_mix: Vec<StemMixOverride>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistCreateRequest {
    pub name: String,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub output_path: Option<String>,
    #[serde(default)]
    pub sections: Vec<SetlistSection>,
    pub items: Vec<ManualSetlistItemInput>,
    #[serde(default)]
    pub mixer: Option<SetlistMixerConfig>,
}

pub fn sanitize_setlist_slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    let mut last_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "sunday-setlist".to_string()
    } else {
        trimmed
    }
}

fn resolve_pad_lock_for_slot(
    explicit: Option<&PadLockConfig>,
    preset_id: Option<&str>,
    config_dir: Option<&Path>,
    warnings: &mut Vec<String>,
) -> Option<PadLockConfig> {
    if let Some(lock) = explicit {
        return Some(lock.clone());
    }
    let id = preset_id.map(str::trim).filter(|s| !s.is_empty())?;
    let presets = config_dir
        .map(list_all_pad_presets)
        .unwrap_or_else(factory_pad_presets);
    if let Some(found) = presets
        .into_iter()
        .find(|p| p.id.eq_ignore_ascii_case(id) || p.name.eq_ignore_ascii_case(id))
    {
        return Some(PadLockConfig {
            enabled: true,
            gain: 0.45,
            preset_id: Some(found.id),
            preset_name: Some(found.name),
            layers: found.layers,
            brightness: found.brightness,
            shimmer: found.shimmer,
            pan: found.pan,
        });
    }
    warnings.push(format!(
        "Pad preset '{id}' was not found in factory or user presets; leaving pad unlocked."
    ));
    None
}

fn compute_shift_with_note(
    original_key: &str,
    target_key: Option<&str>,
    notes: &mut Vec<String>,
) -> (i32, Option<String>) {
    let Some(target) = target_key.map(str::trim).filter(|s| !s.is_empty()) else {
        return (0, Some(original_key.to_string()));
    };
    match compute_semitone_shift(original_key, target) {
        Ok(shift) => (shift, Some(target.to_string())),
        Err(msg) => {
            notes.push(format!("{msg} — key_shift_semitones defaulted to 0."));
            (0, Some(original_key.to_string()))
        }
    }
}

/// Resolves a list of requested songs against the local Song Library and YouTube search
/// without downloading audio or writing files.
pub fn plan_sunday_setlist(
    req: &SetlistPlanRequest,
    lib_cfg: &LibraryConfig,
    wb_cfg: &WorkbenchConfig,
) -> SetlistPlanResult {
    let entries = scan_song_library(lib_cfg);
    let tools = detect_toolchain_status(wb_cfg);
    let mut warnings = Vec::new();
    let mut items = Vec::with_capacity(req.songs.len());

    plan_with_entries_and_search(
        req,
        lib_cfg,
        &entries,
        |query, limit| {
            let Some(ref yt_dlp) = tools.yt_dlp.resolved_path else {
                return Err(format!(
                    "yt-dlp is not installed ({})",
                    tools.yt_dlp.install_hint
                ));
            };
            search_youtube(yt_dlp, query, limit)
        },
        &mut warnings,
        &mut items,
    )
}

pub(crate) fn plan_with_entries_and_search<F>(
    req: &SetlistPlanRequest,
    lib_cfg: &LibraryConfig,
    entries: &[LibrarySongEntry],
    mut search_fn: F,
    warnings: &mut Vec<String>,
    items: &mut Vec<PlannedSongResolution>,
) -> SetlistPlanResult
where
    F: FnMut(&str, usize) -> Result<Vec<YouTubeSearchCandidate>, String>,
{
    let mut from_library_count = 0usize;
    let mut to_ingest_count = 0usize;
    let mut needs_confirmation_count = 0usize;
    let mut blocked_count = 0usize;

    for (slot, song) in req.songs.iter().enumerate() {
        let mut notes = Vec::new();
        let slot_kind = song.slot_kind.unwrap_or(SetlistSlotKind::SongBundle);
        if matches!(
            slot_kind,
            SetlistSlotKind::PadOnly | SetlistSlotKind::PercussionGroove
        ) {
            from_library_count += 1;
            let key = song
                .target_key
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "C".to_string());
            let default_bpm = match slot_kind {
                SetlistSlotKind::PercussionGroove => 120.0,
                _ => 72.0,
            };
            notes.push(format!("Standalone {:?} slot (no backing stems required).", slot_kind));
            items.push(PlannedSongResolution {
                slot,
                requested: song.clone(),
                status: SongResolutionStatus::ReadyFromLibrary,
                matched_library_dir: None,
                matched_song_id: Some(sanitize_setlist_slug(&song.title)),
                matched_title: Some(song.title.clone()),
                original_key: Some(key.clone()),
                effective_key: Some(key),
                key_shift_semitones: 0,
                bpm: Some(song.bpm_override.unwrap_or(default_bpm)),
                selected_youtube_url: None,
                library_candidates: Vec::new(),
                youtube_candidates: Vec::new(),
                notes,
            });
            continue;
        }

        let matches = match_library_songs(
            entries,
            &song.title,
            song.artist.as_deref(),
            song.youtube_url.as_deref(),
        );

        let top_ready = matches
            .iter()
            .find(|m| m.entry.status == LibraryEntryStatus::Ready && m.score >= 0.85);

        if let Some(m) = top_ready {
            from_library_count += 1;
            let (shift, eff_key) = compute_shift_with_note(
                &m.entry.original_key,
                song.target_key.as_deref(),
                &mut notes,
            );
            if m.entry
                .provenance
                .as_ref()
                .is_some_and(|p| p.needs_review)
            {
                notes.push("Library entry is marked needs_review — verify Bar 1 and sections in Workbench.".into());
            }
            items.push(PlannedSongResolution {
                slot,
                requested: song.clone(),
                status: SongResolutionStatus::ReadyFromLibrary,
                matched_library_dir: Some(m.entry.dir.clone()),
                matched_song_id: Some(m.entry.song_id.clone()),
                matched_title: Some(m.entry.title.clone()),
                original_key: Some(m.entry.original_key.clone()),
                effective_key: eff_key,
                key_shift_semitones: shift,
                bpm: Some(song.bpm_override.unwrap_or(m.entry.bpm)),
                selected_youtube_url: None,
                library_candidates: matches.into_iter().take(3).collect(),
                youtube_candidates: Vec::new(),
                notes,
            });
            continue;
        }

        // Local file directly provided
        if let Some(ref local_path) = song.local_file.as_ref().filter(|p| !p.trim().is_empty()) {
            let src_path = Path::new(local_path);
            if !src_path.exists() {
                blocked_count += 1;
                notes.push(format!("Local file not found: {local_path}"));
                items.push(PlannedSongResolution {
                    slot,
                    requested: song.clone(),
                    status: SongResolutionStatus::Blocked,
                    matched_library_dir: None,
                    matched_song_id: None,
                    matched_title: None,
                    original_key: None,
                    effective_key: song.target_key.clone(),
                    key_shift_semitones: 0,
                    bpm: song.bpm_override,
                    selected_youtube_url: None,
                    library_candidates: matches.into_iter().take(3).collect(),
                    youtube_candidates: Vec::new(),
                    notes,
                });
                continue;
            }
            to_ingest_count += 1;
            items.push(PlannedSongResolution {
                slot,
                requested: song.clone(),
                status: SongResolutionStatus::ReadyToIngest,
                matched_library_dir: None,
                matched_song_id: None,
                matched_title: Some(song.title.clone()),
                original_key: None,
                effective_key: song.target_key.clone(),
                key_shift_semitones: 0,
                bpm: song.bpm_override,
                selected_youtube_url: None,
                library_candidates: matches.into_iter().take(3).collect(),
                youtube_candidates: Vec::new(),
                notes,
            });
            continue;
        }

        // Explicit YouTube URL provided
        if let Some(ref yt_url) = song.youtube_url.as_ref().filter(|u| !u.trim().is_empty()) {
            let source_ref = canonicalize_source_ref("youtube", yt_url);
            let predicted_dir = PathBuf::from(&lib_cfg.library_root).join(&song.title);
            if let Some(cached_dir) =
                find_cached_library_bundle(&source_ref, &predicted_dir, lib_cfg)
            {
                if let Some(cached_entry) = entries.iter().find(|e| e.dir == cached_dir) {
                    from_library_count += 1;
                    let (shift, eff_key) = compute_shift_with_note(
                        &cached_entry.original_key,
                        song.target_key.as_deref(),
                        &mut notes,
                    );
                    items.push(PlannedSongResolution {
                        slot,
                        requested: song.clone(),
                        status: SongResolutionStatus::ReadyFromLibrary,
                        matched_library_dir: Some(cached_entry.dir.clone()),
                        matched_song_id: Some(cached_entry.song_id.clone()),
                        matched_title: Some(cached_entry.title.clone()),
                        original_key: Some(cached_entry.original_key.clone()),
                        effective_key: eff_key,
                        key_shift_semitones: shift,
                        bpm: Some(song.bpm_override.unwrap_or(cached_entry.bpm)),
                        selected_youtube_url: Some(yt_url.to_string()),
                        library_candidates: matches.into_iter().take(3).collect(),
                        youtube_candidates: Vec::new(),
                        notes,
                    });
                    continue;
                }
            }

            to_ingest_count += 1;
            items.push(PlannedSongResolution {
                slot,
                requested: song.clone(),
                status: SongResolutionStatus::ReadyToIngest,
                matched_library_dir: None,
                matched_song_id: None,
                matched_title: Some(song.title.clone()),
                original_key: None,
                effective_key: song.target_key.clone(),
                key_shift_semitones: 0,
                bpm: song.bpm_override,
                selected_youtube_url: Some(yt_url.to_string()),
                library_candidates: matches.into_iter().take(3).collect(),
                youtube_candidates: Vec::new(),
                notes,
            });
            continue;
        }

        // Missing from Library & no YouTube URL provided -> search YouTube
        let search_query = match song.artist.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            Some(artist) => format!("{} {} official audio", song.title.trim(), artist),
            None => format!("{} official audio", song.title.trim()),
        };

        let mut yt_candidates = Vec::new();
        match search_fn(&search_query, req.search_limit.max(1)) {
            Ok(found) => yt_candidates = found,
            Err(err) => {
                notes.push(err.clone());
                warnings.push(format!("Song '{}' search failed: {err}", song.title));
            }
        }

        if yt_candidates.is_empty() {
            blocked_count += 1;
            notes.push("No YouTube candidates found — provide an explicit youtube_url or local_file.".into());
            items.push(PlannedSongResolution {
                slot,
                requested: song.clone(),
                status: SongResolutionStatus::Blocked,
                matched_library_dir: None,
                matched_song_id: None,
                matched_title: None,
                original_key: None,
                effective_key: song.target_key.clone(),
                key_shift_semitones: 0,
                bpm: song.bpm_override,
                selected_youtube_url: None,
                library_candidates: matches.into_iter().take(3).collect(),
                youtube_candidates: Vec::new(),
                notes,
            });
        } else if req.auto_approve {
            to_ingest_count += 1;
            let best = &yt_candidates[0];
            notes.push(format!(
                "Auto-approved top YouTube match '{}' ({})",
                best.title, best.webpage_url
            ));
            items.push(PlannedSongResolution {
                slot,
                requested: song.clone(),
                status: SongResolutionStatus::ReadyToIngest,
                matched_library_dir: None,
                matched_song_id: None,
                matched_title: Some(song.title.clone()),
                original_key: None,
                effective_key: song.target_key.clone(),
                key_shift_semitones: 0,
                bpm: song.bpm_override,
                selected_youtube_url: Some(best.webpage_url.clone()),
                library_candidates: matches.into_iter().take(3).collect(),
                youtube_candidates: yt_candidates,
                notes,
            });
        } else {
            needs_confirmation_count += 1;
            notes.push("Confirm which YouTube candidate (or ambiguous Library entry) to use before downloading.".into());
            items.push(PlannedSongResolution {
                slot,
                requested: song.clone(),
                status: SongResolutionStatus::NeedsConfirmation,
                matched_library_dir: None,
                matched_song_id: None,
                matched_title: None,
                original_key: None,
                effective_key: song.target_key.clone(),
                key_shift_semitones: 0,
                bpm: song.bpm_override,
                selected_youtube_url: None,
                library_candidates: matches.into_iter().take(3).collect(),
                youtube_candidates: yt_candidates,
                notes,
            });
        }
    }

    SetlistPlanResult {
        setlist_name: req.setlist_name.clone(),
        sections: req.sections.clone(),
        ready_to_build: needs_confirmation_count == 0 && blocked_count == 0,
        from_library_count,
        to_ingest_count,
        needs_confirmation_count,
        blocked_count,
        items: items.clone(),
        warnings: warnings.clone(),
    }
}

pub fn build_sunday_setlist(
    req: &SetlistBuildRequest,
    lib_cfg: &LibraryConfig,
    wb_cfg: &WorkbenchConfig,
    config_dir: Option<&Path>,
    sink: &dyn ProgressSink,
) -> Result<SetlistBuildResult, String> {
    let plan_req = SetlistPlanRequest {
        setlist_name: req.setlist_name.clone(),
        sections: req.sections.clone(),
        songs: req.songs.clone(),
        auto_approve: req.auto_approve,
        search_limit: 5,
    };
    let plan = plan_sunday_setlist(&plan_req, lib_cfg, wb_cfg);
    if !plan.ready_to_build {
        return Ok(SetlistBuildResult {
            built: false,
            needs_confirmation: plan.needs_confirmation_count > 0,
            setlist_path: None,
            setlist: None,
            warnings: plan.warnings.clone(),
            plan,
            ingest: None,
            review_required_songs: Vec::new(),
        });
    }

    let tools = detect_toolchain_status(wb_cfg);
    let mut warnings = plan.warnings.clone();
    let (eff_engine, eff_preset, fallback_warn) = resolve_effective_separation(
        req.separation_engine,
        req.stem_preset,
        wb_cfg,
        &tools,
    );
    if let Some(w) = fallback_warn {
        warnings.push(w);
    }

    let mut jobs: Vec<ImportJobRequest> = Vec::new();
    let mut slot_to_job_idx: Vec<(usize, usize)> = Vec::new();

    for item in &plan.items {
        if item.status == SongResolutionStatus::ReadyToIngest {
            let source = if let Some(ref lp) = item
                .requested
                .local_file
                .as_ref()
                .filter(|s| !s.trim().is_empty())
            {
                ImportSource::LocalFile((*lp).clone())
            } else if let Some(ref url) = item.selected_youtube_url {
                ImportSource::YouTubeUrl(url.clone())
            } else {
                return Err(format!(
                    "Slot {} ('{}') is marked ReadyToIngest without source",
                    item.slot + 1,
                    item.requested.title
                ));
            };
            let job_idx = jobs.len();
            jobs.push(ImportJobRequest {
                source,
                song_title: Some(item.requested.title.clone()),
                artist: item.requested.artist.clone(),
                target_dir: None,
                separation_engine: eff_engine,
                stem_preset: eff_preset,
                custom_model: req.custom_model.clone(),
                force_reextract: req.force_reextract,
            });
            slot_to_job_idx.push((item.slot, job_idx));
        }
    }

    let ingest_result = if jobs.is_empty() {
        None
    } else {
        Some(ingest_songs_batch(
            BatchIngestRequest {
                jobs,
                use_default_with_fallback: false,
            },
            lib_cfg,
            wb_cfg,
            sink,
        ))
    };

    if let Some(ref batch) = ingest_result {
        for item in &batch.items {
            warnings.extend(item.warnings.clone());
            if let Some(ref err) = item.error {
                return Err(format!(
                    "Failed to ingest '{}': {err}",
                    item.song_title
                ));
            }
        }
    }

    let mut final_items: Vec<SetlistItem> = Vec::with_capacity(plan.items.len());
    let mut review_required_songs: Vec<String> = Vec::new();

    for planned in &plan.items {
        let slot_kind = planned
            .requested
            .slot_kind
            .unwrap_or(SetlistSlotKind::SongBundle);
        let pad_lock = resolve_pad_lock_for_slot(
            planned.requested.pad_lock.as_ref(),
            planned.requested.pad_preset_id.as_deref(),
            config_dir,
            &mut warnings,
        );

        if matches!(
            slot_kind,
            SetlistSlotKind::PadOnly | SetlistSlotKind::PercussionGroove
        ) {
            let key = planned
                .requested
                .target_key
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "C".to_string());
            let base_bpm = match slot_kind {
                SetlistSlotKind::PercussionGroove => 120.0,
                _ => 72.0,
            };
            let base_click_mode = match slot_kind {
                SetlistSlotKind::PadOnly => ClickMode::Off,
                _ => ClickMode::AlwaysOn,
            };
            final_items.push(SetlistItem {
                song_id: sanitize_setlist_slug(&planned.requested.title),
                song_dir: None,
                title: Some(planned.requested.title.clone()),
                original_key: Some(key),
                base_bpm: Some(planned.requested.bpm_override.unwrap_or(base_bpm)),
                base_click_mode: Some(base_click_mode),
                key_shift_semitones: 0,
                bpm_override: planned.requested.bpm_override,
                click_mode: planned.requested.click_mode,
                transition_mode: planned
                    .requested
                    .transition_mode
                    .unwrap_or(TransitionMode::AutoPause),
                crossfade_sec: planned.requested.crossfade_sec.unwrap_or(2.0).clamp(0.0, 10.0),
                ambient_pad_enabled: planned.requested.ambient_pad_enabled.unwrap_or(true),
                pad_key_override: planned.requested.pad_key_override.clone(),
                pad_lock,
                section_id: planned.requested.section_id.clone(),
                slot_kind,
                time_signature: planned.requested.time_signature,
                percussion_lock: planned.requested.percussion_lock.clone(),
                stem_mix: planned.requested.stem_mix.clone(),
            });
            continue;
        }

        let song_dir = match planned.status {
            SongResolutionStatus::ReadyFromLibrary => planned
                .matched_library_dir
                .clone()
                .ok_or_else(|| "Missing matched_library_dir".to_string())?,
            SongResolutionStatus::ReadyToIngest => {
                let (_, job_idx) = slot_to_job_idx
                    .iter()
                    .find(|(s, _)| *s == planned.slot)
                    .ok_or_else(|| "Missing ingest job mapping".to_string())?;
                ingest_result
                    .as_ref()
                    .and_then(|b| b.items.get(*job_idx))
                    .and_then(|r| r.output_dir.clone())
                    .ok_or_else(|| "Ingest completed without output_dir".to_string())?
            }
            _ => unreachable!(),
        };

        let manifest = SongManifest::from_file(&PathBuf::from(&song_dir).join("manifest.json"))
            .map_err(|e| format!("Failed to read manifest in {song_dir}: {e}"))?;

        let mut slot_notes = Vec::new();
        let (key_shift_semitones, _) = compute_shift_with_note(
            &manifest.original_key,
            planned.requested.target_key.as_deref(),
            &mut slot_notes,
        );
        warnings.extend(slot_notes);

        if manifest
            .provenance
            .as_ref()
            .is_some_and(|p| p.needs_review)
            || manifest.original_key == "Unknown"
        {
            review_required_songs.push(format!("{} ({})", manifest.title, song_dir));
        }

        final_items.push(SetlistItem {
            song_id: manifest.id.clone(),
            song_dir: Some(song_dir),
            title: Some(manifest.title.clone()),
            original_key: Some(manifest.original_key.clone()),
            base_bpm: Some(manifest.bpm),
            base_click_mode: Some(manifest.click_mode),
            key_shift_semitones,
            bpm_override: planned.requested.bpm_override,
            click_mode: planned.requested.click_mode,
            transition_mode: planned
                .requested
                .transition_mode
                .unwrap_or(TransitionMode::AutoPause),
            crossfade_sec: planned.requested.crossfade_sec.unwrap_or(2.0).clamp(0.0, 10.0),
            ambient_pad_enabled: planned.requested.ambient_pad_enabled.unwrap_or(true),
            pad_key_override: planned.requested.pad_key_override.clone(),
            pad_lock,
            section_id: planned.requested.section_id.clone(),
            slot_kind,
            time_signature: planned.requested.time_signature,
            percussion_lock: planned.requested.percussion_lock.clone(),
            stem_mix: planned.requested.stem_mix.clone(),
        });
    }

    let setlist_id = sanitize_setlist_slug(&req.setlist_name);
    let setlist = SetlistManifest {
        id: setlist_id.clone(),
        name: req.setlist_name.clone(),
        sections: req.sections.clone(),
        items: final_items,
        mixer: req.mixer.clone(),
    };
    setlist.validate().map_err(|e| e.to_string())?;

    let out_path = if let Some(ref explicit) = req.output_path.as_ref().filter(|s| !s.trim().is_empty())
    {
        PathBuf::from(explicit)
    } else {
        PathBuf::from(&lib_cfg.setlists_root).join(format!("{setlist_id}.json"))
    };
    save_setlist_manifest(&out_path, &setlist).map_err(|e| e.to_string())?;

    let out_path_str = out_path.to_string_lossy().to_string();
    record_recent_setlist(config_dir, lib_cfg, &out_path_str);

    Ok(SetlistBuildResult {
        built: true,
        needs_confirmation: false,
        setlist_path: Some(out_path_str),
        setlist: Some(setlist),
        plan,
        ingest: ingest_result,
        review_required_songs,
        warnings,
    })
}

pub fn create_or_update_setlist_from_items(
    req: &SetlistCreateRequest,
    lib_cfg: &LibraryConfig,
    config_dir: Option<&Path>,
) -> Result<(String, SetlistManifest, Vec<String>), String> {
    let mut warnings = Vec::new();
    let mut items = Vec::with_capacity(req.items.len());

    for (idx, input) in req.items.iter().enumerate() {
        let slot_kind = input.slot_kind.unwrap_or(SetlistSlotKind::SongBundle);
        let pad_lock = resolve_pad_lock_for_slot(
            input.pad_lock.as_ref(),
            input.pad_preset_id.as_deref(),
            config_dir,
            &mut warnings,
        );

        if matches!(
            slot_kind,
            SetlistSlotKind::PadOnly | SetlistSlotKind::PercussionGroove
        ) && input.song_dir.as_ref().is_none_or(|d| d.trim().is_empty())
        {
            let title = input
                .title
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| match slot_kind {
                    SetlistSlotKind::PadOnly => format!("Ambient Pad {}", idx + 1),
                    _ => format!("Percussion Groove {}", idx + 1),
                });
            let song_id = input
                .song_id
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| sanitize_setlist_slug(&title));
            let key = input
                .original_key
                .clone()
                .or_else(|| input.target_key.clone())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "C".to_string());
            let base_bpm = input.base_bpm.unwrap_or(match slot_kind {
                SetlistSlotKind::PercussionGroove => 120.0,
                _ => 72.0,
            });
            let base_click_mode = match slot_kind {
                SetlistSlotKind::PadOnly => ClickMode::Off,
                _ => ClickMode::AlwaysOn,
            };
            items.push(SetlistItem {
                song_id,
                song_dir: None,
                title: Some(title),
                original_key: Some(key),
                base_bpm: Some(base_bpm),
                base_click_mode: Some(base_click_mode),
                key_shift_semitones: input.key_shift_semitones.unwrap_or(0).clamp(-12, 12),
                bpm_override: input.bpm_override,
                click_mode: input.click_mode,
                transition_mode: input.transition_mode.unwrap_or(TransitionMode::AutoPause),
                crossfade_sec: input.crossfade_sec.unwrap_or(2.0).clamp(0.0, 10.0),
                ambient_pad_enabled: input.ambient_pad_enabled.unwrap_or(true),
                pad_key_override: input.pad_key_override.clone(),
                pad_lock,
                section_id: input.section_id.clone(),
                slot_kind,
                time_signature: input.time_signature,
                percussion_lock: input.percussion_lock.clone(),
                stem_mix: input.stem_mix.clone(),
            });
            continue;
        }

        let song_dir_str = input
            .song_dir
            .as_ref()
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                format!(
                    "Item {} requires 'song_dir' unless slot_kind is 'PadOnly' or 'PercussionGroove'",
                    idx + 1
                )
            })?
            .clone();
        let dir_path = PathBuf::from(&song_dir_str);
        let manifest = SongManifest::from_file(&dir_path.join("manifest.json"))
            .map_err(|e| format!("Invalid song bundle at '{song_dir_str}': {e}"))?;

        let shift = if let Some(explicit_shift) = input.key_shift_semitones {
            explicit_shift.clamp(-12, 12)
        } else if let Some(ref target) = input.target_key {
            let (s, _) = compute_shift_with_note(&manifest.original_key, Some(target), &mut warnings);
            s
        } else {
            0
        };

        items.push(SetlistItem {
            song_id: manifest.id.clone(),
            song_dir: Some(song_dir_str),
            title: input.title.clone().or_else(|| Some(manifest.title.clone())),
            original_key: Some(manifest.original_key.clone()),
            base_bpm: Some(manifest.bpm),
            base_click_mode: Some(manifest.click_mode),
            key_shift_semitones: shift,
            bpm_override: input.bpm_override,
            click_mode: input.click_mode,
            transition_mode: input.transition_mode.unwrap_or(TransitionMode::AutoPause),
            crossfade_sec: input.crossfade_sec.unwrap_or(2.0).clamp(0.0, 10.0),
            ambient_pad_enabled: input.ambient_pad_enabled.unwrap_or(true),
            pad_key_override: input.pad_key_override.clone(),
            pad_lock,
            section_id: input.section_id.clone(),
            slot_kind,
            time_signature: input.time_signature,
            percussion_lock: input.percussion_lock.clone(),
            stem_mix: input.stem_mix.clone(),
        });
    }

    let id = req
        .id
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| sanitize_setlist_slug(&req.name));
    let manifest = SetlistManifest {
        id: id.clone(),
        name: req.name.clone(),
        sections: req.sections.clone(),
        items,
        mixer: req.mixer.clone(),
    };
    manifest.validate().map_err(|e| e.to_string())?;

    let out_path = if let Some(ref explicit) = req.output_path.as_ref().filter(|s| !s.trim().is_empty())
    {
        PathBuf::from(explicit)
    } else {
        PathBuf::from(&lib_cfg.setlists_root).join(format!("{id}.json"))
    };
    save_setlist_manifest(&out_path, &manifest).map_err(|e| e.to_string())?;
    let out_str = out_path.to_string_lossy().to_string();
    record_recent_setlist(config_dir, lib_cfg, &out_str);
    Ok((out_str, manifest, warnings))
}

/// Saves a `SetlistManifest` to an explicit path or a unique `.json` file under
/// `lib_cfg.setlists_root`, creating parent directories and recording the file in `recent_setlists`.
pub fn save_managed_setlist(
    setlist: &SetlistManifest,
    explicit_path: Option<&Path>,
    lib_cfg: &LibraryConfig,
    config_dir: Option<&Path>,
) -> Result<String, String> {
    let mut normalized = setlist.clone();
    if normalized.id.trim().is_empty() {
        normalized.id = sanitize_setlist_slug(&normalized.name);
    }

    let out_path = if let Some(explicit) = explicit_path.filter(|p| !p.as_os_str().is_empty()) {
        explicit.to_path_buf()
    } else {
        let root = PathBuf::from(&lib_cfg.setlists_root);
        let slug = sanitize_setlist_slug(&normalized.name);
        let mut candidate = root.join(format!("{slug}.json"));
        let mut suffix = 2usize;
        while candidate.exists() {
            candidate = root.join(format!("{slug}-{suffix}.json"));
            suffix += 1;
        }
        candidate
    };

    save_setlist_manifest(&out_path, &normalized).map_err(|e| e.to_string())?;
    let out_str = out_path.to_string_lossy().to_string();
    record_recent_setlist(config_dir, lib_cfg, &out_str);
    Ok(out_str)
}

fn record_recent_setlist(config_dir: Option<&Path>, lib_cfg: &LibraryConfig, path: &str) {
    let Some(dir) = config_dir else {
        return;
    };
    let mut updated = lib_cfg.clone();
    updated.recent_setlists.retain(|p| p != path);
    updated.recent_setlists.insert(0, path.to_string());
    let _ = save_library_config(dir, updated);
}

#[cfg(test)]
mod tests {
    use super::*;
    use psalm_audio_core::loader::save_song_manifest;
    use psalm_audio_core::manifest::{SectionDef, StemDef};
    use std::collections::HashMap;

    fn write_test_wav(path: &Path) {
        let sr = 44_100u32;
        let frames = (2.0 * sr as f64) as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let s = ((2.0 * std::f32::consts::PI * 196.0 * (f as f32 / sr as f32)).sin()
                * 0.4
                * 32767.0) as i16;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + pcm.len()) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sr.to_le_bytes());
        wav.extend_from_slice(&(sr * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn test_plan_sunday_setlist_requires_confirmation_without_auto_approve() {
        let entries = vec![LibrarySongEntry {
            dir: "/lib/Gratitude".into(),
            status: LibraryEntryStatus::Ready,
            song_id: "gratitude".into(),
            title: "Gratitude".into(),
            artist: "Brandon Lake".into(),
            original_key: "B".into(),
            bpm: 78.0,
            time_signature: [4, 4],
            estimated_duration_sec: 320.0,
            section_names: vec!["Intro".into()],
            stem_names: vec!["music".into()],
            has_cues: false,
            missing_files: vec![],
            provenance: None,
        }];
        let lib_cfg = LibraryConfig {
            library_root: "/lib".into(),
            setlists_root: "/setlists".into(),
            linked_folders: vec![],
            recent_setlists: vec![],
        };

        let songs = vec![
            RequestedSetlistSong {
                title: "Gratitude".into(),
                artist: Some("Brandon Lake".into()),
                target_key: Some("G".into()),
                transition_mode: Some(TransitionMode::AutoAdvance),
                ambient_pad_enabled: Some(true),
                ..Default::default()
            },
            RequestedSetlistSong {
                title: "Goodness of God".into(),
                artist: Some("Bethel".into()),
                target_key: Some("A".into()),
                ..Default::default()
            },
        ];

        let mock_search = |_q: &str, _limit: usize| {
            Ok(vec![YouTubeSearchCandidate {
                video_id: "abc12345678".into(),
                title: "Goodness of God (Official)".into(),
                channel: "Bethel Music".into(),
                duration_sec: 295.0,
                webpage_url: "https://www.youtube.com/watch?v=abc12345678".into(),
                is_live: false,
                score: 0.95,
            }])
        };

        // Without auto_approve -> NeedsConfirmation
        let mut warnings = Vec::new();
        let mut items = Vec::new();
        let plan_manual = plan_with_entries_and_search(
            &SetlistPlanRequest {
                setlist_name: "Sunday Nov 15".into(),
                sections: Vec::new(),
                songs: songs.clone(),
                auto_approve: false,
                search_limit: 5,
            },
            &lib_cfg,
            &entries,
            mock_search,
            &mut warnings,
            &mut items,
        );
        assert!(!plan_manual.ready_to_build);
        assert_eq!(plan_manual.from_library_count, 1);
        assert_eq!(plan_manual.needs_confirmation_count, 1);
        assert_eq!(plan_manual.items[0].key_shift_semitones, -4); // B -> G
        assert_eq!(
            plan_manual.items[1].status,
            SongResolutionStatus::NeedsConfirmation
        );
        assert_eq!(plan_manual.items[1].youtube_candidates.len(), 1);

        // With auto_approve -> ReadyToIngest
        warnings.clear();
        items.clear();
        let plan_auto = plan_with_entries_and_search(
            &SetlistPlanRequest {
                setlist_name: "Sunday Nov 15".into(),
                sections: Vec::new(),
                songs,
                auto_approve: true,
                search_limit: 5,
            },
            &lib_cfg,
            &entries,
            mock_search,
            &mut warnings,
            &mut items,
        );
        assert!(plan_auto.ready_to_build);
        assert_eq!(plan_auto.to_ingest_count, 1);
        assert_eq!(
            plan_auto.items[1].selected_youtube_url.as_deref(),
            Some("https://www.youtube.com/watch?v=abc12345678")
        );
    }

    #[test]
    fn test_build_sunday_setlist_writes_valid_manifest_from_library_and_local_ingest() {
        let root = std::env::temp_dir().join(format!("psalm_build_setlist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lib_dir = root.join("Library");
        let setlists_dir = root.join("Setlists");
        let config_dir = root.join("Config");
        let src_dir = root.join("Raw");
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::create_dir_all(&setlists_dir).unwrap();
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&src_dir).unwrap();

        // Song 1 already in Library
        let song1_dir = lib_dir.join("Gratitude");
        std::fs::create_dir_all(&song1_dir).unwrap();
        write_test_wav(&song1_dir.join("music.wav"));
        let mut stems = HashMap::new();
        stems.insert("music".into(), StemDef::File("music.wav".into()));
        save_song_manifest(
            &song1_dir,
            &SongManifest {
                id: "gratitude".into(),
                title: "Gratitude".into(),
                artist: "Brandon Lake".into(),
                bpm: 78.0,
                time_signature: [4, 4],
                first_downbeat_sec: 0.0,
                original_key: "B".into(),
                click_mode: Default::default(),
                provenance: None,
                sections: vec![SectionDef {
                    id: "intro".into(),
                    name: "Intro".into(),
                    start_bar: 1,
                    end_bar: 5,
                    start_sec: None,
                    end_sec: None,
                    color: None,
                }],
                stems,
            },
        )
        .unwrap();

        // Song 2 from local WAV
        let song2_wav = src_dir.join("Praise.wav");
        write_test_wav(&song2_wav);

        let lib_cfg = LibraryConfig {
            library_root: lib_dir.to_string_lossy().to_string(),
            setlists_root: setlists_dir.to_string_lossy().to_string(),
            linked_folders: vec![],
            recent_setlists: vec![],
        };
        let wb_cfg = WorkbenchConfig::default();

        let res = build_sunday_setlist(
            &SetlistBuildRequest {
                setlist_name: "Sunday Nov 15".into(),
                sections: Vec::new(),
                songs: vec![
                    RequestedSetlistSong {
                        title: "Gratitude".into(),
                        artist: Some("Brandon Lake".into()),
                        target_key: Some("G".into()),
                        transition_mode: Some(TransitionMode::AutoAdvance),
                        ambient_pad_enabled: Some(true),
                        ..Default::default()
                    },
                    RequestedSetlistSong {
                        title: "Praise".into(),
                        artist: Some("Elevation".into()),
                        local_file: Some(song2_wav.to_string_lossy().to_string()),
                        bpm_override: Some(127.0),
                        ambient_pad_enabled: Some(true),
                        ..Default::default()
                    },
                ],
                mixer: None,
                auto_approve: false,
                output_path: None,
                separation_engine: None,
                stem_preset: None,
                custom_model: None,
                force_reextract: false,
            },
            &lib_cfg,
            &wb_cfg,
            Some(&config_dir),
            &crate::pipeline::NoopProgressSink,
        )
        .unwrap();

        assert!(res.built);
        assert!(!res.needs_confirmation);
        let out_path = res.setlist_path.unwrap();
        assert!(Path::new(&out_path).exists());
        let setlist = res.setlist.unwrap();
        assert_eq!(setlist.items.len(), 2);
        assert_eq!(setlist.items[0].key_shift_semitones, -4);
        assert_eq!(setlist.items[0].transition_mode, TransitionMode::AutoAdvance);
        assert_eq!(setlist.items[1].bpm_override, Some(127.0));
        assert!(!res.review_required_songs.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_sections_standalone_slots_pad_preset_and_save_managed_setlist() {
        use psalm_audio_core::manifest::ServiceSectionKind;
        use psalm_audio_core::percussion_patterns::{GrooveIntensity, PercussionMode, PercussionStyle};

        let root = std::env::temp_dir().join(format!("psalm_managed_set_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let setlists_dir = root.join("Setlists");
        let config_dir = root.join("Config");
        std::fs::create_dir_all(&config_dir).unwrap();

        let lib_cfg = LibraryConfig {
            library_root: root.join("Library").to_string_lossy().to_string(),
            setlists_root: setlists_dir.to_string_lossy().to_string(),
            linked_folders: vec![],
            recent_setlists: vec![],
        };

        let req = SetlistCreateRequest {
            name: "Worship Night".into(),
            id: None,
            output_path: None,
            sections: vec![SetlistSection {
                id: "sec-ministry".into(),
                name: "Ministry".into(),
                kind: ServiceSectionKind::Ministry,
                color: Some("#22d3ee".into()),
                collapsed: false,
                note: None,
            }],
            items: vec![
                ManualSetlistItemInput {
                    title: Some("Prayer Underscore".into()),
                    slot_kind: Some(SetlistSlotKind::PadOnly),
                    section_id: Some("sec-ministry".into()),
                    target_key: Some("G".into()),
                    pad_preset_id: Some("shimmer-cathedral".into()),
                    transition_mode: Some(TransitionMode::ContinueUntilStopped),
                    ..Default::default()
                },
                ManualSetlistItemInput {
                    title: Some("Offering Groove".into()),
                    slot_kind: Some(SetlistSlotKind::PercussionGroove),
                    section_id: Some("sec-ministry".into()),
                    base_bpm: Some(124.0),
                    time_signature: Some([4, 4]),
                    percussion_lock: Some(PercussionLockConfig {
                        mode: PercussionMode::PercussionOnly,
                        style: PercussionStyle::Makossa,
                        intensity: GrooveIntensity::High,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            mixer: Some(SetlistMixerConfig {
                master_gain: 0.95,
                backing_gain: 0.88,
                click_gain: 0.75,
                click_muted: Some(false),
            }),
        };

        let (path1, manifest, warnings) =
            create_or_update_setlist_from_items(&req, &lib_cfg, Some(&config_dir)).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(manifest.sections.len(), 1);
        assert_eq!(manifest.items.len(), 2);
        assert_eq!(manifest.items[0].slot_kind, SetlistSlotKind::PadOnly);
        assert_eq!(
            manifest.items[0]
                .pad_lock
                .as_ref()
                .and_then(|l| l.preset_id.as_deref()),
            Some("shimmer-cathedral")
        );
        assert_eq!(
            manifest.items[1].slot_kind,
            SetlistSlotKind::PercussionGroove
        );
        assert_eq!(
            manifest.items[1].percussion_lock.as_ref().map(|p| p.style),
            Some(PercussionStyle::Makossa)
        );
        assert_eq!(manifest.mixer.as_ref().map(|m| m.master_gain), Some(0.95));

        // Saving again via save_managed_setlist without explicit path produces unique filename
        let path2 = save_managed_setlist(&manifest, None, &lib_cfg, Some(&config_dir)).unwrap();
        assert_ne!(path1, path2);
        assert!(path2.ends_with("worship-night-2.json"));
        assert!(Path::new(&path2).exists());

        // Saving with explicit path overwrites cleanly
        let path3 =
            save_managed_setlist(&manifest, Some(Path::new(&path1)), &lib_cfg, Some(&config_dir))
                .unwrap();
        assert_eq!(path3, path1);

        let _ = std::fs::remove_dir_all(&root);
    }
}

