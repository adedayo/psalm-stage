# SPEC-014: Optional Audio Import, Local Stem Separation & Arrangement Workbench

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-014` |
| **Title** | Optional Audio Import, Local Stem Separation & Arrangement Workbench |
| **Module** | `crates/psalm-audio-core/src/analysis.rs`, `src-tauri/src/workbench.rs`, `src/app/components/import-workbench/` |
| **Status** | Implemented & Verified |
| **Priority** | P1 (High) |
| **Related Specs** | [SPEC-006: Library & Bundle System](./SPEC-006-LIBRARY-BUNDLE-SYSTEM.md), [SPEC-012: Native Picker & Song Setup](./SPEC-012-NATIVE-PICKER-AND-SONG-SETUP.md), [SPEC-013: Song Library & Setlist Studio](./SPEC-013-SONG-LIBRARY-AND-SETLIST-STUDIO.md) |

---

## 1. Architectural Principle: Optional & Zero Live-Stage Coupling

Psalm Stage's **live playback core must remain lightweight, deterministic, and dependency-free**. Musicians running Psalm Stage on a Sunday stage laptop must never be required to install Python, PyTorch, FFmpeg, or gigabytes of ML weights just to play multitracks.

Therefore, the **Import & Stem Workbench** is strictly **optional**:
- **Direct Import (Zero External Dependencies)**: Any local `.wav`, `.mp3`, `.flac`, or `.ogg` track or existing stem folder can be imported, analyzed in pure Rust (`symphonia`), aligned on the waveform timeline, and saved to the Library immediately.
- **Optional Local Toolchain Orchestrator**: When external tools (`yt-dlp`, `ffmpeg`, `audio-separator` / UVR CLI, `demucs`) are installed locally—or configured via custom paths/virtualenv in Workbench Settings—the user unlocks:
  - **YouTube / URL Audio Ingestion** via `yt-dlp` + `ffmpeg`.
  - **Local AI Stem Separation** via **UVR (`audio-separator`)** (MDX-Net / Mel-Band Roformer / Demucs models) or **Demucs** (`htdemucs`, `htdemucs_6s`).
- If optional tools are missing, the UI presents clear status badges, copyable installation commands, and custom executable path pickers without impairing live stage playback or direct local file imports.

---

## 2. Toolchain Detection & Configuration (`~/.psalm-stage/workbench-config.json`)

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkbenchConfig {
    pub yt_dlp_path: Option<String>,
    pub ffmpeg_path: Option<String>,
    pub audio_separator_path: Option<String>,
    pub demucs_path: Option<String>,
    pub default_separation_engine: SeparationEngine,
    pub default_stem_preset: StemPreset,
}
```

Detection searches:
1. User-configured explicit executable path (if non-empty).
2. System `PATH` plus common macOS/Linux/Windows user tool paths (`/opt/homebrew/bin`, `/usr/local/bin`, `~/.local/bin`, `~/.cargo/bin`).

Each detected tool returns `{ available: bool, resolved_path: Option<String>, version: Option<String>, install_hint: String }`.

---

## 3. Ingest & Separation Job Pipeline

### 3.1 Supported Engines & Presets
- `SeparationEngine`:
  - `None` (Direct stereo backing track — no ML separation, copies/normalizes into `music.<ext>`).
  - `AudioSeparator` (UVR CLI `audio-separator` supporting MDX-Net, Roformer, and Demucs checkpoints).
  - `Demucs` (`demucs` CLI).
- `StemPreset`:
  - `TwoStems` (`vocals` + `instrumental`)
  - `FourStems` (`drums`, `bass`, `vocals`, `other`)
  - `SixStems` (`drums`, `bass`, `vocals`, `guitar`, `piano`, `other`)
  - `CustomModel(String)` (arbitrary UVR model filename passed to `audio-separator -m`)

### 3.2 Async Background Execution & Telemetry
- Long-running ingest/separation jobs execute on a dedicated background thread (`std::process::Command` with piped stdout/stderr), never on the audio or UI thread.
- Progress events (`workbench:job-progress`) stream `{ job_id, stage, progress_pct, message, log_line, status: Running | Completed | Failed | Cancelled, output_dir }`.
- Users can cancel an active job cleanly.
- Separated stem filenames from UVR/Demucs are normalized into clean Psalm Stage stem roles (`drums`, `bass`, `vocals`, `guitar`, `piano`, `music`, etc.) with automatic `pitched` / `is_cues` defaults via `classify_stem_filename`.

---

## 4. Rust Audio Analysis & Waveform Peak Extraction (`psalm-audio-core::analysis`)

To power an ultra-responsive waveform and beat-grid workbench with zero JavaScript decoding stalls:
- **`analyze_audio_file(path, num_buckets)`** (pure Rust via `symphonia`):
  - Computes true `duration_sec`, `sample_rate`, `channels`.
  - Downsamples into `num_buckets` normalized min/max amplitude pairs (`peaks: Vec<[f32; 2]>`, values in `[-1.0, 1.0]`).
  - Estimates initial **BPM** using energy-flux onset envelope + autocorrelation across `[60.0, 180.0]` BPM, plus an estimated `first_downbeat_sec`.
- **`analyze_song_folder_mixdown(dir, manifest_opt, num_buckets)`**:
  - Builds a composite waveform peak envelope across a song folder's stems (prioritizing `drums`/`music`/`other`, excluding `cues` if melodic stems exist) so both single-track and multitrack bundles open cleanly in the Workbench.

---

## 5. Interactive Workbench UX

1. **Source Ingest Panel**:
   - **Drop / Pick Local Audio File** (`.wav`, `.mp3`, `.flac`, `.ogg`, `.m4a`).
   - **YouTube URL Ingest** (fetches metadata, previews title/duration, downloads high-quality audio when `yt-dlp` is available).
   - **Open Existing Library Song / Folder** to edit its waveform grid & section markers.
   - **Optional Stem Separation Selector**: Choose `No Separation (Fast Direct Track)`, `UVR (audio-separator)`, or `Demucs`, plus `2-Stem` / `4-Stem` / `6-Stem` / `Custom UVR Model`, with real-time progress log and cancel button.
2. **Interactive Waveform & Beat-Grid Canvas**:
   - High-contrast waveform rendering with playhead, click-to-seek auditioning (plays directly through Psalm Stage's deterministic audio engine with optional live **Click Preview**!), and zoom/pan controls.
   - **Bar Grid Overlay**: Draws crisp bar lines (`1, 2, 3, ...`) computed live from `bpm`, `time_signature`, and `first_downbeat_sec`.
   - **Tap Tempo** + **Auto-Detect BPM** + **Set Downbeat to Playhead** + **`±10ms` / `±1ms` Downbeat Nudge** buttons.
3. **Rapid Section Marker Painting**:
   - Color-coded section timeline bar directly above the waveform.
   - Quick Worship Section Presets (`Intro`, `Verse`, `Pre-Chorus`, `Chorus`, `Bridge`, `Instrumental`, `Tag`, `Outro`) that append or split at the current playhead bar.
   - Click any section block to edit its name, start/end bar, and color token.
4. **Stem Roles & Library Save**:
   - Toggle `Transpose Key` (`pitched`) and `IEM Cues Bus` (`is_cues`) per stem, adjust default gain, and click **Save to Library** (and optionally **Save & Add to Setlist**).

---

## 6. Verification & Testing Strategy

- **Rust Unit Tests**:
  - `test_analyze_audio_peaks_and_bpm_estimation`: Generate a synthetic 120 BPM percussive WAV click fixture, verify `analyze_audio_file` returns exact duration, bounded `[-1.0, 1.0]` peaks, and BPM estimate within tolerance of `120.0` (or harmonic octave).
  - `test_normalize_separated_stem_role`: Verify UVR (`Song_(Drums)_htdemucs.wav`, `Song_(Instrumental)_UVR.wav`) and Demucs (`drums.wav`, `other.wav`, `vocals.wav`) output filenames map cleanly to Psalm Stage stem keys and roles.
  - `test_detect_workbench_tools_never_panics`: Verify tool detection returns structured `ToolchainStatus` cleanly even when optional CLIs are absent.
- **Angular Unit Tests (`Vitest`)**:
  - Workbench mode renders source ingest, toolchain status drawer, waveform bar-grid math, tap-tempo calculation, section splitting/presets, and saving to library.

