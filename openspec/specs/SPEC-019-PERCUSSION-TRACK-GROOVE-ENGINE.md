# SPEC-019: Built-In Multi-Style Percussion Track & Groove Engine

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-019` |
| **Title** | Built-In Multi-Style Percussion Track & Groove Engine |
| **Module** | `crates/psalm-audio-core/src/percussion.rs`, `percussion_patterns.rs`, `engine.rs`, `manifest.rs`, `src/app/` |
| **Status** | Implemented |
| **Priority** | P1 (High) |
| **Extends** | [SPEC-001: Real-Time DSP Audio Engine](./SPEC-001-AUDIO-ENGINE.md), [SPEC-002: Click & Routing](./SPEC-002-CLICK-AND-ROUTING.md) |
| **Related Specs** | [SPEC-016: Tonic Pad Player](./SPEC-016-TONIC-PAD-PLAYER.md), [SPEC-017: Live Stage Setlist UX & Sections](./SPEC-017-LIVE-STAGE-SETLIST-UX-AND-SECTIONS.md), [SPEC-018: Song Crossfade Transitions](./SPEC-018-SONG-CROSSFADE-TRANSITIONS.md) |

---

## 1. Overview & Motivation

In many worship moments—such as acoustic sets, spontaneous praise, African/Afrobeat medleys, or songs where a band is playing live without wanting full melodic backing tracks—a worship leader wants **rhythmic percussion only** (tambourine, shakers, claps, drums, clave/rhythm sticks, etc.) locked to the song's tempo and time signature, OR wants to **layer** extra percussion energy on top of an existing track.

Following the pure-synthesis, zero-external-sample philosophy of [SPEC-016](./SPEC-016-TONIC-PAD-PLAYER.md), this specification adds a **real-time, beat-locked Percussion Track & Groove Engine** directly inside `psalm-audio-core`:
1. **9 Independently Mixable & Muteable Instruments**: `Kick`, `Snare`, `HiHat`, `HandDrums` (Conga / Djembe / Talking Drum / Amapiano Log Drum), `Tambourine`, `Shaker`, `Claps`, `Clave` (wooden rhythm sticks / woodblock), and `Bell` (Cowbell / African Agogo).
2. **12 Authentic Style Presets Across `4/4`, `3/4`, `6/8`, and `12/8`**:
   - **African & Diaspora**: `Afrobeat`, `HighlifePraise`, `Woro` (`6/8` & `12/8`), `Makossa`, `Amapiano`
   - **Contemporary Worship & Acoustic**: `Worship44`, `Worship68`, `AcousticCajon`
   - **Funk, Gospel & Global**: `FunkPocket`, `GospelPraise`, `LatinCaribbean`, `ReggaeSoca`
3. **3-Mode Song Integration (`PercussionMode`)**:
   - **`Off`**: Percussion Track inactive.
   - **`Layer`**: Percussion Track plays tightly synced alongside the song's backing stems.
   - **`PercussionOnly`**: Automatically mutes the song's backing music stems (`music_l`/`music_r` stem audio) while keeping the song's BPM, time signature, section timeline, IEM Click/Cues, and optional Tonic Pad active—driving the song purely with the Percussion Track.
4. **Standalone `PercussionGroove` Slots**: Create setlist slots with no audio files required (ideal for live band grooves, praise breaks, or `Offering` / `Ministry`).
5. **3 Live Groove Intensity Levels (`Low`, `Medium`, `High`) + Optional Section Auto-Follow**: Adapt energy dynamically across Verse, Chorus, and Bridge/Praise Break.

---

## 2. Audio Engine & Synthesis Architecture

### 2.1 Real-Time Invariants
- **Zero Heap Allocations**: All synthesis voices, noise generators, bandpass/resonator filters, pattern step tables, and latency delay lines MUST be pre-allocated inside `PercussionEngine::new` / `set_sample_rate`.
- **Zero-Flam Latency Compensation**:
  - When `PercussionMode::Layer` is active and backing stems are time-stretched or pitch-shifted (`!is_stretch_bypassed()`), the Percussion Engine stereo output passes through a pre-allocated `PercussionDelayBuffer` delayed by `stretched_output_latency_frames(rate)` (matching `ClickDelayBuffer` in [SPEC-002](./SPEC-002-CLICK-AND-ROUTING.md)) so percussion transients align frame-for-frame with stretched backing stems.
  - When `PercussionMode::PercussionOnly` or playing a standalone `PercussionGroove` slot (backing stems muted/absent), stretcher latency delay is bypassed (`0` samples) for instant live response.
- **Bus Routing**:
  - Primary stereo percussion renders into `music_l` / `music_r` (FOH bus, scaled by `master_gain`).
  - Optional `iem_send: f32` (`0.0..=1.0`, default `0.0`) lets worship teams feed a controlled amount of the percussion groove into `iem_l` / `iem_r` for drummers/vocalists.

### 2.2 Sub-Beat Step Sequencer & Master Clock Sync
- Pattern resolution:
  - Duple meters (`4/4`, `2/4`, `3/4`): **16th-note grid** (`4` subdivisions per quarter-note beat; `16` steps/bar in `4/4`, `12` steps/bar in `3/4`).
  - Compound meters (`6/8`, `12/8`, e.g., `Woro`, `Worship68`): **16th-note / triplet-feel compound grid** (`3` eighth-note subdivisions or `6` 16th-note subdivisions per dotted-quarter beat; `12` or `24` steps per bar).
- Timing lock:
  - Driven directly by the engine's continuous beat clock (`beats` timestamps if present, or `first_downbeat_sec` + `current_bpm`).
  - Supports per-style **Micro-Swing** (`0.0..=0.35`) and subtle **Humanization** (`±1.5 ms` deterministic PRNG micro-timing & velocity variation) so shakers, tambourines, and claps feel organic rather than robotic.

### 2.3 Instrument Synthesis Models (Pure DSP, Zero Samples)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PercussionInstrument {
    Kick,
    Snare,
    HiHat,
    HandDrums,   // Conga / Djembe / Talking Drum / Amapiano Log Drum
    Tambourine,
    Shaker,
    Claps,
    Clave,       // Wooden Clave / Woodblock ("rhythm sticks")
    Bell,        // Cowbell / African Agogo (high/low pitch accent)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PercussionChannelConfig {
    pub instrument: PercussionInstrument,
    pub enabled: bool,
    /// Channel mix level: 0.0..=1.5
    pub gain: f32,
    /// Stereo pan: -1.0..=1.0 (default instrument-appropriate spread)
    pub pan: f32,
}
```

Synthesizer voice design per instrument (polyphonic/choked fixed voice pool, zero alloc):
1. **`Kick`**: Pitch-swept sine/triangle fundamental (`140 Hz -> 48 Hz` fast exponential pitch envelope) + short click transient.
2. **`Snare`**: Dual resonant body modes (`185 Hz` + `330 Hz`) + high-passed snappy noise burst (`1.8 kHz`), shifting to warm box-wood resonance in `AcousticCajon`.
3. **`HiHat`**: 6-oscillator inharmonic metallic square cluster + high-pass filter (`> 6.5 kHz`), with distinct closed (`45 ms`) and open (`220 ms`) decay shapes.
4. **`HandDrums`**: Velocity-sensitive dual-tone membrane synthesis supporting open tone, slap, bass, **Talking Drum** upward/downward pitch glide (`Woro`/`HighlifePraise`), and deep woody FM-like **Log Drum** thump (`Amapiano`).
5. **`Tambourine`**: Metallic bell-cluster FM partials (`5.4 kHz`–`9.2 kHz`) + shaped burst envelope distinguishing crisp accent strikes from continuous jingle shakes.
6. **`Shaker`**: Band-pass filtered white/pink noise (`3.5 kHz`–`9.0 kHz`, `Q = 1.4`) with dual-envelope forward/back stroke shaping (`16th` accent vs ghost).
7. **`Claps`**: 4-tap staggered micro-burst (`0 ms`, `11 ms`, `21 ms`, `33 ms`) of band-passed noise (`1.1 kHz`–`2.6 kHz`) + diffuse tail simulating an ensemble of hands clapping together.
8. **`Clave`**: Dual-partial woody resonator (`2.45 kHz` + `3.7 kHz`, tight `35 ms` decay) modeling authentic hardwood **claves / rhythm sticks**.
9. **`Bell`**: Inharmonic two-partial metallic **agogo / cowbell** (`560 Hz` & `845 Hz` for cowbell; `880 Hz` & `1320 Hz` high/low pair for African agogo in `Woro` / `Afrobeat` / `Makossa`).

---

## 3. Styles, Intensity & Data Contracts

### 3.1 Style & Mode Definitions

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PercussionMode {
    #[default]
    Off,
    Layer,
    PercussionOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PercussionStyle {
    #[default]
    Worship44,
    Worship68,
    Afrobeat,
    HighlifePraise,
    Woro,
    Makossa,
    Amapiano,
    FunkPocket,
    GospelPraise,
    AcousticCajon,
    LatinCaribbean,
    ReggaeSoca,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GrooveIntensity {
    Low,
    #[default]
    Medium,
    High,
}
```

### 3.2 Style Characteristic Matrix

| Style | Default Meter | Signature Instruments & Feel |
| :--- | :--- | :--- |
| **`Afrobeat`** | `4/4` | Interlocking 16th shaker, 3-2/Afrobeat bell & clave, syncopated kick, conga/hand drums |
| **`HighlifePraise`** | `4/4` | Bright agogo bell pattern, lively tambourine, claps on 2 & 4, buoyant kick/snare |
| **`Woro`** | `6/8` / `12/8` | Compound rolling 12/8 Nigerian praise: high-low agogo bell, heavy driving shaker, talking drum pitch glides, claps |
| **`Makossa`** | `4/4` | Driving four-on-the-floor kick pulse, crisp clave/bell syncopation, bright shaker & snare pocket |
| **`Amapiano`** | `4/4` | Signature bouncy swung 16th shaker, syncopated woody **Log Drum** hits, rim/snare & crisp claps |
| **`Worship44`** | `4/4` | Modern stadium pulse: warm kick, rim/snare, 8th/16th shaker, chorus tambourine swell |
| **`Worship68`** | `6/8` | Compound worship ballad: deep kick/floor pulse, brushed snare, swelling tambourine & shaker |
| **`AcousticCajon`** | `4/4` / `6/8` | Strip-down wooden cajon bass/slap, light shaker, tambourine, claps (zero electronic hi-hat) |
| **`FunkPocket`** | `4/4` | Tight 16th-note syncopated kick & ghost-note snare, crisp hi-hat, claps, cowbell |
| **`GospelPraise`** | `4/4` | High-energy Sunday praise/shout: driving kick/snare, full tambourine & claps, bell accents |
| **`LatinCaribbean`** | `4/4` | 3-2 son clave, conga tumbao, steady shaker, cowbell |
| **`ReggaeSoca`** | `4/4` | One-drop / soca offbeat skank feel with rim/clave, shaker, kick/snare, bell |

### 3.3 Section Auto-Follow Heuristic
When `percussion_follow_sections == true`, `MixerState` updates effective `GrooveIntensity` on section transitions:
- `Intro`, `Verse`, `Prayer`, `Sermon`, `Tag`, `Outro` → `GrooveIntensity::Low` (subtle shaker, clave, soft kick/cajon)
- `Pre-Chorus`, `Chorus`, `Turnaround` → `GrooveIntensity::Medium` (adds tambourine, claps, fuller groove)
- `Bridge`, `Vamp`, `Praise`, `Instrumental`, `Shout` → `GrooveIntensity::High` (full ensemble, bell/agogo, driving dynamics)
- Manual live clicks on `Low` / `Medium` / `High` override immediately (and optionally pause auto-follow until the next section change or user re-enables `Auto`).

### 3.4 Setlist Persistence & Engine Contracts
- `SetlistItem` & `PercussionLockConfig`:
  - `#[serde(default)] pub percussion_mode: PercussionMode`
  - `#[serde(default)] pub percussion_style: Option<PercussionStyle>`
  - `#[serde(default)] pub percussion_intensity: Option<GrooveIntensity>`
  - `#[serde(default, skip_serializing_if = "Option::is_none")] pub percussion_channels: Option<Vec<PercussionChannelConfig>>`
  - `#[serde(default, skip_serializing_if = "Option::is_none")] pub percussion_gain: Option<f32>`
- `EngineCommand` additions:
  - `SetPercussionMode(PercussionMode)`
  - `SetPercussionStyle(PercussionStyle)`
  - `SetPercussionIntensity(GrooveIntensity)`
  - `SetPercussionFollowSections(bool)`
  - `SetPercussionGain(f32)`
  - `SetPercussionIemSend(f32)`
  - `SetPercussionChannel { instrument: PercussionInstrument, enabled: Option<bool>, gain: Option<f32>, pan: Option<f32> }`
  - `ResetPercussionMix`
- `EngineSnapshot` additions:
  - `percussion_mode: PercussionMode`
  - `percussion_style: PercussionStyle`
  - `percussion_intensity: GrooveIntensity`
  - `percussion_follow_sections: bool`
  - `percussion_gain: f32`
  - `percussion_iem_send: f32`
  - `percussion_channels: Vec<PercussionChannelConfig>`
- `MidiTarget` additions (`SPEC-005`):
  - `TogglePercussionMode` (cycles `Off` / active mode), `PercussionGain` (CC), `PercussionIntensityUp`, `PercussionIntensityDown`.

---

## 4. UI Integration (Live Stage & Setlist Studio)

- **Live Stage — Percussion Rack Card** (collapsible / compact alongside Tonic Pad Player):
  - Mode pill switcher: **`Off` | `Layer` | `Percussion Only`** (with clear badge warning when `Percussion Only` is muting backing music stems).
  - **Style Selector** grouped by category (`African`, `Worship`, `Funk & Gospel`, `Acoustic & Global`) with time-signature indicator (`4/4`, `6/8 / 12/8`).
  - **Intensity Buttons**: **`Low` | `Med` | `High`** + **`Auto Section`** toggle.
  - **9-Channel Mini Strip Mixer**: per-instrument mute button + level slider for `Kick`, `Snare`, `Hi-Hat`, `Hand Drums / Log`, `Tambourine`, `Shaker`, `Claps`, `Clave`, `Bell` + `Master Groove` & `IEM Send` sliders.
- **Setlist Slot & Batch Bar**:
  - Every song slot displays a compact `Perc Only` or `Perc+` badge when enabled, and can be configured per-slot or across multiple selected slots via `SPEC-017`.

---

## 5. Verification & Testing Strategy

- **Rust Unit Tests (`psalm-audio-core`)**:
  - `test_percussion_all_styles_produce_bounded_audio`: Render 4 bars at `110 BPM` across `44.1 kHz`, `48 kHz`, and `96 kHz` for all 12 `PercussionStyle`s and all 3 `GrooveIntensity` levels; assert zero NaNs/Infs, non-silent RMS energy, and peak amplitude `< 1.0`.
  - `test_percussion_only_mutes_backing_stems_keeps_click_and_pad`: Enable `PercussionMode::PercussionOnly` on a loaded song with active stems, click, and pad; verify backing stems do not appear in `music_l/r`, while percussion + pad render to `music_l/r` and click renders to `iem_l/r`.
  - `test_percussion_layer_latency_compensation_matches_stretcher`: Run `PercussionMode::Layer` with `bpm_override` (stretcher active) and verify percussion downbeat onset aligns within `1` sample of the stretched stem/click downbeat (`5760` samples latency).
  - `test_percussion_channel_mute_and_solo_isolation`: Mute all channels except `Clave`; verify output matches clave-only synthesis and muting all channels produces silence.
  - `test_percussion_woro_and_worship68_compound_meter`: Verify `6/8` and `12/8` bar lengths and accent patterns loop seamlessly across bar boundaries.
- **Angular Unit Tests (`Vitest`)**:
  - Switching `PercussionMode` (`Off` / `Layer` / `PercussionOnly`), style preset, intensity, and muting individual instruments (`Tambourine`, `Shaker`, `Claps`, `Clave`, etc.) dispatches the expected `EngineCommand` payloads.
