# SPEC-013: Local Song Library & Rapid Setlist Creation Studio

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-013` |
| **Title** | Local Song Library & Rapid Setlist Creation Studio |
| **Module** | `crates/psalm-audio-core`, `src-tauri/src/library.rs`, `src/app/components/setlist-studio/` |
| **Status** | Implemented & Verified |
| **Priority** | P1 (High) |
| **Related Specs** | [SPEC-004: Multi-Song Setlist Flow](./SPEC-004-SETLIST-FLOW.md), [SPEC-006: Library & Bundle System](./SPEC-006-LIBRARY-BUNDLE-SYSTEM.md), [SPEC-008: Stage UI](./SPEC-008-STAGE-UI-SPARTAN.md), [SPEC-014: Audio Import & Stem Workbench](./SPEC-014-AUDIO-IMPORT-AND-STEM-WORKBENCH.md) |

---

## 1. Overview & Motivation

While **Live Stage** mode optimizes for zero-distraction Sunday performance, worship leaders and music directors need an equally fast, ergonomic **preparation environment** to browse their local multitrack catalog, test key/tempo transitions, build cohesive service flows, and organize songs before stepping on stage.

This specification introduces:
1. **Top-Bar Workspace Mode Switcher (`Live Stage` | `Setlist Studio` | `Import Workbench`)**:
   - Seamless switching between live performance, rapid setlist authoring, and audio import/stem preparation.
   - **Stage Safety Guard**: Engaging **Live Lock** immediately forces and locks the workspace to `Live Stage`, disabling navigation into `Setlist Studio` or `Import Workbench`.
2. **Local Song Library Indexer**:
   - Configurable **Primary Library Root** (default `~/Music/PsalmStage/Library`) plus any number of **Linked Folders** (persisted in `~/.psalm-stage/library-config.json`).
   - Fast recursive folder discovery returning structured `LibrarySongEntry` summaries:
     - `Ready` bundles (`manifest.json` valid and at least one stem present).
     - `NeedsSetup` folders (audio stems detected without `manifest.json`).
     - `MissingStems` bundles (`manifest.json` references missing audio files).
   - Metadata includes `song_id`, `title`, `artist`, `original_key`, `bpm`, `time_signature`, `estimated_duration_sec`, `sections` summary, `stems` summary, and folder path.
3. **Rapid Setlist Creation Studio**:
   - **Two-Pane Studio Ergonomics**:
     - **Left Pane — Library Browser**: Instant search (title, artist, key, folder), status filter (`All` | `Ready` | `Needs Setup`), key filter, quick sort (`Title`, `BPM`, `Key`), one-click `+ Add` to setlist, drag-to-add, and `Open in Workbench`.
     - **Right Pane — Setlist Flow Builder**:
       - Drag-and-drop slot reordering (`@angular/cdk/drag-drop`) + accessible Up/Down controls.
       - Inline per-slot rapid overrides: **Target Key** (semitone shift `-12..+12` displayed as musical key), **BPM Override**, **Transition Mode** (`AutoPause` | `AutoAdvance` | `Manual`), **Crossfade Seconds**, **Ambient Pad** toggle, **Duplicate Slot** (e.g., for reprises), and **Remove**.
       - **Harmonic Transition Bridges**: Visual connector badges between adjacent songs showing key movement (e.g., `G → D (+7 / 5th)` or `Same Key`) and pad bridge status.
       - **Setlist Runtime Telemetry**: Total song count and total estimated service duration (`MM:SS`), plus **New / Open / Save Setlist** and **"Go Live →"** CTA.

---

## 2. Data Models & Persistence

### 2.1 Library Configuration (`~/.psalm-stage/library-config.json`)

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryConfig {
    pub library_root: String,
    #[serde(default)]
    pub linked_folders: Vec<String>,
    #[serde(default)]
    pub recent_setlists: Vec<String>,
}
```

### 2.2 Library Entry Summary

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryEntryStatus {
    Ready,
    NeedsSetup,
    MissingStems,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySongEntry {
    pub dir: String,
    pub status: LibraryEntryStatus,
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub original_key: String,
    pub bpm: f64,
    pub time_signature: [u8; 2],
    pub estimated_duration_sec: f64,
    pub section_names: Vec<String>,
    pub stem_names: Vec<String>,
    pub has_cues: bool,
    pub missing_files: Vec<String>,
}
```

---

## 3. Tauri IPC Commands

- `get_library_config() -> Result<LibraryConfig, String>`
- `save_library_config(config: LibraryConfig) -> Result<LibraryConfig, String>`
- `scan_song_library() -> Result<Vec<LibrarySongEntry>, String>`
- `ensure_library_root() -> Result<String, String>`

---

## 4. Real-Time Engine Enhancements

To support studio auditioning and accurate runtime feedback without violating audio-thread invariants:
- `EngineCommand::SeekSec(f64)`: Repositions `source_frame_pos`, updates `active_section_idx`, clears `last_beat_index` and `click_delay`, and primes stretchers when seeking.
- `EngineSnapshot` includes `current_position_sec: f64` and `total_duration_sec: f64` calculated purely from preloaded frame counts and `sample_rate`.

---

## 5. Verification & Testing Strategy

- **Rust Unit Tests**:
  - `test_scan_library_discovers_ready_needs_setup_and_missing_stems`: Verify scanning temporary directories accurately classifies valid bundles, raw stem directories, and broken manifests.
  - `test_library_config_roundtrip_and_dedup`: Verify config persistence and deduplication of linked folders.
  - `test_engine_seek_sec_updates_position_and_section`: Verify `SeekSec` moves playback cursor and active section deterministically without allocations.
- **Angular Unit Tests (`Vitest`)**:
  - Workspace mode switcher locks to `stage` when `liveLock()` is active.
  - Studio search/filtering, adding library songs to setlist, duplicating slots, computing total duration & harmonic key intervals, and updating slot key/BPM overrides.

