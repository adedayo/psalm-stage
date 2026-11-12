# SPEC-010: Local-First Offline Caching & Non-Destructive Overrides

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-010` |
| **Title** | Local-First Offline Caching & Non-Destructive Overrides |
| **Module** | `src-tauri/src/`, `src/app/services/` |
| **Status** | Planned / Architecture Defined |
| **Priority** | P3 (Low) |
| **Related Specs** | [SPEC-004: Setlist Flow](./SPEC-004-SETLIST-FLOW.md), [SPEC-006: Library Bundle System](./SPEC-006-LIBRARY-BUNDLE-SYSTEM.md) |

---

## 1. Overview & Stage Requirements

Church environments frequently face unreliable Wi-Fi or zero internet connectivity in auditorium wings, green rooms, or portable campus setups.

The library architecture must guarantee:
1. **100% Offline Autonomy**: Once a song bundle is downloaded or imported locally, the stage laptop requires zero internet connectivity to load, stretch, loop, and playback.
2. **Local-First Conflict Resolution**: Song adjustments made on a stage laptop (fader levels, key shifts, MIDI assignments) are stored in local override sidecars rather than overwriting shared cloud templates.
3. **Non-Destructive Versioning**: Multiple worship leaders can create distinct arrangement profiles (e.g. "Youth Service Arrangement" vs "Sunday Morning Arrangement") pointing to the same immutable audio stem files.

---

## 2. Directory Layout & Sidecar Architecture

```text
~/.psalm-stage/
├── config.json                     # Audio device, buffer size, global defaults
├── midi-profile.json               # Personal hardware MIDI controller bindings
├── setlists/                       # Local service setlists
│   └── 2026-11-15-sunday.json
└── overrides/                      # Non-destructive song adjustments
    └── gratitude-brandon-lake.json
```

### 2.1 Sidecar Override Schema

```json
{
  "song_id": "gratitude-brandon-lake",
  "worship_leader": "Sarah",
  "saved_key_shift": 2,
  "saved_bpm": 80.0,
  "stem_mix": {
    "drums": { "gain": 0.85, "muted": false },
    "bass": { "gain": 0.90, "muted": false },
    "keys": { "gain": 1.00, "muted": false },
    "guitars": { "gain": 0.70, "muted": false },
    "cues": { "gain": 0.85, "muted": false }
  },
  "custom_arrangement": [
    "intro", "verse1", "chorus1", "verse1", "chorus1", "bridge1", "chorus1", "outro"
  ]
}
```

---

## 3. Verification & Testing Strategy

- **`test_sidecar_override_application`**: Confirm that loading a song with an active sidecar applies fader gains and key shifts while leaving the base `manifest.json` intact.
- **`test_offline_manifest_integrity`**: Validate checksum hashes on loaded stems to detect corrupted downloads or incomplete file copies before live playback starts.
