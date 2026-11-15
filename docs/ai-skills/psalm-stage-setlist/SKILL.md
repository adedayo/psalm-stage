---
name: psalm-stage-setlist
description: Plan, ingest, and assemble Sunday worship setlists in Psalm Stage using the local Song Library, YouTube Workbench pipeline, and deterministic key/transition rules.
---

# Psalm Stage Sunday Setlist Workflow

Use this workflow whenever a user wants to create or update a worship setlist in **Psalm Stage** from a list of songs (with or without YouTube links, keys, tempos, or medley/flow notes).

## Tools Available

Prefer the **Psalm Stage MCP tools** (`psalm-stage` MCP server) when connected, or run `psalm-stage-cli ... --json` as a fallback.

## Mandatory Workflow: Plan → Confirm → Build → Review

1. **Check Environment First (Once per session)**
   - Call `get_toolchain_status` (or `psalm-stage-cli status --json`) to inspect `library_root`, `setlists_root`, and whether `yt_dlp`, `ffmpeg`, `audio_separator`, and `demucs` are available.
   - If `yt_dlp` is missing and songs need YouTube ingest, share the returned `install_hint` rather than failing silently.

2. **Always Dry-Run with `plan_sunday_setlist` First**
   - Translate the user's request into structured song items:
     - `title`, optional `artist`, optional `youtube_url`
     - `target_key` (e.g., `"G"`, `"Eb"`, `"F#m"`) if specified
     - `bpm_override` if specified
     - `transition_mode`: set `"AutoAdvance"` when the user says *"flow into"*, *"medley"*, *"no pause"*, or *"straight into"* on the preceding song; otherwise default to `"AutoPause"`.
     - `ambient_pad_enabled`: default `true` (especially for transitions and pads).
   - Call `plan_sunday_setlist` (or `psalm-stage-cli setlist plan --json`).

3. **Confirm YouTube Search Matches Before Downloading/Separating**
   - For any song **without** a YouTube URL that is missing from the Library, `plan_sunday_setlist` returns `needs_confirmation` with ranked YouTube search candidates (`title`, `channel`, `duration_sec`, `webpage_url`).
   - **Always present those candidates to the user and ask them to confirm** which URL to use before calling `ingest_songs_batch` or `build` (unless the user explicitly asked you to auto-pick / auto-approve).
   - If a library match is ambiguous (`score` between `0.60` and `0.85`), confirm whether to reuse that library song or import a fresh version.

4. **Execute Batch Ingest & Create the Setlist**
   - Once all missing songs have confirmed URLs (or `auto_approve: true`), run `create_or_update_setlist` / `psalm-stage-cli setlist build --json` (or `ingest_songs_batch` followed by `create_or_update_setlist`).
   - Already-imported songs are automatically reused from Library provenance cache (`find_cached_library_bundle`) unless `force_reextract: true` is requested.
   - If the configured default stem separator (`AudioSeparator` or `Demucs`) is not installed, the pipeline automatically falls back to a stereo backing track (`None`) and includes a warning.

5. **Report Clearly & Highlight Workbench QA Items**
   - Summarize the saved setlist name, file path in `setlists_root`, slot order, effective keys, semitone shifts, BPMs, and transition modes.
   - Explicitly list any newly ingested songs (`needs_review: true`) or songs with `Original key: Unknown` / low `key_confidence`, and remind the user to open them in **Import Workbench** (`Review Needed` badge in Setlist Studio) to verify Bar 1 downbeat, key, and section markers before Sunday rehearsal.

