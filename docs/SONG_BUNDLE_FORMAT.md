# Song Bundle Format & Manifest Specification

Psalm Stage utilizes a transparent, open, non-proprietary song bundle directory format. This ensures worship teams own their tracks, stems, and arrangements without vendor lock-in or subscription licensing restrictions.

---

## 1. Directory Structure

A song bundle is simply a folder on your drive containing standard audio files and a single `manifest.json`:

```text
My Worship Song (Key G, 72 BPM)/
├── manifest.json
├── drums.wav
├── bass.wav
├── music.wav
├── guitars.wav
├── keys.wav
├── bgvs.wav
└── cues.wav
```

### Supported Audio Codecs:
- **WAV** (Linear PCM 16-bit, 24-bit, 32-bit float; 44.1 kHz, 48 kHz, 88.2 kHz, 96 kHz)
- **FLAC** (Lossless compressed; saves ~50% disk space while retaining bit-perfect audio quality)
- **OGG Vorbis** (Lossy compressed; great for lightweight portable setups)
- **MP3** (MPEG Layer 3; supported for quick rehearsal or demo imports)

---

## 2. Manifest Schema (`manifest.json`)

The manifest defines all musical metadata, timekeeping, section markers, and stem definitions.

```json
{
  "$schema": "https://psalm-stage.org/schemas/v1/manifest.json",
  "id": "way-maker-leeland",
  "title": "Way Maker",
  "artist": "Leeland",
  "bpm": 68.0,
  "time_signature": [4, 4],
  "first_downbeat_sec": 0.42,
  "original_key": "E",
  "sections": [
    {
      "id": "intro",
      "name": "Intro",
      "start_bar": 1,
      "end_bar": 5,
      "color": "#3b82f6"
    },
    {
      "id": "verse1",
      "name": "Verse 1",
      "start_bar": 5,
      "end_bar": 13,
      "color": "#10b981"
    },
    {
      "id": "chorus1",
      "name": "Chorus 1",
      "start_bar": 13,
      "end_bar": 21,
      "color": "#f59e0b"
    },
    {
      "id": "bridge1",
      "name": "Bridge 1",
      "start_bar": 21,
      "end_bar": 29,
      "color": "#ec4899"
    },
    {
      "id": "outro",
      "name": "Outro",
      "start_bar": 29,
      "end_bar": 33,
      "color": "#6366f1"
    }
  ],
  "stems": {
    "drums": {
      "file": "drums.wav",
      "pitched": false,
      "default_gain": 0.95
    },
    "bass": {
      "file": "bass.wav",
      "pitched": true,
      "default_gain": 0.90
    },
    "music": {
      "file": "music.wav",
      "pitched": true,
      "default_gain": 0.85
    },
    "cues": {
      "file": "cues.wav",
      "pitched": false,
      "default_gain": 0.80
    }
  }
}
```

---

## 3. Field Definitions & Rules

### Top-Level Properties:
| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `id` | `string` | **Yes** | Unique identifier (slug-cased, e.g. `"living-hope"`) |
| `title` | `string` | **Yes** | Song display name shown on stage |
| `artist` | `string` | No | Artist / band / composer attribution |
| `bpm` | `number` | **Yes** | Master recorded tempo (must be between $20.0$ and $300.0$) |
| `time_signature`| `[u8, u8]` | No | Meter numerator and denominator (defaults to `[4, 4]`) |
| `first_downbeat_sec` | `number` | No | Timestamp in seconds to Bar 1, Beat 1 (defaults to `0.0`) |
| `original_key` | `string` | No | Musical root key (e.g. `"C"`, `"F#"`, `"Bb"`, defaults to `"C"`) |

### Section Object (`sections[]`):
| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `id` | `string` | **Yes** | Slug identifier (`"intro"`, `"verse1"`, `"chorus"`) |
| `name` | `string` | **Yes** | Label rendered on the large stage navigation tile |
| `start_bar` | `u32` | **Yes** | 1-based start bar (inclusive) |
| `end_bar` | `u32` | **Yes** | 1-based end bar (exclusive). A 4-bar section starting at 1 ends at 5. |
| `color` | `string` | No | Hex color code or Tailwind color for the tile border |

### Stem Object (`stems{}`):
| Field | Type | Required | Description |
| :--- | :--- | :--- | :--- |
| `file` | `string` | **Yes** | Relative path to audio file inside the song folder |
| `pitched` | `boolean`| No | Set to `false` for drums, percussion, clicks, and verbal cues so they are NOT pitch-shifted when transposing key! (Defaults to `true`) |
| `default_gain`| `number` | No | Initial linear gain ($0.0\text{ to }1.0$, defaults to $1.0$) |

---

## 4. Section Bar Math Validation Invariant

For every section $S_i$:
$$1 \le S_i.\text{start\_bar} < S_i.\text{end\_bar}$$
$$S_i.\text{end\_bar} \le S_{i+1}.\text{start\_bar}$$

Sections must be strictly non-overlapping and contiguous for standard linear arrangements, though gaps are permitted if an un-stemmed free-flow section exists.
