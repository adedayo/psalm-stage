# SPEC-012: Native OS File Picker & In-App Song Bundle Builder

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-012` |
| **Title** | Native OS File Picker & In-App Song Bundle Builder |
| **Module** | `crates/psalm-audio-core/src/loader.rs`, `src-tauri/src/lib.rs`, `src/app/` |
| **Status** | Implemented & Verified |
| **Priority** | P1 (High) |
| **Related Specs** | [SPEC-006: Library & Bundle System](./SPEC-006-LIBRARY-BUNDLE-SYSTEM.md), [SPEC-004: Setlist Flow](./SPEC-004-SETLIST-FLOW.md), [SPEC-008: Stage UI](./SPEC-008-STAGE-UI-SPARTAN.md) |

---

## 1. Overview & Motivation

Requiring worship leaders or stage volunteers to hand-type filesystem paths and manually author `manifest.json` files in a text editor creates friction before Sunday rehearsal.

This specification adds:
1. **Native OS Folder & File Dialogs** via `tauri-plugin-dialog` (`@tauri-apps/plugin-dialog`) for selecting song folders (to load directly or add to the setlist) and saving/opening `.json` setlists, with graceful fallback to manual path inputs in browser preview mode.
2. **In-App Song Bundle Inspector & Manifest Builder**:
   - Scan any selected directory containing audio stems (`.wav`, `.flac`, `.ogg`, `.mp3`), even when `manifest.json` does not yet exist.
   - Auto-classify discovered stems (`drums`, `bass`, `cues`, `keys`, `guitars`, `vocals`, etc.) with sensible `pitched` and `is_cues` defaults.
   - Allow editing song metadata (`title`, `artist`, `bpm`, `time_signature`, `first_downbeat_sec`, `original_key`), stem roles/gains, and arrangement sections (`name`, `start_bar`, `end_bar`, `color`).
   - Validate and write `<song_dir>/manifest.json` atomically to disk, then immediately load or refresh the song in the audio engine.

---

## 2. Native Dialog Integration

### 2.1 Dependencies & Capabilities
- Rust: `tauri-plugin-dialog = "2"` registered in `src-tauri/Cargo.toml` and `src-tauri/src/lib.rs`.
- Frontend: `@tauri-apps/plugin-dialog` in `package.json`.
- Tauri Capabilities (`src-tauri/capabilities/default.json`):
  - `dialog:allow-open`
  - `dialog:allow-save`

### 2.2 Supported Native Actions
- **Browse Song Folder**: Opens OS directory picker; loads folder directly or queues into the setlist.
- **Inspect / Setup Song Folder**: Opens OS directory picker (or edits the active song folder) inside the Song Setup modal/drawer.
- **Open / Save Setlist**: Native `.json` open and save dialogs for portable Sunday service setlist files.

---

## 3. Folder Discovery & Stem Auto-Classification

When scanning a folder without `manifest.json` (or opening an existing folder in the Song Setup editor):
1. Supported extensions (case-insensitive): `wav`, `flac`, `ogg`, `mp3`.
2. Hidden files (starting with `.`) are ignored.
3. Stem naming & classification heuristics:
   - Normalize filename stem (without extension) into a clean identifier (`lowercase`, whitespace/hyphens normalized).
   - **Cues / Click stems**: filenames containing `cue`, `cues`, `guide`, `click`, or `count` default to `pitched: false`, `is_cues: true`.
   - **Drums / Percussion**: filenames containing `drum`, `kick`, `snare`, `perc`, `hat`, `tom`, `loop` default to `pitched: false`, `is_cues: false`.
   - **All melodic / vocal / harmonic stems** (`bass`, `keys`, `piano`, `synth`, `pad`, `guitar`, `gtr`, `eg`, `ag`, `vocal`, `vox`, `bgv`, `strings`, `organ`, `music`) default to `pitched: true`, `is_cues: false`.
4. **Duration Probing & Default Sections**:
   - Probe the longest stem's duration in seconds using `symphonia` metadata (or sample-count fallback) without full decode when inspecting, computing `estimated_bars = ((duration_sec - first_downbeat_sec) / seconds_per_bar).ceil()`.
   - If no `manifest.json` exists yet, seed clean default sections (e.g., `Intro` bars `1..5` and `Song Body` `5..(estimated_bars + 1).max(9)`) so the user can refine bars immediately.

---

## 4. IPC Contracts (`src-tauri/src/lib.rs`)

- `inspect_song_folder(dir: String) -> Result<InspectedSongBundle, String>`
  - Returns `{ dir, has_existing_manifest, estimated_duration_sec, manifest }` (either parsed existing `manifest.json` + any newly detected audio files, or an auto-generated draft `SongManifest`).
- `save_song_manifest(dir: String, manifest: SongManifest, reload_if_active: bool) -> Result<(), String>`
  - Validates the manifest (`bpm` in `[30.0, 260.0]`, non-empty `title`, `time_signature` denominator nonzero, sections have `start_bar >= 1` and `end_bar > start_bar`, at least one stem), writes pretty-printed JSON to `<dir>/manifest.json`, and optionally dispatches `EngineCommand::LoadSongDir(dir)` when active.
- `save_setlist_file(path: String, setlist: SetlistManifest) -> Result<(), String>`
- `load_setlist_file(path: String) -> Result<SetlistManifest, String>`

---

## 5. Verification & Testing Strategy

- **`test_stem_classification_heuristics`**: Verify `cues.wav`, `guide_track.mp3`, `drums.wav`, `percussion.flac`, `bass.wav`, `eg1.wav`, and `keys.ogg` receive exact expected `pitched` and `is_cues` defaults.
- **`test_inspect_song_folder_synthesizes_draft_manifest`**: Create a temp directory with WAV stems and no `manifest.json`; verify `inspect_song_folder` produces a valid draft `SongManifest` with estimated duration, bars, and sorted stems.
- **`test_save_song_manifest_roundtrip_and_validation`**: Validate rejection of invalid sections (`end_bar <= start_bar`) or invalid BPM, and verify valid manifest persistence round-trips cleanly through `load_song_from_dir`.
- **Frontend Spec (`app.spec.ts`)**: Verify Song Setup / Setlist UI actions and validation guards under Vitest.
