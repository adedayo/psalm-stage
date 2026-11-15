# SPEC-017: Live Stage Setlist Drag-and-Drop, Multi-Select Batch Editing & Service Sections

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-017` |
| **Title** | Live Stage Setlist Drag-and-Drop, Multi-Select Batch Editing & Service Sections |
| **Module** | `crates/psalm-audio-core/src/manifest.rs`, `engine.rs`, `crates/psalm-workbench`, `crates/psalm-stage-cli`, `src/app/` |
| **Status** | Implemented |
| **Priority** | P1 (High) |
| **Extends** | [SPEC-004: Multi-Song Setlist Engine](./SPEC-004-SETLIST-FLOW.md), [SPEC-008: High-Contrast Stage UI](./SPEC-008-STAGE-UI-SPARTAN.md), [SPEC-013: Song Library & Setlist Studio](./SPEC-013-SONG-LIBRARY-AND-SETLIST-STUDIO.md) |
| **Related Specs** | [SPEC-015: AI Harness Setlist Automation](./SPEC-015-AI-HARNESS-SETLIST-AUTOMATION.md), [SPEC-016: Tonic Pad Player](./SPEC-016-TONIC-PAD-PLAYER.md), [SPEC-018: Song Crossfade Transitions](./SPEC-018-SONG-CROSSFADE-TRANSITIONS.md), [SPEC-019: Percussion Track & Groove Engine](./SPEC-019-PERCUSSION-TRACK-GROOVE-ENGINE.md) |

---

## 1. Overview & Motivation

During Sunday rehearsals and live stage prep, worship leaders frequently adjust song order, apply uniform musical settings across blocks of songs (e.g., bringing three songs into the same key or disabling click for acoustic moments), and organize the service into recognizable liturgy blocks (**Praise**, **Worship**, **Sermonic**, **Sermon**, **Ministry**, **Offering**, or custom sections).

Currently:
- **Setlist Studio** supports `@angular/cdk/drag-drop` reordering, whereas **Live Stage** only provides `Up` / `Dn` buttons.
- Changing key, tempo, pad state, click mode, or transition behavior requires editing one song at a time.
- `SetlistManifest` is a flat `items: Vec<SetlistItem>` with no service-level sections or native support for non-backing-track slots (e.g., continuous pad/underscore behind the **Sermon** or a standalone percussion groove).

This specification establishes:
1. **Live Stage Drag-and-Drop Setlist Reordering**: Full `@angular/cdk/drag-drop` parity with Setlist Studio, guarded by **Live Lock**.
2. **Multi-Select Songs & Atomic Batch Settings**: Select multiple slots (checkboxes + `Shift+Click` range selection + `Select Section` / `Clear`) and apply common settings in one action.
3. **Service Sections (`SetlistSection`)**: Color-coded, collapsible service sections (`Praise`, `Worship`, `Sermonic`, `Sermon`, `Ministry`, `Offering`, `Custom`) across **Live Stage**, **Setlist Studio**, `.json` setlist manifests, and AI/CLI tooling (`SPEC-015`).
4. **Standalone Pad-Only & Percussion+Pad Slots (`SetlistSlotKind`)**: Allow slots with no backing audio bundle (for example, `Sermon` underscore behind the preacher, `Ministry` prayer pad, or live percussion groove).

---

## 2. Live Stage Drag-and-Drop Reordering

### 2.1 UX & Stage Safety Rules
- The Sunday Setlist queue in `src/app/app.html` adopts `@angular/cdk/drag-drop` (`cdkDropList`, `cdkDrag`, `cdkDragHandle`) using the same `⋮⋮` drag handle pattern as Setlist Studio (`src/app/components/setlist-studio/`).
- **Stage Safety Invariant**:
  - When `liveLock()` is `true`, `[cdkDropListDisabled]="liveLock()"` and `[cdkDragDisabled]="liveLock()"` MUST disable dragging completely, visually dimming/hiding drag handles so accidental stage touches cannot reorder songs mid-service.
  - Accessible `Up` / `Dn` buttons remain available (or via compact action controls) for keyboard/touch accessibility when unlocked.
- Dropping a slot at index `to` dispatches `EngineCommand::MoveSetlistSlot { from, to }`, which preserves `active_setlist_slot` pointing to the currently active song and preserves `section_id` reassignment when dropped into a different section.

---

## 3. Multi-Select & Batch Common Settings

### 3.1 Selection Model (Frontend State)
- Both **Live Stage** and **Setlist Studio** maintain a local `selectedSlots = signal<Set<number>>(new Set())` (resetting safely or remapping when slots are reordered/removed).
- Interaction ergonomics:
  - Per-row selection checkbox (independent of clicking the row to cue/select the active song).
  - `Shift+Click` selects a contiguous slot range `[anchor..slot]`.
  - Quick actions: **Select All**, **Select Section**, **Clear Selection**.

### 3.2 Batch Edit Toolbar
When `selectedSlots().size > 0` and `!liveLock()`, a high-contrast **Batch Apply Bar** appears above/inside the setlist card showing `"N selected"` and controls to apply any subset of common settings:
- **Target Key** (`C`..`B`, `Cm`..`Bm`, or `Reset to Original`): computes per-slot `key_shift_semitones` relative to each song's `original_key` (clamped `[-12, 12]`) or applies an explicit relative semitone offset (`-2`, `-1`, `+1`, `+2`).
- **Tempo (BPM)**: set uniform `bpm_override` (`30.0..=260.0`) or `Reset to Native BPM`.
- **Ambient Pad**: `Pad ON` / `Pad OFF` (`ambient_pad_enabled`), plus optional **Apply Current Pad Preset Lock** or **Clear Pad Lock (Pass-Through)**.
- **Click Mode**: `AlwaysOn` | `IntroOnly` | `Off`.
- **Transition Mode & Crossfade**: `AutoAdvance` | `AutoPause` | `Manual` and `crossfade_sec` (`0.0..=10.0`).
- **Percussion Mode & Style** (per [SPEC-019](./SPEC-019-PERCUSSION-TRACK-GROOVE-ENGINE.md)): `Off` | `Layer` | `PercussionOnly`, Style preset, and Intensity.
- **Assign to Service Section**: move/assign all selected slots to an existing or newly created `SetlistSection`.

### 3.3 Atomic Engine Command (`SetlistBatchPatch`)

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetlistBatchPatch {
    #[serde(default)]
    pub slots: Vec<usize>,
    /// Absolute target key per slot (resolved relative to each slot's original_key)
    /// OR explicit semitone shift override.
    #[serde(default)]
    pub target_key: Option<String>,
    #[serde(default)]
    pub key_shift_semitones: Option<i32>,
    /// Some(Some(bpm)) sets override; Some(None) clears override back to native BPM.
    #[serde(default)]
    pub bpm_override: Option<Option<f64>>,
    #[serde(default)]
    pub click_mode: Option<Option<ClickMode>>,
    #[serde(default)]
    pub transition_mode: Option<TransitionMode>,
    #[serde(default)]
    pub crossfade_sec: Option<f32>,
    #[serde(default)]
    pub ambient_pad_enabled: Option<bool>,
    #[serde(default)]
    pub pad_lock: Option<Option<PadLockConfig>>,
    #[serde(default)]
    pub section_id: Option<Option<String>>,
    #[serde(default)]
    pub percussion_mode: Option<PercussionMode>,
    #[serde(default)]
    pub percussion_style: Option<PercussionStyle>,
    #[serde(default)]
    pub percussion_intensity: Option<GrooveIntensity>,
}
```

- `EngineCommand::BatchUpdateSetlistSlots(SetlistBatchPatch)`:
  - Iterates over valid indices in `patch.slots` in a single command pass.
  - If `patch.target_key` is provided, computes shortest-distance `key_shift_semitones` from each item's `original_key` (via `compute_semitone_shift` helper). If `original_key` is `"Unknown"`, leaves `key_shift_semitones` unchanged for that item.
  - If `active_setlist_slot` is included in `patch.slots`, immediately applies live changes (tempo, key shift, click mode, pad state/lock, percussion state) to the active transport without stopping playback (unless `liveLock` blocks UI edits).

---

## 4. Service Sections (`Praise`, `Worship`, `Sermonic`, `Sermon`, `Ministry`, `Offering`)

### 4.1 Data Model (`crates/psalm-audio-core/src/manifest.rs`)

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ServiceSectionKind {
    #[default]
    Praise,
    Worship,
    Sermonic,
    Sermon,
    Ministry,
    Offering,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetlistSection {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ServiceSectionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default)]
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SetlistSlotKind {
    #[default]
    SongBundle,
    /// Continuous Tonic Pad / underscore slot with no backing stems (e.g., Sermon / Ministry)
    PadOnly,
    /// Built-in Percussion Track + optional Tonic Pad with no backing stems
    PercussionGroove,
}
```

Extensions to existing structs (all `#[serde(default)]` for 100% backward compatibility with existing `setlist.json` files):
- `SetlistManifest`:
  - `#[serde(default)] pub sections: Vec<SetlistSection>`
- `SetlistItem`:
  - `#[serde(default, skip_serializing_if = "Option::is_none")] pub section_id: Option<String>`
  - `#[serde(default)] pub slot_kind: SetlistSlotKind`
  - `#[serde(default, skip_serializing_if = "Option::is_none")] pub time_signature: Option<[u8; 2]>` (used by `PercussionGroove` standalone slots)
- `EngineSnapshot`:
  - `pub setlist_sections: Vec<SetlistSection>`
- `SetlistItemSnapshot`:
  - `pub section_id: Option<String>`
  - `pub slot_kind: SetlistSlotKind`
  - `pub estimated_duration_sec: f64` (0 or continuous indicator for `PadOnly`/`PercussionGroove` slots)

### 4.2 Section UI & Ordering Behavior
- **Built-In Section Presets & Color Coding**:
  - `Praise` (`#f59e0b` Amber)
  - `Worship` (`#06b6d4` Cyan)
  - `Sermonic` (`#a855f7` Purple)
  - `Sermon` (`#6366f1` Indigo — e.g. subtle underscore behind the preacher)
  - `Ministry` (`#10b981` Emerald)
  - `Offering` (`#f43f5e` Rose)
  - `Custom` (`#71717a` Zinc — user-editable label)
- **Collapsible Section Headers**:
  - Displays color accent bar, section name, optional short note (e.g., `"Playing behind preacher"`), item count, total duration (`MM:SS`), **Select All in Section**, **Collapse/Expand** toggle, and **Move Section Up/Down**.
  - When collapsed, if the currently active song belongs to that section, the header shows a glowing `ACTIVE` pill with the current song title so the stage operator never loses orientation.
- **Drag Between Sections**:
  - Users can drag any song row into any section's drop zone (including empty sections such as a newly created `Sermon` or `Offering` section) to move and reassign its `section_id`.
  - Ordering invariant: items sharing a `section_id` stay contiguous within their section's visual group when dragged within or between sections; ungrouped items (`section_id: None`) appear in a clean ungrouped list at the top or bottom until assigned.
- **Deleting a Section**:
  - Removing a `SetlistSection` sets `section_id = None` on its slots rather than deleting the songs.

### 4.3 Standalone `PadOnly` & `PercussionGroove` Slots
- Clicking **+ Add Pad / Groove Slot** (in Live Stage, Setlist Studio, or inside any Service Section like `Sermon` or `Ministry`) creates a `SetlistItem` with `song_dir: None` and `slot_kind: PadOnly` or `PercussionGroove`:
  - `PadOnly` slot: selecting/cueing engages the Tonic Pad in the slot's configured `original_key` (with optional `pad_lock` preset/volume) and leaves audio stems silent—sustaining smoothly behind the preacher (`Sermon`), altar call (`Ministry`), or `Offering` until the operator advances.
  - `PercussionGroove` slot: runs the built-in Percussion Track (`SPEC-019`) at the slot's `base_bpm` and `time_signature` (looping indefinitely until stopped or `NextSong` is triggered) plus optional Tonic Pad and IEM Click.

### 4.4 `EngineCommand` Additions
- `UpsertSetlistSection(SetlistSection)`
- `RemoveSetlistSection(String)` (unassigns items, removes section)
- `MoveSetlistSection { id: String, to_index: usize }`
- `ToggleSetlistSectionCollapsed { id: String, collapsed: bool }`
- `AddStandaloneSetlistSlot { title: String, slot_kind: SetlistSlotKind, key: String, bpm: f64, time_signature: [u8; 2], section_id: Option<String>, insert_at: Option<usize> }`
- `BatchUpdateSetlistSlots(SetlistBatchPatch)`

---

## 5. Verification & Testing Strategy

- **Rust Unit Tests (`psalm-audio-core`)**:
  - `test_setlist_sections_backward_compat_deserialization`: Deserialize legacy `SetlistManifest` JSON without `sections`, `section_id`, or `slot_kind`; verify defaults populate cleanly.
  - `test_batch_update_setlist_slots_key_and_settings`: Apply a `SetlistBatchPatch` with `target_key: Some("G")`, `bpm_override`, `click_mode`, and `ambient_pad_enabled` across heterogeneous songs (`C`, `D`, `F`) and verify per-slot `key_shift_semitones` (`+7`, `+5`, `+2`) and active song live update.
  - `test_standalone_pad_only_and_percussion_slot_activation`: Cue `PadOnly` and `PercussionGroove` slots without `song_dir`; verify `PreloadState::Ready`, Tonic Pad key engagement, and zero panics in `render_block`.
  - `test_remove_section_preserves_slots`: Removing a `SetlistSection` clears `section_id` on member items without removing them from `setlist_items`.
- **Angular Unit Tests (`Vitest`)**:
  - Live Stage drag-drop calls `MoveSetlistSlot` when unlocked and disables `cdkDropList` when `liveLock()` is active.
  - Multi-select checkbox + `Shift+Click` range selection displays the Batch Edit Bar and emits `BatchUpdateSetlistSlots`.
  - Collapsible section headers group slots by `section_id` and compute section durations accurately.
