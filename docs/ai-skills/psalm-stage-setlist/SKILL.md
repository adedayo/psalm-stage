---
name: psalm-stage-setlist
description: Plan, ingest, and assemble Sunday worship setlists in Psalm Stage using the local Song Library, YouTube Workbench pipeline, Tonic Pad presets, Percussion Loops, and deterministic key/transition rules.
---

# Psalm Stage Setlist & Workbench Workflow

Use the `psalm-stage` MCP tools to inspect the local library, check installed audio/AI tooling, plan and build Sunday setlists with service sections and standalone Pad/Percussion slots, manage song manifests (keys, tempos, transitions, sections, stem mixes), configure Tonic Pad and Percussion tracks, and save managed setlists.

## 1. Discovery & Session Context
- `get_toolchain_status`: Verify `yt-dlp`, `ffmpeg`, `demucs`, and `uv` availability before YouTube downloads or stem separation.
- `get_stage_session`: Inspect current live stage state (loaded setlist/song, transport, master/backing/click gains, crossfade settings, output routing, active Tonic Pad & Percussion state).
- `get_ai_workflow_guide`: Returns this guide bundled directly in the MCP server.

## 2. Song Library & Manifests
- `list_library_songs`: List songs in the Song Library with detected/effective BPM, keys, time signatures, durations, stems, and sections.
- `match_library_song`: Match a title/artist query against existing Song Library entries.
- `get_song_manifest` / `update_song_manifest`: Read or edit a song's `song.json` / `manifest.json` (title, artist, BPM, key, time signature, transition style/crossfade, `sections`, stem gains/mute/solo/cue flags, pad lock, percussion lock).
- `generate_section_cues`: Synthesize an IEM-only section cue track (`section-cues.wav`, `is_cues: true`) from a song's arrangement section marks (`Intro`, `Verse 1`, `Chorus`, `Outro`, etc.) with configurable beat lead-in and pre-cue chime.

## 3. YouTube Search & Ingestion
- `search_youtube_tracks`: Search YouTube for worship tracks/stems.
- `ingest_song` / `ingest_songs_batch`: Download, analyze (BPM, key, sections), optionally separate stems (`demucs`), and publish into the Song Library. Supports setting metadata, sections, pad locks, and percussion locks.

## 4. Tonic Pad Presets & Dynamic Percussion Studio
- `list_pad_presets`, `save_user_pad_preset`, `delete_user_pad_preset`: Browse factory and user Tonic Pad presets (`WarmAnalog`, `ShimmerCathedral`, `FeltPianoOrgan`, `SubDrone`, `BrightWorship`), or save/delete custom user presets.
- `list_percussion_styles`, `list_percussion_kits`, `generate_percussion_kit`: Inspect worship percussion styles (`AfroWorship`, `ModernGospel`, `WarmAcoustic`, `AnthemPop`, `LoFiDevotional`) and custom acoustic/synthetic kits.
- `list_percussion_loops`, `save_percussion_loop`, `delete_percussion_loop`, `export_percussion_loop_bundle`: Manage dynamic multi-stem custom percussion loops and export 9-stem WAV loop bundles.

## 5. Planning & Saving Sunday Setlists
- `plan_sunday_setlist`: Plan a Sunday setlist matching library songs (plus YouTube candidates for missing songs) with deterministic key/transition rules, `sections` (Service Sections like Call to Worship, Praise, Worship, Altar Call), standalone `PadOnly` / `PercussionGroove` slots, per-slot `pad_preset_id` / `pad_lock` / `percussion_lock`, and `mixer` levels.
- `create_or_update_setlist`: Build or update a `.json` setlist manifest supporting song slots, standalone `PadOnly` / `PercussionGroove` slots, `sections`, `stem_mix`, crossfades, `ContinueUntilStopped`, and `mixer` (`master_gain`, `backing_gain`, `click_gain`, pad/percussion gains).
- `list_setlists` / `get_setlist`: Inspect saved setlist `.json` manifests.
- `save_managed_setlist`: Save a setlist manifest to an explicit path or generate a collision-safe slug file in the Managed Setlists directory (`setlists_root`).
