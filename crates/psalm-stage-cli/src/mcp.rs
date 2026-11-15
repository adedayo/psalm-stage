use std::io::{BufRead, Write};
use std::path::Path;

use psalm_audio_core::library::{list_managed_setlists, match_library_songs, LibraryEntryStatus};
use psalm_audio_core::loader::load_setlist_manifest;
use psalm_workbench::{
    build_sunday_setlist, create_or_update_setlist_from_items, detect_toolchain_status,
    ingest_songs_batch, plan_sunday_setlist, resolve_app_config_dir, resolve_effective_separation,
    BatchIngestRequest, ImportJobRequest, ImportSource, ManualSetlistItemInput, NoopProgressSink,
    RequestedSetlistSong, SeparationEngine, SetlistBuildRequest, SetlistCreateRequest,
    SetlistPlanRequest, StemPreset, WorkbenchManager, BUNDLED_SKILL_MD,
};
use serde_json::{json, Value};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

pub fn run_mcp_stdio<R: BufRead, W: Write>(reader: R, mut writer: W) -> Result<(), String> {
    let config_dir = resolve_app_config_dir();
    let manager = WorkbenchManager::new(config_dir);

    for line in reader.lines() {
        let raw = line.map_err(|e| format!("MCP stdin read error: {e}"))?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<Value>(trimmed) else {
            let err_resp = json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": "Parse error" }
            });
            writeln!(writer, "{err_resp}").map_err(|e| e.to_string())?;
            writer.flush().map_err(|e| e.to_string())?;
            continue;
        };

        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or_else(|| json!({}));

        // Notifications without an `id` do not receive a JSON-RPC response.
        let Some(req_id) = id else {
            continue;
        };

        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "protocolVersion": MCP_PROTOCOL_VERSION,
                    "capabilities": {
                        "tools": { "listChanged": false }
                    },
                    "serverInfo": {
                        "name": "psalm-stage",
                        "version": env!("CARGO_PKG_VERSION")
                    }
                }
            }),
            "ping" => json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {}
            }),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {
                    "tools": mcp_tool_definitions()
                }
            }),
            "tools/call" => {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
                match dispatch_tool_call(&manager, name, args) {
                    Ok(payload) => json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "content": [{
                                "type": "text",
                                "text": serde_json::to_string_pretty(&payload).unwrap_or_else(|_| payload.to_string())
                            }],
                            "structuredContent": payload
                        }
                    }),
                    Err(err_msg) => json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "result": {
                            "isError": true,
                            "content": [{
                                "type": "text",
                                "text": err_msg
                            }]
                        }
                    }),
                }
            }
            _ => json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": {
                    "code": -32601,
                    "message": format!("Method not found: {method}")
                }
            }),
        };

        writeln!(writer, "{response}").map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn mcp_tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "get_toolchain_status",
            "description": "Inspect Psalm Stage library/setlists directories, default separation config, and availability of ffmpeg, yt-dlp, audio-separator, and demucs.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "list_library_songs",
            "description": "List songs indexed in the local Psalm Stage Library with optional text query and status filter.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Optional substring filter across title, artist, key, or stems" },
                    "status": { "type": "string", "enum": ["Ready", "NeedsSetup", "MissingStems"] }
                }
            }
        }),
        json!({
            "name": "match_library_song",
            "description": "Fuzzy-match a song title/artist or YouTube URL against the local Library.",
            "inputSchema": {
                "type": "object",
                "required": ["title"],
                "properties": {
                    "title": { "type": "string" },
                    "artist": { "type": "string" },
                    "youtube_url": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "search_youtube_tracks",
            "description": "Search YouTube via yt-dlp for a song and return ranked audio candidates for user confirmation.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 10, "default": 5 }
                }
            }
        }),
        json!({
            "name": "plan_sunday_setlist",
            "description": "Dry-run plan a Sunday setlist: match Library songs, search YouTube for missing unlinked songs, compute key shifts, and flag items needing confirmation before any downloads occur.",
            "inputSchema": {
                "type": "object",
                "required": ["songs"],
                "properties": {
                    "setlist_name": { "type": "string" },
                    "auto_approve": { "type": "boolean", "default": false },
                    "search_limit": { "type": "integer", "default": 5 },
                    "songs": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["title"],
                            "properties": {
                                "title": { "type": "string" },
                                "artist": { "type": "string" },
                                "youtube_url": { "type": "string" },
                                "local_file": { "type": "string" },
                                "target_key": { "type": "string" },
                                "bpm_override": { "type": "number" },
                                "transition_mode": { "type": "string", "enum": ["AutoPause", "AutoAdvance", "Manual", "ContinueUntilStopped"] },
                                "crossfade_sec": { "type": "number" },
                                "ambient_pad_enabled": { "type": "boolean" },
                                "pad_key_override": { "type": "string" },
                                "click_mode": { "type": "string", "enum": ["AlwaysOn", "IntroOnly", "Off"] }
                            }
                        }
                    }
                }
            }
        }),
        json!({
            "name": "ingest_song",
            "description": "Download/import a single song from a YouTube URL or local audio file through the Workbench pipeline (uses default separation with stereo fallback unless specified; skips if cached).",
            "inputSchema": {
                "type": "object",
                "required": ["source_kind", "source_value"],
                "properties": {
                    "source_kind": { "type": "string", "enum": ["YouTubeUrl", "LocalFile"] },
                    "source_value": { "type": "string" },
                    "song_title": { "type": "string" },
                    "artist": { "type": "string" },
                    "separation_engine": { "type": "string", "enum": ["None", "AudioSeparator", "Demucs"] },
                    "stem_preset": { "type": "string", "enum": ["TwoStems", "FourStems", "SixStems", "Custom"] },
                    "force_reextract": { "type": "boolean", "default": false }
                }
            }
        }),
        json!({
            "name": "ingest_songs_batch",
            "description": "Sequentially ingest multiple confirmed YouTube URLs or local files into the Song Library.",
            "inputSchema": {
                "type": "object",
                "required": ["jobs"],
                "properties": {
                    "jobs": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["source_kind", "source_value"],
                            "properties": {
                                "source_kind": { "type": "string", "enum": ["YouTubeUrl", "LocalFile"] },
                                "source_value": { "type": "string" },
                                "song_title": { "type": "string" },
                                "artist": { "type": "string" }
                            }
                        }
                    },
                    "use_default_with_fallback": { "type": "boolean", "default": true }
                }
            }
        }),
        json!({
            "name": "create_or_update_setlist",
            "description": "Either execute a full Sunday setlist build (when 'songs' is provided, ingesting confirmed missing songs and writing the setlist JSON) OR assemble a setlist directly from Library 'items' with song_dir.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "setlist_name": { "type": "string" },
                    "name": { "type": "string" },
                    "output_path": { "type": "string" },
                    "auto_approve": { "type": "boolean", "default": false },
                    "separation_engine": { "type": "string", "enum": ["None", "AudioSeparator", "Demucs"] },
                    "stem_preset": { "type": "string", "enum": ["TwoStems", "FourStems", "SixStems", "Custom"] },
                    "force_reextract": { "type": "boolean", "default": false },
                    "songs": { "type": "array" },
                    "items": { "type": "array" }
                }
            }
        }),
        json!({
            "name": "list_setlists",
            "description": "List managed setlists stored in setlists_root and recent_setlists.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "get_setlist",
            "description": "Load and inspect a saved setlist JSON file by path.",
            "inputSchema": {
                "type": "object",
                "required": ["path"],
                "properties": { "path": { "type": "string" } }
            }
        }),
        json!({
            "name": "get_ai_workflow_guide",
            "description": "Return the canonical Psalm Stage Sunday setlist workflow instructions (SKILL.md).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
    ]
}

fn dispatch_tool_call(manager: &WorkbenchManager, name: &str, args: Value) -> Result<Value, String> {
    let lib_cfg = manager.get_library_config();
    let wb_cfg = manager.get_workbench_config();

    match name {
        "get_toolchain_status" => {
            let tools = detect_toolchain_status(&wb_cfg);
            Ok(json!({
                "config_dir": manager.config_dir().to_string_lossy(),
                "library_root": lib_cfg.library_root,
                "setlists_root": lib_cfg.setlists_root,
                "workbench_config": wb_cfg,
                "toolchain": tools,
            }))
        }
        "list_library_songs" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty());
            let status_filter = args
                .get("status")
                .and_then(|v| v.as_str())
                .and_then(|s| match s {
                    "Ready" => Some(LibraryEntryStatus::Ready),
                    "NeedsSetup" => Some(LibraryEntryStatus::NeedsSetup),
                    "MissingStems" => Some(LibraryEntryStatus::MissingStems),
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
                            || e.stem_names
                                .iter()
                                .any(|st| st.to_ascii_lowercase().contains(q))
                    })
                })
                .collect();
            Ok(json!({ "count": entries.len(), "entries": entries }))
        }
        "match_library_song" => {
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'title'".to_string())?;
            let artist = args.get("artist").and_then(|v| v.as_str());
            let youtube_url = args.get("youtube_url").and_then(|v| v.as_str());
            let entries = manager.scan_library();
            let matches = match_library_songs(&entries, title, artist, youtube_url);
            Ok(json!({ "matches": matches }))
        }
        "search_youtube_tracks" => {
            let query = args
                .get("query")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'query'".to_string())?;
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
            let candidates = manager.search_youtube(query, limit)?;
            Ok(json!({ "query": query, "candidates": candidates }))
        }
        "plan_sunday_setlist" => {
            let req: SetlistPlanRequest = serde_json::from_value(args)
                .map_err(|e| format!("Invalid plan_sunday_setlist arguments: {e}"))?;
            let plan = plan_sunday_setlist(&req, &lib_cfg, &wb_cfg);
            serde_json::to_value(plan).map_err(|e| e.to_string())
        }
        "ingest_song" => {
            let kind = args
                .get("source_kind")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'source_kind'".to_string())?;
            let val = args
                .get("source_value")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'source_value'".to_string())?;
            let source = match kind {
                "YouTubeUrl" => ImportSource::YouTubeUrl(val.to_string()),
                "LocalFile" => ImportSource::LocalFile(val.to_string()),
                other => return Err(format!("Unsupported source_kind '{other}'")),
            };
            let explicit_engine: Option<SeparationEngine> = args
                .get("separation_engine")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let explicit_preset: Option<StemPreset> = args
                .get("stem_preset")
                .and_then(|v| serde_json::from_value(v.clone()).ok());
            let tools = detect_toolchain_status(&wb_cfg);
            let (eff_engine, eff_preset, warn) =
                resolve_effective_separation(explicit_engine, explicit_preset, &wb_cfg, &tools);
            let job = ImportJobRequest {
                source,
                song_title: args
                    .get("song_title")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                artist: args
                    .get("artist")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                target_dir: None,
                separation_engine: eff_engine,
                stem_preset: eff_preset,
                custom_model: None,
                force_reextract: args
                    .get("force_reextract")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            };
            let mut batch = ingest_songs_batch(
                BatchIngestRequest {
                    jobs: vec![job],
                    use_default_with_fallback: false,
                },
                &lib_cfg,
                &wb_cfg,
                &NoopProgressSink,
            );
            if let Some(w) = warn {
                if let Some(first) = batch.items.first_mut() {
                    first.warnings.insert(0, w);
                }
            }
            serde_json::to_value(batch).map_err(|e| e.to_string())
        }
        "ingest_songs_batch" => {
            let jobs_val = args
                .get("jobs")
                .and_then(|v| v.as_array())
                .ok_or_else(|| "Missing 'jobs' array".to_string())?;
            let use_fallback = args
                .get("use_default_with_fallback")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let mut jobs = Vec::with_capacity(jobs_val.len());
            for j in jobs_val {
                let kind = j
                    .get("source_kind")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Job missing 'source_kind'".to_string())?;
                let val = j
                    .get("source_value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Job missing 'source_value'".to_string())?;
                let source = match kind {
                    "YouTubeUrl" => ImportSource::YouTubeUrl(val.to_string()),
                    "LocalFile" => ImportSource::LocalFile(val.to_string()),
                    other => return Err(format!("Unsupported source_kind '{other}'")),
                };
                jobs.push(ImportJobRequest {
                    source,
                    song_title: j
                        .get("song_title")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    artist: j.get("artist").and_then(|v| v.as_str()).map(str::to_string),
                    target_dir: None,
                    separation_engine: wb_cfg.default_separation_engine,
                    stem_preset: wb_cfg.default_stem_preset,
                    custom_model: None,
                    force_reextract: false,
                });
            }
            let res = ingest_songs_batch(
                BatchIngestRequest {
                    jobs,
                    use_default_with_fallback: use_fallback,
                },
                &lib_cfg,
                &wb_cfg,
                &NoopProgressSink,
            );
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "create_or_update_setlist" => {
            if let Some(songs_val) = args.get("songs") {
                let songs: Vec<RequestedSetlistSong> = serde_json::from_value(songs_val.clone())
                    .map_err(|e| format!("Invalid 'songs' array: {e}"))?;
                let setlist_name = args
                    .get("setlist_name")
                    .or_else(|| args.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Sunday Service Set")
                    .to_string();
                let build_req = SetlistBuildRequest {
                    setlist_name,
                    songs,
                    auto_approve: args
                        .get("auto_approve")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    output_path: args
                        .get("output_path")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    separation_engine: args
                        .get("separation_engine")
                        .and_then(|v| serde_json::from_value(v.clone()).ok()),
                    stem_preset: args
                        .get("stem_preset")
                        .and_then(|v| serde_json::from_value(v.clone()).ok()),
                    custom_model: None,
                    force_reextract: args
                        .get("force_reextract")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                };
                let res = build_sunday_setlist(
                    &build_req,
                    &lib_cfg,
                    &wb_cfg,
                    Some(manager.config_dir()),
                    &NoopProgressSink,
                )?;
                serde_json::to_value(res).map_err(|e| e.to_string())
            } else {
                let items_val = args
                    .get("items")
                    .ok_or_else(|| "Provide either 'songs' or 'items'".to_string())?;
                let items: Vec<ManualSetlistItemInput> =
                    serde_json::from_value(items_val.clone())
                        .map_err(|e| format!("Invalid 'items' array: {e}"))?;
                let name = args
                    .get("name")
                    .or_else(|| args.get("setlist_name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("Sunday Service Set")
                    .to_string();
                let req = SetlistCreateRequest {
                    name,
                    id: None,
                    output_path: args
                        .get("output_path")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    items,
                };
                let (path, manifest, warnings) = create_or_update_setlist_from_items(
                    &req,
                    &lib_cfg,
                    Some(manager.config_dir()),
                )?;
                Ok(json!({
                    "built": true,
                    "setlist_path": path,
                    "setlist": manifest,
                    "warnings": warnings,
                }))
            }
        }
        "list_setlists" => {
            let list = list_managed_setlists(&lib_cfg);
            Ok(json!({ "count": list.len(), "setlists": list }))
        }
        "get_setlist" => {
            let path = args
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'path'".to_string())?;
            let manifest = load_setlist_manifest(Path::new(path)).map_err(|e| e.to_string())?;
            serde_json::to_value(manifest).map_err(|e| e.to_string())
        }
        "get_ai_workflow_guide" => Ok(json!({
            "skill_name": "psalm-stage-setlist",
            "markdown": BUNDLED_SKILL_MD
        })),
        other => Err(format!("Unknown MCP tool '{other}'")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_mcp_stdio_initialize_and_tools_list() {
        let input = concat!(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
            "\n",
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            "\n",
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"get_ai_workflow_guide","arguments":{}}}"#,
            "\n"
        );
        let reader = Cursor::new(input.as_bytes());
        let mut output: Vec<u8> = Vec::new();
        run_mcp_stdio(reader, &mut output).unwrap();

        let text = String::from_utf8(output).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 3); // notification produced no response
        assert_eq!(lines[0]["id"], 1);
        assert_eq!(lines[0]["result"]["serverInfo"]["name"], "psalm-stage");

        assert_eq!(lines[1]["id"], 2);
        let tools = lines[1]["result"]["tools"].as_array().unwrap();
        assert!(tools.iter().any(|t| t["name"] == "plan_sunday_setlist"));
        assert!(tools.iter().any(|t| t["name"] == "create_or_update_setlist"));

        assert_eq!(lines[2]["id"], 3);
        assert!(lines[2]["result"]["structuredContent"]["markdown"]
            .as_str()
            .unwrap()
            .contains("Psalm Stage Sunday Setlist Workflow"));
    }
}

