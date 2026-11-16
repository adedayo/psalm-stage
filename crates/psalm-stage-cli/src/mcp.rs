use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};

use psalm_audio_core::library::{list_managed_setlists, match_library_songs, LibraryEntryStatus};
use psalm_audio_core::loader::{
    inspect_song_folder, load_setlist_manifest, save_song_manifest,
};
use psalm_audio_core::manifest::{
    ClickMode, SectionDef, SetlistManifest, SongManifest, StemDef,
};
use psalm_audio_core::pad::{PadRegister, PadTonality, PadVoiceArchetype, MAX_PAD_LAYERS};
use psalm_audio_core::pad_presets::{
    delete_user_pad_preset, list_all_pad_presets, save_user_pad_preset, PadPreset,
};
use psalm_audio_core::percussion::DEFAULT_PERCUSSION_ROOM_MIX;
use psalm_audio_core::percussion_kit::{
    default_percussion_kits_root, default_percussion_loops_root, delete_percussion_loop,
    export_percussion_loop_bundle, generate_studio_percussion_kit, list_percussion_kits,
    list_percussion_loops, save_percussion_loop, ExportLoopBundleRequest, StudioKitProfile,
};
use psalm_audio_core::percussion_patterns::{
    CustomPercussionPattern, GrooveIntensity, HitVariant, PercussionMode, PercussionStyle,
    ALL_PERCUSSION_INSTRUMENTS,
};
use psalm_audio_core::session::{load_stage_session, StageSessionState, STAGE_SESSION_FILENAME};
use psalm_workbench::{
    build_sunday_setlist, create_or_update_setlist_from_items, detect_toolchain_status,
    ingest_songs_batch, plan_sunday_setlist, resolve_app_config_dir, resolve_effective_separation,
    save_managed_setlist, BatchIngestRequest, ImportJobRequest, ImportSource,
    ManualSetlistItemInput, NoopProgressSink, RequestedSetlistSong, SeparationEngine,
    SetlistBuildRequest, SetlistCreateRequest, SetlistPlanRequest, StemPreset, WorkbenchManager,
    BUNDLED_SKILL_MD,
};
use serde_json::{json, Value};

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

const TRANSITION_MODES: [&str; 4] = [
    "AutoPause",
    "AutoAdvance",
    "Manual",
    "ContinueUntilStopped",
];
const CLICK_MODES: [&str; 3] = ["AlwaysOn", "IntroOnly", "Off"];
const SLOT_KINDS: [&str; 3] = ["SongBundle", "PadOnly", "PercussionGroove"];
const SERVICE_SECTION_KINDS: [&str; 7] = [
    "Praise",
    "Worship",
    "Sermonic",
    "Sermon",
    "Ministry",
    "Offering",
    "Custom",
];
const PAD_ARCHETYPES: [&str; 8] = [
    "WarmAnalog",
    "BrightSynth",
    "EvolvingMotion",
    "StringsCello",
    "OrganPad",
    "GuitarSwell",
    "ReversePiano",
    "AiryChoir",
];
const PAD_REGISTERS: [&str; 3] = ["Low", "Mid", "High"];
const PAD_TONALITIES: [&str; 5] = ["Auto", "OpenFifth", "Major", "Minor", "Melodic"];
const PERCUSSION_MODES: [&str; 3] = ["Off", "Layer", "PercussionOnly"];
const PERCUSSION_STYLES: [&str; 12] = [
    "Worship44",
    "Worship68",
    "Afrobeat",
    "HighlifePraise",
    "Woro",
    "Makossa",
    "Amapiano",
    "FunkPocket",
    "GospelPraise",
    "AcousticCajon",
    "LatinCaribbean",
    "ReggaeSoca",
];
const GROOVE_INTENSITIES: [&str; 3] = ["Low", "Medium", "High"];
const PERCUSSION_INSTRUMENTS: [&str; 9] = [
    "Kick",
    "Snare",
    "HiHat",
    "HandDrums",
    "Tambourine",
    "Shaker",
    "Claps",
    "Clave",
    "Bell",
];
const HIT_VARIANTS: [&str; 4] = ["Standard", "Accent", "Soft", "Special"];

fn setlist_section_schema() -> Value {
    json!({
        "type": "object",
        "required": ["id", "name"],
        "properties": {
            "id": { "type": "string" },
            "name": { "type": "string" },
            "kind": { "type": "string", "enum": SERVICE_SECTION_KINDS },
            "color": { "type": "string" },
            "collapsed": { "type": "boolean" },
            "note": { "type": "string" }
        }
    })
}

fn pad_layer_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "enabled": { "type": "boolean" },
            "archetype": { "type": "string", "enum": PAD_ARCHETYPES },
            "register": { "type": "string", "enum": PAD_REGISTERS },
            "tonality": { "type": "string", "enum": PAD_TONALITIES },
            "gain": { "type": "number", "minimum": 0.0, "maximum": 1.0 }
        }
    })
}

fn pad_lock_schema() -> Value {
    json!({
        "type": "object",
        "description": "Locks Tonic Pad Player voice preset & tone to a slot without locking key.",
        "properties": {
            "enabled": { "type": "boolean" },
            "gain": { "type": "number", "minimum": 0.0, "maximum": 1.5 },
            "preset_id": { "type": "string" },
            "preset_name": { "type": "string" },
            "layers": { "type": "array", "maxItems": MAX_PAD_LAYERS, "items": pad_layer_schema() },
            "brightness": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
            "shimmer": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
            "pan": { "type": "number", "minimum": -1.0, "maximum": 1.0 }
        }
    })
}

fn custom_percussion_pattern_schema() -> Value {
    json!({
        "type": "object",
        "required": ["id", "name", "hits"],
        "properties": {
            "id": { "type": "string" },
            "name": { "type": "string" },
            "steps_per_bar": { "type": "integer", "enum": [12, 16, 24], "default": 16 },
            "bars_in_cycle": { "type": "integer", "minimum": 1, "maximum": 4, "default": 2 },
            "swing": { "type": "number", "minimum": 0.0, "maximum": 0.35, "default": 0.0 },
            "is_compound": { "type": "boolean", "default": false },
            "hits": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["step", "instrument", "velocity"],
                    "properties": {
                        "step": { "type": "integer", "minimum": 0, "maximum": 95 },
                        "instrument": { "type": "string", "enum": PERCUSSION_INSTRUMENTS },
                        "velocity": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                        "variant": { "type": "string", "enum": HIT_VARIANTS },
                        "min_intensity": { "type": "string", "enum": GROOVE_INTENSITIES }
                    }
                }
            }
        }
    })
}

fn percussion_lock_schema() -> Value {
    json!({
        "type": "object",
        "description": "Locks beat-synced Percussion Track groove/style/intensity/kit configuration onto a setlist slot.",
        "properties": {
            "mode": { "type": "string", "enum": PERCUSSION_MODES },
            "style": { "type": "string", "enum": PERCUSSION_STYLES },
            "intensity": { "type": "string", "enum": GROOVE_INTENSITIES },
            "gain": { "type": "number", "minimum": 0.0, "maximum": 1.5 },
            "iem_send": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
            "follow_sections": { "type": "boolean" },
            "room_mix": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
            "kit_name": { "type": "string" },
            "kit_dir": { "type": "string" },
            "custom_pattern": custom_percussion_pattern_schema(),
            "channels": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["instrument", "enabled", "gain", "pan"],
                    "properties": {
                        "instrument": { "type": "string", "enum": PERCUSSION_INSTRUMENTS },
                        "enabled": { "type": "boolean" },
                        "gain": { "type": "number", "minimum": 0.0, "maximum": 1.5 },
                        "pan": { "type": "number", "minimum": -1.0, "maximum": 1.0 }
                    }
                }
            }
        }
    })
}

fn stem_mix_schema() -> Value {
    json!({
        "type": "array",
        "items": {
            "type": "object",
            "required": ["name"],
            "properties": {
                "name": { "type": "string" },
                "gain": { "type": "number", "minimum": 0.0, "maximum": 1.5 },
                "muted": { "type": "boolean" }
            }
        }
    })
}

fn mixer_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "master_gain": { "type": "number", "minimum": 0.0, "maximum": 1.5 },
            "backing_gain": { "type": "number", "minimum": 0.0, "maximum": 1.5 },
            "click_gain": { "type": "number", "minimum": 0.0, "maximum": 1.5 },
            "click_muted": { "type": "boolean" }
        }
    })
}

fn requested_song_schema() -> Value {
    json!({
        "type": "object",
        "required": ["title"],
        "properties": {
            "title": { "type": "string" },
            "artist": { "type": "string" },
            "youtube_url": { "type": "string" },
            "local_file": { "type": "string" },
            "slot_kind": { "type": "string", "enum": SLOT_KINDS, "default": "SongBundle" },
            "section_id": { "type": "string", "description": "Matches a SetlistSection id from 'sections'." },
            "target_key": { "type": "string" },
            "bpm_override": { "type": "number", "minimum": 30.0, "maximum": 260.0 },
            "time_signature": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2 },
            "transition_mode": { "type": "string", "enum": TRANSITION_MODES },
            "crossfade_sec": { "type": "number", "minimum": 0.0, "maximum": 10.0 },
            "ambient_pad_enabled": { "type": "boolean" },
            "pad_key_override": { "type": "string" },
            "pad_preset_id": { "type": "string", "description": "Convenience preset ID or name (factory or user) to auto-build pad_lock if pad_lock is omitted." },
            "pad_lock": pad_lock_schema(),
            "click_mode": { "type": "string", "enum": CLICK_MODES },
            "percussion_lock": percussion_lock_schema(),
            "stem_mix": stem_mix_schema()
        }
    })
}

fn manual_item_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "song_dir": { "type": "string", "description": "Required for SongBundle slots; omit for standalone PadOnly or PercussionGroove slots." },
            "song_id": { "type": "string" },
            "title": { "type": "string" },
            "original_key": { "type": "string" },
            "base_bpm": { "type": "number", "minimum": 30.0, "maximum": 260.0 },
            "slot_kind": { "type": "string", "enum": SLOT_KINDS, "default": "SongBundle" },
            "section_id": { "type": "string" },
            "target_key": { "type": "string" },
            "key_shift_semitones": { "type": "integer", "minimum": -12, "maximum": 12 },
            "bpm_override": { "type": "number", "minimum": 30.0, "maximum": 260.0 },
            "time_signature": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2 },
            "transition_mode": { "type": "string", "enum": TRANSITION_MODES },
            "crossfade_sec": { "type": "number", "minimum": 0.0, "maximum": 10.0 },
            "ambient_pad_enabled": { "type": "boolean" },
            "pad_key_override": { "type": "string" },
            "pad_preset_id": { "type": "string" },
            "pad_lock": pad_lock_schema(),
            "click_mode": { "type": "string", "enum": CLICK_MODES },
            "percussion_lock": percussion_lock_schema(),
            "stem_mix": stem_mix_schema()
        }
    })
}

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
            "description": "Dry-run plan a Sunday setlist: match Library songs, support Service Sections and standalone PadOnly/PercussionGroove slots, search YouTube for missing unlinked songs, compute key shifts, and flag items needing confirmation before any downloads occur.",
            "inputSchema": {
                "type": "object",
                "required": ["songs"],
                "properties": {
                    "setlist_name": { "type": "string" },
                    "sections": { "type": "array", "items": setlist_section_schema() },
                    "auto_approve": { "type": "boolean", "default": false },
                    "search_limit": { "type": "integer", "default": 5 },
                    "songs": {
                        "type": "array",
                        "items": requested_song_schema()
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
                    "custom_model": { "type": "string" },
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
                                "artist": { "type": "string" },
                                "separation_engine": { "type": "string", "enum": ["None", "AudioSeparator", "Demucs"] },
                                "stem_preset": { "type": "string", "enum": ["TwoStems", "FourStems", "SixStems", "Custom"] },
                                "custom_model": { "type": "string" },
                                "force_reextract": { "type": "boolean", "default": false }
                            }
                        }
                    },
                    "use_default_with_fallback": { "type": "boolean", "default": true }
                }
            }
        }),
        json!({
            "name": "create_or_update_setlist",
            "description": "Either execute a full Sunday setlist build (when 'songs' is provided, ingesting confirmed missing songs and writing the setlist JSON) OR assemble/update a setlist directly from 'items' (SongBundle paths plus standalone PadOnly/PercussionGroove slots), supporting Service Sections, Tonic Pad Locks, Percussion Track Locks, Stem Mix overrides, and Setlist Mixer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "setlist_name": { "type": "string" },
                    "name": { "type": "string" },
                    "id": { "type": "string" },
                    "output_path": { "type": "string" },
                    "sections": { "type": "array", "items": setlist_section_schema() },
                    "mixer": mixer_schema(),
                    "auto_approve": { "type": "boolean", "default": false },
                    "separation_engine": { "type": "string", "enum": ["None", "AudioSeparator", "Demucs"] },
                    "stem_preset": { "type": "string", "enum": ["TwoStems", "FourStems", "SixStems", "Custom"] },
                    "custom_model": { "type": "string" },
                    "force_reextract": { "type": "boolean", "default": false },
                    "songs": { "type": "array", "items": requested_song_schema() },
                    "items": { "type": "array", "items": manual_item_schema() }
                }
            }
        }),
        json!({
            "name": "save_managed_setlist",
            "description": "Save a complete SetlistManifest object into Managed Setlists (setlists_root) with collision-safe unique naming (or overwrite a specified path) and record it in recent_setlists. Useful for archiving a current setlist before starting a new one.",
            "inputSchema": {
                "type": "object",
                "required": ["setlist"],
                "properties": {
                    "path": { "type": "string", "description": "Optional explicit destination .json path. If omitted, saves a unique file inside setlists_root." },
                    "setlist": {
                        "type": "object",
                        "required": ["name", "items"],
                        "properties": {
                            "id": { "type": "string" },
                            "name": { "type": "string" },
                            "sections": { "type": "array", "items": setlist_section_schema() },
                            "mixer": mixer_schema(),
                            "items": { "type": "array" }
                        }
                    }
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
            "description": "Load and inspect a saved setlist JSON file by explicit path or by managed setlist id/name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "id": { "type": "string", "description": "Optional managed setlist id if path is omitted" },
                    "name": { "type": "string", "description": "Optional managed setlist name if path/id is omitted" }
                }
            }
        }),
        json!({
            "name": "get_song_manifest",
            "description": "Inspect a Library song folder (manifest.json, sections, stems, provenance, estimated duration, and available audio files).",
            "inputSchema": {
                "type": "object",
                "required": ["dir"],
                "properties": {
                    "dir": { "type": "string", "description": "Path to the song bundle directory" }
                }
            }
        }),
        json!({
            "name": "update_song_manifest",
            "description": "Update editable metadata on an existing song bundle manifest.json (title, artist, bpm, time_signature, first_downbeat_sec, original_key, click_mode, sections, stem gain/mute/pitched overrides, or clear needs_review).",
            "inputSchema": {
                "type": "object",
                "required": ["dir"],
                "properties": {
                    "dir": { "type": "string" },
                    "title": { "type": "string" },
                    "artist": { "type": "string" },
                    "bpm": { "type": "number", "minimum": 11.0, "maximum": 300.0 },
                    "time_signature": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2 },
                    "first_downbeat_sec": { "type": "number", "minimum": 0.0 },
                    "original_key": { "type": "string" },
                    "click_mode": { "type": "string", "enum": CLICK_MODES },
                    "clear_needs_review": { "type": "boolean" },
                    "sections": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "required": ["id", "name"],
                            "properties": {
                                "id": { "type": "string" },
                                "name": { "type": "string" },
                                "start_bar": { "type": "integer", "minimum": 1 },
                                "end_bar": { "type": "integer", "minimum": 2 },
                                "start_sec": { "type": "number" },
                                "end_sec": { "type": "number" },
                                "color": { "type": "string" }
                            }
                        }
                    },
                    "stem_overrides": {
                        "type": "object",
                        "description": "Map of stem key name -> partial updates (default_gain, muted, pitched, is_cues)."
                    }
                }
            }
        }),
        json!({
            "name": "list_pad_presets",
            "description": "List all Tonic Pad Player factory (14 built-in) and user presets, along with supported voice archetypes, registers, and tonalities.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "save_user_pad_preset",
            "description": "Create or update a multi-layer custom Tonic Pad Player user preset in pad-presets.json (1-4 layers, brightness, shimmer, pan, crossfade_sec).",
            "inputSchema": {
                "type": "object",
                "required": ["name", "layers"],
                "properties": {
                    "id": { "type": "string", "description": "Optional existing user preset ID to update; omit to auto-generate." },
                    "name": { "type": "string" },
                    "category": { "type": "string", "default": "Custom" },
                    "layers": { "type": "array", "minItems": 1, "maxItems": MAX_PAD_LAYERS, "items": pad_layer_schema() },
                    "brightness": { "type": "number", "minimum": 0.0, "maximum": 1.0, "default": 0.55 },
                    "shimmer": { "type": "number", "minimum": 0.0, "maximum": 1.0, "default": 0.25 },
                    "pan": { "type": "number", "minimum": -1.0, "maximum": 1.0, "default": 0.0 },
                    "crossfade_sec": { "type": "number", "minimum": 0.5, "maximum": 12.0 }
                }
            }
        }),
        json!({
            "name": "delete_user_pad_preset",
            "description": "Delete a custom user Tonic Pad Player preset by ID (built-in factory presets cannot be deleted).",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": { "id": { "type": "string" } }
            }
        }),
        json!({
            "name": "list_percussion_styles",
            "description": "List the 12 built-in Percussion Track styles with meter, steps_per_bar, bars_in_cycle, swing, and available instruments/modes/intensities.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "include_hits": { "type": "boolean", "default": false, "description": "If true, include full step hit definitions for each style." }
                }
            }
        }),
        json!({
            "name": "list_percussion_kits",
            "description": "List installed and generated WAV Percussion Sample Kits (default ~/Music/PsalmStage/PercussionKits).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kits_root": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "generate_percussion_kit",
            "description": "Generate a 36-sample (9 instruments x 4 variants) 16-bit 44.1 kHz studio WAV percussion kit on disk.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "profile": { "type": "string", "enum": ["StudioPraise", "MakossaLive", "AcousticCajon"], "default": "StudioPraise" },
                    "kits_root": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "list_percussion_loops",
            "description": "List saved custom dynamic percussion loops (default ~/Music/PsalmStage/PercussionLoops).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "loops_root": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "save_percussion_loop",
            "description": "Save a custom dynamic percussion loop (1-4 bars) directly from pattern fields or by cloning a built-in style preset.",
            "inputSchema": {
                "type": "object",
                "required": ["name"],
                "properties": {
                    "id": { "type": "string" },
                    "name": { "type": "string" },
                    "clone_style": { "type": "string", "enum": PERCUSSION_STYLES, "description": "If provided without hits, clones this built-in style into an editable custom loop." },
                    "bars_in_cycle": { "type": "integer", "minimum": 1, "maximum": 4, "default": 2 },
                    "steps_per_bar": { "type": "integer", "enum": [12, 16, 24] },
                    "swing": { "type": "number", "minimum": 0.0, "maximum": 0.35 },
                    "is_compound": { "type": "boolean" },
                    "hits": { "type": "array", "items": custom_percussion_pattern_schema()["properties"]["hits"]["items"].clone() },
                    "loops_root": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "delete_percussion_loop",
            "description": "Delete a saved custom dynamic percussion loop by ID.",
            "inputSchema": {
                "type": "object",
                "required": ["id"],
                "properties": {
                    "id": { "type": "string" },
                    "loops_root": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "export_percussion_loop_bundle",
            "description": "Render and export a 9-stem toggleable WAV song bundle (kick.wav..bell.wav + manifest.json with pitched: false) into the Song Library ready for setlists.",
            "inputSchema": {
                "type": "object",
                "required": ["title", "bpm"],
                "properties": {
                    "title": { "type": "string" },
                    "bpm": { "type": "number", "minimum": 30.0, "maximum": 260.0 },
                    "time_signature": { "type": "array", "items": { "type": "integer" }, "minItems": 2, "maxItems": 2 },
                    "bars": { "type": "integer", "minimum": 1, "maximum": 128, "default": 16 },
                    "dynamic_arrangement": { "type": "boolean", "default": true },
                    "intensity": { "type": "string", "enum": GROOVE_INTENSITIES, "default": "High" },
                    "room_mix": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                    "style": { "type": "string", "enum": PERCUSSION_STYLES, "default": "Makossa" },
                    "custom_pattern_id": { "type": "string", "description": "Optional saved custom loop ID to export instead of a built-in style." },
                    "custom_pattern": custom_percussion_pattern_schema(),
                    "kit_dir": { "type": "string" },
                    "output_dir": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "generate_section_cues",
            "description": "Generate an IEM-only section cue track (section-cues.wav) from a song's arrangement section marks (Intro, Verse 1, Chorus, Outro, etc.) and register it with is_cues=true.",
            "inputSchema": {
                "type": "object",
                "required": ["song_dir"],
                "properties": {
                    "song_dir": { "type": "string", "description": "Path to the song bundle folder containing manifest.json." },
                    "lead_in_beats": { "type": "number", "minimum": 0.0, "maximum": 8.0, "default": 2.0 },
                    "include_chime": { "type": "boolean", "default": true }
                }
            }
        }),
        json!({
            "name": "get_stage_session",
            "description": "Inspect persisted live stage session state (audio device, routing mode, active setlist/slot, Tonic Pad preset/layers/key, Percussion Track state, and mixer levels).",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        }),
        json!({
            "name": "get_ai_workflow_guide",
            "description": "Return the canonical Psalm Stage Sunday setlist, Tonic Pad, and Percussion Studio workflow instructions (SKILL.md).",
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
                custom_model: args
                    .get("custom_model")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
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
                let separation_engine = j
                    .get("separation_engine")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or(wb_cfg.default_separation_engine);
                let stem_preset = j
                    .get("stem_preset")
                    .and_then(|v| serde_json::from_value(v.clone()).ok())
                    .unwrap_or(wb_cfg.default_stem_preset);
                jobs.push(ImportJobRequest {
                    source,
                    song_title: j
                        .get("song_title")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    artist: j.get("artist").and_then(|v| v.as_str()).map(str::to_string),
                    target_dir: None,
                    separation_engine,
                    stem_preset,
                    custom_model: j
                        .get("custom_model")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    force_reextract: j
                        .get("force_reextract")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
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
            let sections = args
                .get("sections")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| format!("Invalid 'sections' array: {e}"))?
                .unwrap_or_default();
            let mixer = args
                .get("mixer")
                .map(|v| serde_json::from_value(v.clone()))
                .transpose()
                .map_err(|e| format!("Invalid 'mixer' object: {e}"))?;

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
                    sections,
                    songs,
                    mixer,
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
                    custom_model: args
                        .get("custom_model")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
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
                    id: args.get("id").and_then(|v| v.as_str()).map(str::to_string),
                    output_path: args
                        .get("output_path")
                        .and_then(|v| v.as_str())
                        .map(str::to_string),
                    sections,
                    items,
                    mixer,
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
        "save_managed_setlist" => {
            let setlist_val = args
                .get("setlist")
                .ok_or_else(|| "Missing 'setlist'".to_string())?;
            let setlist: SetlistManifest = serde_json::from_value(setlist_val.clone())
                .map_err(|e| format!("Invalid 'setlist' manifest: {e}"))?;
            let explicit = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from);
            let saved_path = save_managed_setlist(
                &setlist,
                explicit.as_deref(),
                &lib_cfg,
                Some(manager.config_dir()),
            )?;
            let manifest = load_setlist_manifest(Path::new(&saved_path)).map_err(|e| e.to_string())?;
            Ok(json!({
                "saved": true,
                "setlist_path": saved_path,
                "setlist": manifest
            }))
        }
        "list_setlists" => {
            let list = list_managed_setlists(&lib_cfg);
            Ok(json!({ "count": list.len(), "setlists": list }))
        }
        "get_setlist" => {
            let resolved_path = if let Some(path) = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
            {
                path.to_string()
            } else {
                let id_q = args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let name_q = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let managed = list_managed_setlists(&lib_cfg);
                let found = managed.into_iter().find(|s| {
                    id_q.is_some_and(|id| s.id.eq_ignore_ascii_case(id))
                        || name_q.is_some_and(|name| s.name.eq_ignore_ascii_case(name))
                });
                found
                    .map(|s| s.path)
                    .ok_or_else(|| "Provide 'path' or matching managed setlist 'id'/'name'".to_string())?
            };
            let manifest =
                load_setlist_manifest(Path::new(&resolved_path)).map_err(|e| e.to_string())?;
            Ok(json!({
                "path": resolved_path,
                "setlist": manifest
            }))
        }
        "get_song_manifest" => {
            let dir = args
                .get("dir")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'dir'".to_string())?;
            let inspected = inspect_song_folder(Path::new(dir)).map_err(|e| e.to_string())?;
            serde_json::to_value(inspected).map_err(|e| e.to_string())
        }
        "update_song_manifest" => {
            let dir = args
                .get("dir")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'dir'".to_string())?;
            let dir_path = Path::new(dir);
            let mut manifest =
                SongManifest::from_file(&dir_path.join("manifest.json")).map_err(|e| e.to_string())?;
            if let Some(title) = args.get("title").and_then(|v| v.as_str()) {
                manifest.title = title.trim().to_string();
            }
            if let Some(artist) = args.get("artist").and_then(|v| v.as_str()) {
                manifest.artist = artist.trim().to_string();
            }
            if let Some(bpm) = args.get("bpm").and_then(|v| v.as_f64()) {
                manifest.bpm = bpm;
            }
            if let Some(ts_val) = args.get("time_signature") {
                manifest.time_signature = serde_json::from_value(ts_val.clone())
                    .map_err(|e| format!("Invalid 'time_signature': {e}"))?;
            }
            if let Some(downbeat) = args.get("first_downbeat_sec").and_then(|v| v.as_f64()) {
                manifest.first_downbeat_sec = downbeat.max(0.0);
            }
            if let Some(key) = args.get("original_key").and_then(|v| v.as_str()) {
                manifest.original_key = key.trim().to_string();
            }
            if let Some(cm_val) = args.get("click_mode") {
                let cm: ClickMode = serde_json::from_value(cm_val.clone())
                    .map_err(|e| format!("Invalid 'click_mode': {e}"))?;
                manifest.click_mode = cm;
            }
            if args
                .get("clear_needs_review")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                if let Some(ref mut prov) = manifest.provenance {
                    prov.needs_review = false;
                }
            }
            if let Some(sections_val) = args.get("sections") {
                let sections: Vec<SectionDef> = serde_json::from_value(sections_val.clone())
                    .map_err(|e| format!("Invalid 'sections': {e}"))?;
                manifest.sections = sections;
            }
            if let Some(stems_obj) = args.get("stem_overrides").and_then(|v| v.as_object()) {
                for (stem_key, patch_val) in stems_obj {
                    let Some(existing_stem) = manifest.stems.get(stem_key).cloned() else {
                        return Err(format!("Stem '{stem_key}' not found in manifest"));
                    };
                    let mut file = existing_stem.file_name().to_string();
                    let mut pitched = existing_stem.is_pitched(stem_key);
                    let mut is_cues = Some(existing_stem.is_cues(stem_key));
                    let mut default_gain = match existing_stem {
                        StemDef::Detailed { default_gain, .. } => default_gain,
                        StemDef::File(_) => 1.0,
                    };
                    let mut muted = existing_stem.is_muted();

                    if let Some(f) = patch_val.get("file").and_then(|v| v.as_str()) {
                        file = f.to_string();
                    }
                    if let Some(p) = patch_val.get("pitched").and_then(|v| v.as_bool()) {
                        pitched = p;
                    }
                    if let Some(c) = patch_val.get("is_cues").and_then(|v| v.as_bool()) {
                        is_cues = Some(c);
                    }
                    if let Some(g) = patch_val.get("default_gain").and_then(|v| v.as_f64()) {
                        default_gain = (g as f32).clamp(0.0, 1.5);
                    }
                    if let Some(m) = patch_val.get("muted").and_then(|v| v.as_bool()) {
                        muted = m;
                    }
                    manifest.stems.insert(
                        stem_key.clone(),
                        StemDef::Detailed {
                            file,
                            pitched,
                            is_cues,
                            default_gain,
                            muted,
                        },
                    );
                }
            }
            save_song_manifest(dir_path, &manifest).map_err(|e| e.to_string())?;
            Ok(json!({
                "saved": true,
                "dir": dir,
                "manifest": manifest
            }))
        }
        "list_pad_presets" => {
            let presets = list_all_pad_presets(manager.config_dir());
            Ok(json!({
                "count": presets.len(),
                "presets": presets,
                "voice_archetypes": [
                    PadVoiceArchetype::WarmAnalog,
                    PadVoiceArchetype::BrightSynth,
                    PadVoiceArchetype::EvolvingMotion,
                    PadVoiceArchetype::StringsCello,
                    PadVoiceArchetype::OrganPad,
                    PadVoiceArchetype::GuitarSwell,
                    PadVoiceArchetype::ReversePiano,
                    PadVoiceArchetype::AiryChoir
                ],
                "registers": [PadRegister::Low, PadRegister::Mid, PadRegister::High],
                "tonalities": [
                    PadTonality::Auto,
                    PadTonality::OpenFifth,
                    PadTonality::Major,
                    PadTonality::Minor,
                    PadTonality::Melodic
                ]
            }))
        }
        "save_user_pad_preset" => {
            let preset = PadPreset {
                id: args
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                name: args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing 'name'".to_string())?
                    .to_string(),
                category: args
                    .get("category")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Custom")
                    .to_string(),
                is_factory: false,
                layers: serde_json::from_value(
                    args.get("layers")
                        .cloned()
                        .ok_or_else(|| "Missing 'layers'".to_string())?,
                )
                .map_err(|e| format!("Invalid 'layers': {e}"))?,
                brightness: args
                    .get("brightness")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.55) as f32,
                shimmer: args
                    .get("shimmer")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.25) as f32,
                pan: args.get("pan").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                crossfade_sec: args
                    .get("crossfade_sec")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32),
            };
            let presets =
                save_user_pad_preset(manager.config_dir(), preset).map_err(|e| e.to_string())?;
            Ok(json!({ "count": presets.len(), "presets": presets }))
        }
        "delete_user_pad_preset" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'id'".to_string())?;
            let presets =
                delete_user_pad_preset(manager.config_dir(), id).map_err(|e| e.to_string())?;
            Ok(json!({ "count": presets.len(), "presets": presets }))
        }
        "list_percussion_styles" => {
            let include_hits = args
                .get("include_hits")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let styles: Vec<Value> = PercussionStyle::all()
                .iter()
                .map(|style| {
                    let spec = style.pattern();
                    let mut entry = json!({
                        "style": style,
                        "steps_per_bar": spec.steps_per_bar,
                        "bars_in_cycle": spec.bars_in_cycle,
                        "swing": spec.swing,
                        "is_compound": spec.is_compound,
                        "hit_count": spec.hits.len(),
                    });
                    if include_hits {
                        let custom = style.to_custom_pattern("preview", "preview");
                        entry["hits"] = serde_json::to_value(custom.hits).unwrap_or(Value::Null);
                    }
                    entry
                })
                .collect();
            Ok(json!({
                "count": styles.len(),
                "styles": styles,
                "modes": [PercussionMode::Off, PercussionMode::Layer, PercussionMode::PercussionOnly],
                "intensities": [GrooveIntensity::Low, GrooveIntensity::Medium, GrooveIntensity::High],
                "instruments": ALL_PERCUSSION_INSTRUMENTS,
                "variants": HitVariant::ALL
            }))
        }
        "list_percussion_kits" => {
            let root = args
                .get("kits_root")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(default_percussion_kits_root);
            let kits = list_percussion_kits(&root);
            Ok(json!({
                "kits_root": root.to_string_lossy(),
                "count": kits.len(),
                "kits": kits
            }))
        }
        "generate_percussion_kit" => {
            let profile: StudioKitProfile = match args.get("profile") {
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("Invalid 'profile': {e}"))?,
                None => StudioKitProfile::StudioPraise,
            };
            let root = args
                .get("kits_root")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(default_percussion_kits_root);
            let info =
                generate_studio_percussion_kit(&root, profile).map_err(|e| e.to_string())?;
            serde_json::to_value(info).map_err(|e| e.to_string())
        }
        "list_percussion_loops" => {
            let root = args
                .get("loops_root")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(default_percussion_loops_root);
            let loops = list_percussion_loops(&root);
            Ok(json!({
                "loops_root": root.to_string_lossy(),
                "count": loops.len(),
                "loops": loops
            }))
        }
        "save_percussion_loop" => {
            let name = args
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'name'".to_string())?
                .trim()
                .to_string();
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| psalm_workbench::sanitize_setlist_slug(&name));
            let target_bars = args
                .get("bars_in_cycle")
                .and_then(|v| v.as_u64())
                .map(|b| (b as u8).clamp(1, 4));

            let mut pattern = if let Some(hits_val) = args.get("hits") {
                let hits = serde_json::from_value(hits_val.clone())
                    .map_err(|e| format!("Invalid 'hits': {e}"))?;
                CustomPercussionPattern {
                    id,
                    name,
                    steps_per_bar: args
                        .get("steps_per_bar")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(16) as u8,
                    bars_in_cycle: target_bars.unwrap_or(2),
                    swing: args.get("swing").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32,
                    is_compound: args
                        .get("is_compound")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                    hits,
                }
            } else if let Some(clone_val) = args.get("clone_style") {
                let style: PercussionStyle = serde_json::from_value(clone_val.clone())
                    .map_err(|e| format!("Invalid 'clone_style': {e}"))?;
                let mut cloned = style.to_custom_pattern(&id, &name);
                if let Some(bars) = target_bars {
                    cloned = cloned.with_bars(bars);
                }
                if let Some(sw) = args.get("swing").and_then(|v| v.as_f64()) {
                    cloned.swing = sw as f32;
                }
                cloned
            } else {
                return Err(
                    "Provide either 'hits' array or 'clone_style' to create a percussion loop"
                        .to_string(),
                );
            };
            pattern = pattern.clamped_and_sorted();
            let root = args
                .get("loops_root")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(default_percussion_loops_root);
            let loops = save_percussion_loop(&root, pattern).map_err(|e| e.to_string())?;
            Ok(json!({
                "loops_root": root.to_string_lossy(),
                "count": loops.len(),
                "loops": loops
            }))
        }
        "delete_percussion_loop" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'id'".to_string())?;
            let root = args
                .get("loops_root")
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(default_percussion_loops_root);
            let loops = delete_percussion_loop(&root, id).map_err(|e| e.to_string())?;
            Ok(json!({
                "loops_root": root.to_string_lossy(),
                "count": loops.len(),
                "loops": loops
            }))
        }
        "export_percussion_loop_bundle" => {
            let title = args
                .get("title")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required argument 'title'".to_string())?
                .to_string();
            let bpm = args
                .get("bpm")
                .and_then(|v| v.as_f64())
                .ok_or_else(|| "Missing required argument 'bpm'".to_string())?;
            let style: PercussionStyle = match args.get("style") {
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("Invalid 'style': {e}"))?,
                None => PercussionStyle::Makossa,
            };
            let time_signature = match args.get("time_signature") {
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("Invalid 'time_signature': {e}"))?,
                None => {
                    if matches!(style, PercussionStyle::Woro | PercussionStyle::Worship68) {
                        [6, 8]
                    } else {
                        [4, 4]
                    }
                }
            };
            let custom_pattern = if let Some(cp_val) = args.get("custom_pattern") {
                Some(
                    serde_json::from_value::<CustomPercussionPattern>(cp_val.clone())
                        .map_err(|e| format!("Invalid 'custom_pattern': {e}"))?,
                )
            } else if let Some(cp_id) = args.get("custom_pattern_id").and_then(|v| v.as_str()) {
                let loops = list_percussion_loops(&default_percussion_loops_root());
                Some(
                    loops
                        .into_iter()
                        .find(|l| l.id == cp_id)
                        .ok_or_else(|| format!("Custom percussion loop '{cp_id}' not found"))?,
                )
            } else {
                None
            };
            let intensity: GrooveIntensity = match args.get("intensity") {
                Some(v) => serde_json::from_value(v.clone())
                    .map_err(|e| format!("Invalid 'intensity': {e}"))?,
                None => GrooveIntensity::High,
            };
            let req = ExportLoopBundleRequest {
                title,
                output_dir: args
                    .get("output_dir")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                bpm,
                time_signature,
                bars: args.get("bars").and_then(|v| v.as_u64()).unwrap_or(16) as u32,
                dynamic_arrangement: args
                    .get("dynamic_arrangement")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(true),
                intensity,
                room_mix: args
                    .get("room_mix")
                    .and_then(|v| v.as_f64())
                    .map(|v| v as f32)
                    .unwrap_or(DEFAULT_PERCUSSION_ROOM_MIX),
                style,
                custom_pattern,
                kit_dir: args
                    .get("kit_dir")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            };
            let res = export_percussion_loop_bundle(Path::new(&lib_cfg.library_root), req)
                .map_err(|e| e.to_string())?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "generate_section_cues" => {
            let song_dir = args
                .get("song_dir")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing required 'song_dir' argument".to_string())?;
            let lead_in_beats = args
                .get("lead_in_beats")
                .and_then(|v| v.as_f64())
                .unwrap_or(2.0) as f32;
            let include_chime = args
                .get("include_chime")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let res = psalm_workbench::generate_section_cues_for_song(
                Path::new(song_dir),
                None,
                &psalm_workbench::SectionCueOptions {
                    lead_in_beats,
                    include_chime,
                    sample_rate: None,
                    use_os_tts: None,
                },
            )?;
            serde_json::to_value(res).map_err(|e| e.to_string())
        }
        "get_stage_session" => {
            let session_path = manager.config_dir().join(STAGE_SESSION_FILENAME);
            let exists = session_path.exists();
            let session = load_stage_session(&session_path).unwrap_or_else(StageSessionState::default);
            Ok(json!({
                "session_file": session_path.to_string_lossy(),
                "exists": exists,
                "session": session
            }))
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
        for expected in [
            "plan_sunday_setlist",
            "create_or_update_setlist",
            "save_managed_setlist",
            "get_song_manifest",
            "update_song_manifest",
            "list_pad_presets",
            "save_user_pad_preset",
            "delete_user_pad_preset",
            "list_percussion_styles",
            "list_percussion_kits",
            "generate_percussion_kit",
            "list_percussion_loops",
            "save_percussion_loop",
            "delete_percussion_loop",
            "export_percussion_loop_bundle",
            "generate_section_cues",
            "get_stage_session",
        ] {
            assert!(
                tools.iter().any(|t| t["name"] == expected),
                "Missing expected MCP tool '{expected}'"
            );
        }

        assert_eq!(lines[2]["id"], 3);
        assert!(lines[2]["result"]["structuredContent"]["markdown"]
            .as_str()
            .unwrap()
            .contains("Psalm Stage Setlist & Workbench Workflow"));
    }

    #[test]
    fn test_mcp_pad_percussion_manifest_and_setlist_tools() {
        let root = std::env::temp_dir().join(format!("psalm_mcp_latest_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let config_dir = root.join("Config");
        let lib_dir = root.join("Library");
        let setlists_dir = root.join("Setlists");
        let kits_dir = root.join("Kits");
        let loops_dir = root.join("Loops");
        std::fs::create_dir_all(&config_dir).unwrap();
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::create_dir_all(&setlists_dir).unwrap();

        let manager = WorkbenchManager::new(config_dir);
        manager
            .save_library_config(psalm_audio_core::library::LibraryConfig {
                library_root: lib_dir.to_string_lossy().to_string(),
                setlists_root: setlists_dir.to_string_lossy().to_string(),
                linked_folders: vec![],
                recent_setlists: vec![],
            })
            .unwrap();

        // 1. Pad presets list / save / delete
        let presets = dispatch_tool_call(&manager, "list_pad_presets", json!({})).unwrap();
        assert!(presets["count"].as_u64().unwrap() >= 14);
        let saved_pad = dispatch_tool_call(
            &manager,
            "save_user_pad_preset",
            json!({
                "id": "sunday-warm",
                "name": "Sunday Warmth",
                "layers": [{
                    "enabled": true,
                    "archetype": "GuitarSwell",
                    "register": "Mid",
                    "tonality": "Auto",
                    "gain": 0.85
                }],
                "brightness": 0.6,
                "shimmer": 0.4
            }),
        )
        .unwrap();
        assert!(saved_pad["presets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "sunday-warm"));

        // 2. Percussion styles + custom loop save/list/delete + kit generate/list + 9-stem bundle export
        let styles = dispatch_tool_call(
            &manager,
            "list_percussion_styles",
            json!({ "include_hits": true }),
        )
        .unwrap();
        assert_eq!(styles["count"], 12);

        let loop_saved = dispatch_tool_call(
            &manager,
            "save_percussion_loop",
            json!({
                "id": "makossa-sunday",
                "name": "Makossa Sunday",
                "clone_style": "Makossa",
                "bars_in_cycle": 4,
                "loops_root": loops_dir.to_string_lossy()
            }),
        )
        .unwrap();
        assert_eq!(loop_saved["loops"][0]["bars_in_cycle"], 4);

        let kit = dispatch_tool_call(
            &manager,
            "generate_percussion_kit",
            json!({
                "profile": "MakossaLive",
                "kits_root": kits_dir.to_string_lossy()
            }),
        )
        .unwrap();
        assert_eq!(kit["sample_count"], 28);

        let exported = dispatch_tool_call(
            &manager,
            "export_percussion_loop_bundle",
            json!({
                "title": "High Praise Makossa",
                "bpm": 128.0,
                "bars": 4,
                "style": "Makossa",
                "kit_dir": kit["dir"]
            }),
        )
        .unwrap();
        let exported_dir = exported["song_dir"].as_str().unwrap().to_string();
        assert_eq!(exported["stem_files"].as_array().unwrap().len(), 9);

        // 3. Inspect & update song manifest
        let inspected =
            dispatch_tool_call(&manager, "get_song_manifest", json!({ "dir": exported_dir }))
                .unwrap();
        assert_eq!(inspected["manifest"]["bpm"], 128.0);
        let updated = dispatch_tool_call(
            &manager,
            "update_song_manifest",
            json!({
                "dir": exported_dir,
                "bpm": 130.0,
                "original_key": "G",
                "click_mode": "IntroOnly"
            }),
        )
        .unwrap();
        assert_eq!(updated["manifest"]["bpm"], 130.0);
        assert_eq!(updated["manifest"]["original_key"], "G");

        let cues_gen = dispatch_tool_call(
            &manager,
            "generate_section_cues",
            json!({
                "song_dir": exported_dir,
                "lead_in_beats": 2.0,
                "include_chime": true
            }),
        )
        .unwrap();
        assert_eq!(cues_gen["stem_key"], "cues");
        assert!(cues_gen["manifest"]["stems"]["cues"].is_object());

        // 4. Create setlist with Service Section, SongBundle + standalone PadOnly slot, save_managed_setlist & delete user preset
        let created = dispatch_tool_call(
            &manager,
            "create_or_update_setlist",
            json!({
                "name": "Sunday Morning",
                "sections": [{
                    "id": "sec-praise",
                    "name": "Praise",
                    "kind": "Praise"
                }],
                "items": [
                    {
                        "song_dir": exported_dir,
                        "section_id": "sec-praise",
                        "transition_mode": "AutoAdvance",
                        "crossfade_sec": 3.0
                    },
                    {
                        "title": "Altar Prayer Pad",
                        "slot_kind": "PadOnly",
                        "target_key": "D",
                        "pad_preset_id": "sunday-warm",
                        "transition_mode": "ContinueUntilStopped"
                    }
                ]
            }),
        )
        .unwrap();
        assert_eq!(created["built"], true);
        assert_eq!(created["setlist"]["sections"].as_array().unwrap().len(), 1);
        assert_eq!(created["setlist"]["items"][1]["slot_kind"], "PadOnly");
        assert_eq!(
            created["setlist"]["items"][1]["pad_lock"]["preset_id"],
            "sunday-warm"
        );

        let archived = dispatch_tool_call(
            &manager,
            "save_managed_setlist",
            json!({ "setlist": created["setlist"] }),
        )
        .unwrap();
        assert!(archived["setlist_path"]
            .as_str()
            .unwrap()
            .ends_with("sunday-morning-2.json"));

        let del_pad = dispatch_tool_call(
            &manager,
            "delete_user_pad_preset",
            json!({ "id": "sunday-warm" }),
        )
        .unwrap();
        assert!(!del_pad["presets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "sunday-warm"));

        let del_loop = dispatch_tool_call(
            &manager,
            "delete_percussion_loop",
            json!({ "id": "makossa-sunday", "loops_root": loops_dir.to_string_lossy() }),
        )
        .unwrap();
        assert_eq!(del_loop["count"], 0);

        let session = dispatch_tool_call(&manager, "get_stage_session", json!({})).unwrap();
        assert!(session["session"].is_object());

        let _ = std::fs::remove_dir_all(&root);
    }
}

