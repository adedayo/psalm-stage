# Changelog

All notable changes to **Psalm Stage** are documented in this file.
Releases and version bumps are automated using [Release Please](https://github.com/googleapis/release-please) and [Conventional Commits](https://www.conventionalcommits.org/).

## [0.1.0](https://github.com/adedayo/psalm-stage/releases/tag/v0.1.0) (2026-11-15)

### Features
* **Zero-Latency Rust Audio Engine (`psalm-audio-core`)**: Sample-accurate multitrack playback, Signalsmith Stretch real-time tempo/key transposition, and latency-compensated synthesized metronome click (`ClickSynth`).
* **Universal Stage Audio & MIDI Control**: 2-channel Split L/R and multi-bus output matrix, Omni USB/DIN MIDI auto-connect with hotplug detection, one-click MIDI Learn, and Live Lock mode.
* **Live Stage, Setlist Studio & Tonic Pad Player**: Quantized section jumping, seamless infinite looping, harmonic crossfade transitions, multi-layer ambient drone pads, and screen keep-awake during live sets.
* **Dynamic Percussion & Loop Studio**: Built-in groove engine (Worship, Afrobeat, Makossa, Woro, Funk), 9-instrument mixer, custom pattern authoring, and multi-stem loop bundle export.
* **Import Workbench & AI Automation (`psalm-stage-cli`)**: Song library scanner, YouTube & local audio ingest, optional UVR/Demucs stem separation, waveform beat/section annotation, and headless stdio MCP server for AI setlist planning.
* **Cross-Platform Desktop Packaging**: Tauri v2 installers for macOS (Apple Silicon & Intel `.dmg`/`.pkg`), Windows (`.exe`/`.msi`), and Linux (`.AppImage`/`.deb`), plus in-app release detection.
