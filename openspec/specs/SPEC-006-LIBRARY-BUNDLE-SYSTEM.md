# SPEC-006: Song Bundle Packaging, Manifest Schema & Stem Decoder

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-006` |
| **Title** | Song Bundle Packaging, Manifest Schema & Stem Decoder |
| **Module** | `crates/psalm-audio-core/src/manifest.rs`, `loader.rs` |
| **Status** | Implemented & Verified |
| **Priority** | P1 (High) |
| **Related Specs** | [SPEC-001: Audio Engine](./SPEC-001-AUDIO-ENGINE.md), [SPEC-004: Setlist Flow](./SPEC-004-SETLIST-FLOW.md) |

---

## 1. Overview & Format Design

Worship teams need a portable, self-contained, human-readable format for multitrack songs. Proprietary, encrypted song bundles lock churches into closed ecosystems and prevent using custom in-house recorded stems.

Psalm Stage standardizes on an open song directory bundle:
- Standard audio stems encoded in WAV, FLAC, OGG, or MP3.
- A single declarative `manifest.json` file defining song structure, downbeat alignment, and stem pitch classification.

---

## 2. Manifest Schema Specification

A valid song directory contains at minimum:
```text
song-folder/
├── manifest.json
├── drums.wav
├── bass.wav
├── music.wav
└── cues.wav
```

### 2.1 Complete JSON Schema

```json
{
  "$schema": "https://psalm-stage.org/schemas/v1/manifest.json",
  "id": "gratitude-brandon-lake",
  "title": "Gratitude",
  "artist": "Brandon Lake",
  "bpm": 78.0,
  "time_signature": [6, 8],
  "first_downbeat_sec": 0.35,
  "original_key": "B",
  "sections": [
    { "id": "intro", "name": "Intro", "start_bar": 1, "end_bar": 5, "color": "#3b82f6" },
    { "id": "verse1", "name": "Verse 1", "start_bar": 5, "end_bar": 13, "color": "#10b981" },
    { "id": "chorus1", "name": "Chorus 1", "start_bar": 13, "end_bar": 21, "color": "#f59e0b" },
    { "id": "bridge1", "name": "Bridge 1", "start_bar": 21, "end_bar": 29, "color": "#ec4899" },
    { "id": "outro", "name": "Outro", "start_bar": 29, "end_bar": 33, "color": "#6366f1" }
  ],
  "stems": {
    "drums": { "file": "drums.wav", "pitched": false, "default_gain": 0.95 },
    "bass": { "file": "bass.wav", "pitched": true, "default_gain": 0.90 },
    "keys": { "file": "keys.wav", "pitched": true, "default_gain": 0.85 },
    "guitars": { "file": "guitars.wav", "pitched": true, "default_gain": 0.85 },
    "cues": { "file": "cues.wav", "pitched": false, "default_gain": 0.80 }
  }
}
```

---

## 3. The `pitched` Stem Invariant

> [!IMPORTANT] Harmonic Transposition Invariant
> When a song's key is changed (e.g. transposing $+3$ semitones for a vocal register):
> - **Pitched Stems (`pitched: true`)**: Synthesizers, pianos, electric guitars, bass, acoustic guitars, and backing vocals pass through the pitch-shifting phase vocoder.
> - **Unpitched Stems (`pitched: false`)**: Drums, percussion, clicks, and verbal guide cues ("Verse", "Chorus") **must never be pitch shifted**. Pitching a drum track distorts snare snap and cymbals; pitching vocal guide cues causes unnatural chipmunk or demon artifacts.

---

## 4. Multi-Format Audio Decoding & Sample Rate Unification

Live audio hardware can run at $44.1\text{ kHz}$, $48\text{ kHz}$, or $96\text{ kHz}$. Stems supplied by different producers may also vary in native sample rates.

`crates/psalm-audio-core/src/loader.rs` uses `symphonia` for fast, zero-copy decoding of:
- PCM 16/24/32-bit WAV
- Lossless FLAC
- Ogg Vorbis
- MP3 (MPEG-1 Audio Layer III)

During initial load, all decoded channels are converted to 32-bit floating point (`f32`) and linearly resampled to match the target audio interface rate:
$$x_{\text{out}}[n] = (1 - \alpha) \cdot x_{\text{in}}[\lfloor s \rfloor] + \alpha \cdot x_{\text{in}}[\lfloor s \rfloor + 1]$$
where $s = n \cdot \frac{f_{\text{in}}}{f_{\text{out}}}$ and $\alpha = s - \lfloor s \rfloor$.

---

## 5. Verification & Testing

Tests in `crates/psalm-audio-core/src/loader.rs`:
- **`test_resample_stereo_linear`**: Verifies exact sample count and amplitude preservation when upsampling ($44.1\text{ kHz} \to 48\text{ kHz}$) and downsampling ($48\text{ kHz} \to 44.1\text{ kHz}$).
- **`test_generate_demo_song`**: Confirms that when no library is mounted, the engine dynamically generates a complete 32-bar synthetic song bundle in memory for instant zero-config testing.

