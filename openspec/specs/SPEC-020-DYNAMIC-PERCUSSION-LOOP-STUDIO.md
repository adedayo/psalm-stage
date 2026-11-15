# SPEC-020: Dynamic Percussion Loop Studio, Sample Kits & Multi-Stem Bundle Export

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-020` |
| **Title** | Dynamic Percussion Loop Studio, Sample Kits & Multi-Stem Bundle Export |
| **Module** | `crates/psalm-audio-core/src/percussion.rs`, `percussion_patterns.rs`, `percussion_kit.rs`, `engine.rs`, `src-tauri/src/lib.rs`, `crates/psalm-stage-cli/src/main.rs`, `src/app/` |
| **Status** | Implemented |
| **Extends** | [SPEC-019: Built-In Multi-Style Percussion Track & Groove Engine](./SPEC-019-PERCUSSION-TRACK-GROOVE-ENGINE.md) |

---

## 1. Overview & Motivation

While `SPEC-019` introduced 12 built-in percussion grooves across 9 toggleable instruments, worship leaders also need:
1. **Studio-grade audio richness** (multi-layer drum synthesis, 5-tap stereo human claps, multi-zill tambourines, modal hand drums/talking drums, stereo room ambience, plus real WAV sample kit playback).
2. **Custom Dynamic Loop Authoring** directly inside Psalm Stage—cloning any preset (e.g. `Makossa`, `Afrobeat`, `Woro`, `Worship44`) or building a 1-bar / 2-bar loop from scratch with per-step `Low` / `Medium` / `High` intensity gating and hit variants (`Standard`, `Accent`, `Soft`, `Special`).
3. **Exportable Toggleable Multi-Stem Song Bundles** (`kick.wav`, `snare.wav`, `hihat.wav`, `hand_drums.wav`, `tambourine.wav`, `shaker.wav`, `claps.wav`, `clave.wav`, `bell.wav` + `manifest.json`) ready to drop straight into the Psalm Stage Song Library or any DAW, with every drum/percussion stem marked `pitched: false` so key shifts never affect drum pitch.

---

## 2. Core Components

### 2.1 Hybrid Sample + Multi-Layer Acoustic DSP (`percussion.rs`)
- **36-Slot Pre-Allocated Sample Bank (`PercussionSampleKitData`)**: 9 instruments × 4 `HitVariant`s loaded off the audio callback via `symphonia` (`load_percussion_kit_from_dir`), falling back cleanly to `Standard` or upgraded DSP voices when a variant slot is absent.
- **Zero-Alloc Stereo Studio Room Reverb (`PercRoomReverb`)**: Pre-allocated stereo early-reflection / comb ambience with per-instrument send weighting (`Kick` low-end clean, `Claps` / `Snare` / `Tambourine` / `HandDrums` rich room bloom) controlled via `room_mix: f32` (`0.0..=1.0`).
- **Live Step Playhead, Independent Loop Auditioning & Setlist Tempo Sync (`engine.rs`)**: `EngineSnapshot::percussion_current_step`, `percussion_audition_playing`, and `percussion_audition_bpm` power real-time playhead tracking and standalone loop playback via `StartPercussionAudition` / `StopPercussionAudition` / `SetPercussionAuditionBpm`, allowing loops to be played and tempo-adjusted continuously even while stage playback is stopped and `percussion_mode` is `Off` (automatically stopping audition when full stage `Play` starts). Setlist song switches (`SelectSetlistSlot`, `NextSong`, `PrevSong`) and live tempo updates (`SetBpm`, `AdjustBpm`, `SetSongTiming`, slot config updates) keep `percussion_audition_bpm` and meter synchronized with the active setlist tempo, and `reset_step_cursor()` preserves reverb/cymbal tails across loop wraps.

### 2.2 Sample Kit Generator & Multi-Stem Bundle Exporter (`percussion_kit.rs`)
- **Studio Sample Kit Generator (`generate_studio_percussion_kit`)**: Generates 16-bit 44.1 kHz stereo WAV one-shot packs in `~/Music/PsalmStage/PercussionKits/` (`StudioPraise`, `MakossaLive`, `AcousticCajon`) and recognizes arbitrary user WAV folders dropped into `~/Music/PsalmStage/PercussionKits/<KitName>/`.
- **Custom Loop Preset Store (`1`–`4` Bars)**: Saves/lists/deletes `CustomPercussionPattern` JSON files in `~/Music/PsalmStage/PercussionLoops/` supporting 1, 2, or 4 bars (`12`/`16`/`24` steps per bar, up to 96 steps across 4 bars) with `CustomPercussionPattern::with_bars()` bar expansion.
- **9-Stem Song Bundle Exporter (`export_percussion_loop_bundle`)**: Renders each active instrument cleanly in isolation into `kick.wav` … `bell.wav` alongside `manifest.json` (`pitched: false` per stem), supporting either seamless loops or dynamic multi-section arrangements (`Intro [Low]` → `Verse [Low]` → `Chorus [Medium]` → `Praise Break [High]`).

### 2.3 CLI & UI Integration
- **CLI (`psalm-stage-cli loop`)**: `loop kit generate`, `loop kit list`, `loop list`, `loop export`.
- **Dedicated Loop Studio Workspace (`WorkspaceMode = 'loops'`, `src/app/components/loop-studio/`)**: Full-screen multi-bar beat workbench accessible from top navigation (`Loop Studio`) or Live Stage (`Full Studio →`). Features an independent `PLAY / STOP LOOP` transport with editable BPM, Tap Tempo, Click toggle, `1` / `2` / `4` Bar cycle expansion, Bar `1..4` & `All Bars` matrix tabs, per-bar variation tools (`Copy Bar 1 → All`, `Copy Prev Bar`, `+ Turnaround Fill`, `Clear Bar`, quick groove layer fills), 9-instrument Mute/Solo/Gain/Audition channels, Paint Gate/Variant brush controls, and 9-stem song bundle export.
- **Live Stage Compact Loop Studio (`src/app/app.html`, `src/app/app.ts`)**: Inline `▶ Play Loop` transport shortcut, 1/2/4-bar quick editor, sound kit picker + generator, room ambience control, and 9-stem bundle export right on stage.
