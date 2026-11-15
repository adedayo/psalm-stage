use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::pad::{PadLayerConfig, MAX_PAD_LAYERS};
use crate::percussion::PercussionLockConfig;
use crate::{AudioCoreError, Result};

/// Source and stem separation metadata persisted in `manifest.json` so already-extracted
/// bundles in the Library are reused instead of re-downloaded or re-extracted unless forced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportProvenance {
    pub source_kind: String,
    pub source_ref: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_source: Option<String>,
    pub separation_engine: String,
    pub stem_preset: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_model: Option<String>,
    #[serde(default)]
    pub needs_review: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_notes: Vec<String>,
}

/// Normalizes a YouTube URL (extracting its canonical 11-char video ID when present)
/// or local audio file path into a stable cache key for Library deduplication.
pub fn canonicalize_source_ref(kind: &str, raw: &str) -> String {
    let trimmed = raw.trim();
    if kind.eq_ignore_ascii_case("youtube") {
        if let Some(id) = extract_youtube_video_id(trimmed) {
            return format!("youtube:{id}");
        }
        return format!(
            "youtube:{}",
            trimmed.trim_end_matches('/').to_ascii_lowercase()
        );
    }
    let normalized = Path::new(trimmed)
        .canonicalize()
        .unwrap_or_else(|_| Path::new(trimmed).to_path_buf())
        .to_string_lossy()
        .trim()
        .to_string();
    format!("file:{normalized}")
}

fn extract_youtube_video_id(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    if let Some(idx) = lower.find("v=") {
        let rest = &url[idx + 2..];
        let id: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if !id.is_empty() {
            return Some(id);
        }
    }
    for marker in ["youtu.be/", "/shorts/", "/embed/", "/live/"] {
        if let Some(idx) = lower.find(marker) {
            let rest = &url[idx + marker.len()..];
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

/// Controls when the built-in metronome click plays during a song.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ClickMode {
    /// Metronome click plays throughout the entire song.
    #[default]
    AlwaysOn,
    /// Metronome click plays during the Intro/count-in and switches off automatically when the Intro ends.
    IntroOnly,
    /// Metronome click starts muted for this song.
    Off,
}

/// Describes a song bundle stored in a folder alongside its stem audio files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SongManifest {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    pub bpm: f64,
    #[serde(default = "default_time_signature")]
    pub time_signature: [u8; 2],
    #[serde(default)]
    pub first_downbeat_sec: f64,
    #[serde(default = "default_key")]
    pub original_key: String,
    #[serde(default)]
    pub click_mode: ClickMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ImportProvenance>,
    pub sections: Vec<SectionDef>,
    pub stems: HashMap<String, StemDef>,
}

fn default_time_signature() -> [u8; 2] {
    [4, 4]
}

fn default_key() -> String {
    "Unknown".to_string()
}

/// A named worship song section defined on the timeline.
/// Supports decimal second timestamps (`start_sec`/`end_sec`) for freeform/live/YouTube songs
/// as well as 1-based bar numbers `[start_bar, end_bar)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionDef {
    pub id: String,
    pub name: String,
    #[serde(default = "default_start_bar")]
    pub start_bar: u32,
    #[serde(default = "default_end_bar")]
    pub end_bar: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_sec: Option<f64>,
    #[serde(default)]
    pub color: Option<String>,
}

fn default_start_bar() -> u32 {
    1
}

fn default_end_bar() -> u32 {
    2
}

/// Stem definition supporting either a bare filename string or an object with `pitched` metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StemDef {
    File(String),
    Detailed {
        file: String,
        #[serde(default = "default_true")]
        pitched: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_cues: Option<bool>,
        #[serde(default = "default_gain")]
        default_gain: f32,
        #[serde(default)]
        muted: bool,
    },
}

fn default_true() -> bool {
    true
}

fn default_gain() -> f32 {
    1.0
}

fn default_crossfade_sec() -> f32 {
    2.0
}

impl StemDef {
    pub fn file_name(&self) -> &str {
        match self {
            StemDef::File(f) => f,
            StemDef::Detailed { file, .. } => file,
        }
    }

    pub fn is_pitched(&self, stem_key: &str) -> bool {
        match self {
            StemDef::File(_) => !matches!(
                stem_key,
                "drums"
                    | "percussion"
                    | "cues"
                    | "click"
                    | "kick"
                    | "snare"
                    | "hihat"
                    | "hand_drums"
                    | "tambourine"
                    | "shaker"
                    | "claps"
                    | "clave"
                    | "bell"
            ),
            StemDef::Detailed { pitched, .. } => *pitched,
        }
    }

    pub fn is_cues(&self, stem_key: &str) -> bool {
        match self {
            StemDef::File(_) => matches!(
                stem_key.to_ascii_lowercase().as_str(),
                "cues" | "cue" | "guide"
            ),
            StemDef::Detailed { is_cues, .. } => is_cues.unwrap_or_else(|| {
                matches!(
                    stem_key.to_ascii_lowercase().as_str(),
                    "cues" | "cue" | "guide"
                )
            }),
        }
    }

    pub fn is_muted(&self) -> bool {
        match self {
            StemDef::File(_) => false,
            StemDef::Detailed { muted, .. } => *muted,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransitionMode {
    AutoAdvance,
    AutoPause,
    Manual,
    ContinueUntilStopped,
}

impl Default for TransitionMode {
    fn default() -> Self {
        Self::AutoPause
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SetlistSlotKind {
    #[default]
    SongBundle,
    PadOnly,
    PercussionGroove,
}

fn default_mixer_master_gain() -> f32 {
    1.0
}

fn default_mixer_click_gain() -> f32 {
    0.85
}

fn default_stem_mix_gain() -> f32 {
    1.0
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StemMixOverride {
    pub name: String,
    #[serde(default = "default_stem_mix_gain")]
    pub gain: f32,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetlistMixerConfig {
    #[serde(default = "default_mixer_master_gain")]
    pub master_gain: f32,
    #[serde(default = "default_mixer_click_gain")]
    pub click_gain: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_muted: Option<bool>,
}

impl Default for SetlistMixerConfig {
    fn default() -> Self {
        Self {
            master_gain: default_mixer_master_gain(),
            click_gain: default_mixer_click_gain(),
            click_muted: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetlistManifest {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub sections: Vec<SetlistSection>,
    #[serde(default)]
    pub items: Vec<SetlistItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mixer: Option<SetlistMixerConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadLockConfig {
    pub enabled: bool,
    pub gain: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset_name: Option<String>,
    #[serde(default)]
    pub layers: Vec<PadLayerConfig>,
    pub brightness: f32,
    pub shimmer: f32,
    #[serde(default)]
    pub pan: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SetlistItem {
    pub song_id: String,
    #[serde(default)]
    pub song_dir: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub original_key: Option<String>,
    #[serde(default)]
    pub base_bpm: Option<f64>,
    #[serde(default)]
    pub base_click_mode: Option<ClickMode>,
    #[serde(default)]
    pub key_shift_semitones: i32,
    #[serde(default)]
    pub bpm_override: Option<f64>,
    #[serde(default)]
    pub click_mode: Option<ClickMode>,
    #[serde(default)]
    pub transition_mode: TransitionMode,
    #[serde(default = "default_crossfade_sec")]
    pub crossfade_sec: f32,
    #[serde(default = "default_true")]
    pub ambient_pad_enabled: bool,
    #[serde(default)]
    pub pad_key_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_lock: Option<PadLockConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
    #[serde(default)]
    pub slot_kind: SetlistSlotKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<[u8; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percussion_lock: Option<PercussionLockConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stem_mix: Vec<StemMixOverride>,
}

impl Default for SetlistItem {
    fn default() -> Self {
        Self {
            song_id: String::new(),
            song_dir: None,
            title: None,
            original_key: None,
            base_bpm: None,
            base_click_mode: None,
            key_shift_semitones: 0,
            bpm_override: None,
            click_mode: None,
            transition_mode: TransitionMode::AutoPause,
            crossfade_sec: default_crossfade_sec(),
            ambient_pad_enabled: true,
            pad_key_override: None,
            pad_lock: None,
            section_id: None,
            slot_kind: SetlistSlotKind::SongBundle,
            time_signature: None,
            percussion_lock: None,
            stem_mix: Vec::new(),
        }
    }
}

impl SetlistManifest {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(AudioCoreError::Manifest(
                "Setlist name cannot be empty".into(),
            ));
        }
        for (idx, item) in self.items.iter().enumerate() {
            if item.song_id.trim().is_empty() {
                return Err(AudioCoreError::Manifest(format!(
                    "Setlist slot {} has empty song_id",
                    idx + 1
                )));
            }
            if !(-12..=12).contains(&item.key_shift_semitones) {
                return Err(AudioCoreError::Manifest(format!(
                    "Setlist slot {} key_shift_semitones {} out of range -12..=12",
                    idx + 1,
                    item.key_shift_semitones
                )));
            }
            if let Some(bpm) = item.bpm_override {
                if !(30.0..=260.0).contains(&bpm) {
                    return Err(AudioCoreError::Manifest(format!(
                        "Setlist slot {} bpm_override {} out of range 30..=260",
                        idx + 1,
                        bpm
                    )));
                }
            }
            if !(0.0..=10.0).contains(&item.crossfade_sec) {
                return Err(AudioCoreError::Manifest(format!(
                    "Setlist slot {} crossfade_sec {} out of range 0..=10",
                    idx + 1,
                    item.crossfade_sec
                )));
            }
            if let Some(ref lock) = item.pad_lock {
                if lock.layers.len() > MAX_PAD_LAYERS {
                    return Err(AudioCoreError::Manifest(format!(
                        "Setlist slot {} pad_lock has {} layers (max {MAX_PAD_LAYERS})",
                        idx + 1,
                        lock.layers.len()
                    )));
                }
                if !(0.0..=1.5).contains(&lock.gain)
                    || !(0.0..=1.0).contains(&lock.brightness)
                    || !(0.0..=1.0).contains(&lock.shimmer)
                    || !(-1.0..=1.0).contains(&lock.pan)
                {
                    return Err(AudioCoreError::Manifest(format!(
                        "Setlist slot {} pad_lock has out-of-range parameters",
                        idx + 1
                    )));
                }
            }
            if let Some(ref perc) = item.percussion_lock {
                if !(0.0..=1.5).contains(&perc.gain) || !(0.0..=1.0).contains(&perc.iem_send) {
                    return Err(AudioCoreError::Manifest(format!(
                        "Setlist slot {} percussion_lock has out-of-range gain/iem_send",
                        idx + 1
                    )));
                }
                for ch in &perc.channels {
                    if !(0.0..=1.5).contains(&ch.gain) || !(-1.0..=1.0).contains(&ch.pan) {
                        return Err(AudioCoreError::Manifest(format!(
                            "Setlist slot {} percussion_lock channel has out-of-range parameters",
                            idx + 1
                        )));
                    }
                }
            }
            for stem in &item.stem_mix {
                if stem.name.trim().is_empty() || !(0.0..=1.5).contains(&stem.gain) {
                    return Err(AudioCoreError::Manifest(format!(
                        "Setlist slot {} stem_mix '{}' has invalid gain {}",
                        idx + 1,
                        stem.name,
                        stem.gain
                    )));
                }
            }
        }
        if let Some(ref mixer) = self.mixer {
            if !(0.0..=1.5).contains(&mixer.master_gain) || !(0.0..=1.5).contains(&mixer.click_gain)
            {
                return Err(AudioCoreError::Manifest(
                    "Setlist mixer master_gain/click_gain out of range 0.0..=1.5".into(),
                ));
            }
        }
        Ok(())
    }
}

impl SongManifest {
    pub fn from_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        let manifest: Self = serde_json::from_str(&raw)
            .map_err(|e| AudioCoreError::Manifest(format!("{}: {e}", path.display())))?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<()> {
        if self.bpm <= 10.0 || self.bpm > 300.0 {
            return Err(AudioCoreError::Manifest(format!(
                "Invalid BPM {} for {}",
                self.bpm, self.title
            )));
        }
        if self.time_signature[0] == 0 {
            return Err(AudioCoreError::Manifest(
                "Time signature numerator must be > 0".into(),
            ));
        }
        for sec in &self.sections {
            match (sec.start_sec, sec.end_sec) {
                (Some(s), Some(e)) => {
                    if !s.is_finite() || !e.is_finite() || s < 0.0 || e <= s + 0.01 {
                        return Err(AudioCoreError::Manifest(format!(
                            "Section '{}' has invalid time range {:.2}s..{:.2}s",
                            sec.name, s, e
                        )));
                    }
                }
                _ => {
                    if sec.start_bar == 0 || sec.end_bar <= sec.start_bar {
                        return Err(AudioCoreError::Manifest(format!(
                            "Section '{}' has invalid bar range {}..{}",
                            sec.name, sec.start_bar, sec.end_bar
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    pub fn beats_per_bar(&self) -> u32 {
        self.time_signature[0].max(1) as u32
    }

    pub fn seconds_per_beat(&self) -> f64 {
        60.0 / self.bpm
    }

    /// Returns the absolute timeline start timestamp (seconds) of a section.
    /// Uses decimal `start_sec` when present, otherwise converts `start_bar`.
    pub fn section_start_seconds(&self, sec: &SectionDef) -> f64 {
        sec.start_sec
            .filter(|v| v.is_finite() && *v >= 0.0)
            .unwrap_or_else(|| self.bar_to_seconds(sec.start_bar))
    }

    /// Returns the absolute timeline end timestamp (seconds) of a section.
    /// Uses decimal `end_sec` when valid, otherwise converts `end_bar`.
    pub fn section_end_seconds(&self, sec: &SectionDef) -> f64 {
        let start = self.section_start_seconds(sec);
        sec.end_sec
            .filter(|v| v.is_finite() && *v > start)
            .unwrap_or_else(|| self.bar_to_seconds(sec.end_bar))
    }

    /// Returns true if playback position `(time_sec, bar)` lies inside `sec`.
    pub fn section_contains_at(&self, sec: &SectionDef, time_sec: f64, bar: u32) -> bool {
        match (sec.start_sec, sec.end_sec) {
            (Some(s), Some(e)) => time_sec >= s && time_sec < e,
            _ => bar >= sec.start_bar && bar < sec.end_bar,
        }
    }

    /// Returns true if `(time_sec, bar)` has reached or passed the end of `sec`.
    pub fn section_ended_at(&self, sec: &SectionDef, time_sec: f64, bar: u32) -> bool {
        match sec.end_sec {
            Some(e) if e.is_finite() => time_sec >= e,
            _ => bar >= sec.end_bar,
        }
    }

    /// Returns the 1-based bar number where the song's Intro finishes (so `ClickMode::IntroOnly`
    /// switches off the metronome automatically at that bar downbeat).
    pub fn intro_end_bar(&self) -> u32 {
        if let Some(intro) = self
            .sections
            .iter()
            .find(|s| s.name.to_ascii_lowercase().contains("intro"))
        {
            return intro.end_bar.max(2);
        }
        if self.sections.len() > 1 && self.sections[0].start_bar == 1 {
            return self.sections[0].end_bar.max(2);
        }
        5
    }

    /// Returns the timeline timestamp (seconds) where the Intro finishes for `ClickMode::IntroOnly`.
    pub fn intro_end_seconds(&self) -> f64 {
        if let Some(intro) = self
            .sections
            .iter()
            .find(|s| s.name.to_ascii_lowercase().contains("intro"))
        {
            return self.section_end_seconds(intro);
        }
        if self.sections.len() > 1
            && (self.sections[0].start_bar == 1
                || self.section_start_seconds(&self.sections[0]) <= self.first_downbeat_sec + 0.5)
        {
            return self.section_end_seconds(&self.sections[0]);
        }
        self.bar_to_seconds(5)
    }

    /// Converts a 1-based bar number to an absolute source timestamp in seconds.
    pub fn bar_to_seconds(&self, bar_1_based: u32) -> f64 {
        let zero_based_bars = bar_1_based.saturating_sub(1) as usize;
        let beat_index = zero_based_bars * self.beats_per_bar() as usize;
        self.first_downbeat_sec + (beat_index as f64) * self.seconds_per_beat()
    }

    /// Returns `(bar_1_based, beat_in_bar_1_based, fractional_beat)` for a source timestamp.
    pub fn seconds_to_bar_beat(&self, time_sec: f64) -> (u32, u32, f64) {
        let bpb = self.beats_per_bar();
        let elapsed_from_downbeat = (time_sec - self.first_downbeat_sec).max(0.0);
        let total_beats = elapsed_from_downbeat / self.seconds_per_beat();
        let bpb_f = bpb as f64;
        let bar = (total_beats / bpb_f).floor() as u32 + 1;
        let beat_in_bar = (total_beats % bpb_f).floor() as u32 + 1;
        let frac = total_beats.fract();
        (bar, beat_in_bar, frac)
    }

    /// Returns continuous 0-based beat position (`0.0` = beat 1 of bar 1) for groove/percussion sync.
    pub fn seconds_to_continuous_beat(&self, time_sec: f64) -> Option<f64> {
        if time_sec < self.first_downbeat_sec {
            return None;
        }
        Some((time_sec - self.first_downbeat_sec) / self.seconds_per_beat())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_beat_math_round_trips() {
        let m = SongManifest {
            id: "test".into(),
            title: "Test Song".into(),
            artist: "".into(),
            bpm: 120.0,
            time_signature: [4, 4],
            first_downbeat_sec: 0.5,
            original_key: "G".into(),
            click_mode: ClickMode::AlwaysOn,
            provenance: None,
            sections: vec![SectionDef {
                id: "verse".into(),
                name: "Verse 1".into(),
                start_bar: 1,
                end_bar: 5,
                start_sec: None,
                end_sec: None,
                color: None,
            }],
            stems: HashMap::new(),
        };

        // At 120 BPM, 1 beat = 0.5s, 1 bar = 2.0s. Bar 3 starts at 0.5 + 4.0 = 4.5s.
        let t = m.bar_to_seconds(3);
        assert!((t - 4.5).abs() < 1e-6);

        let (bar, beat, _frac) = m.seconds_to_bar_beat(4.55);
        assert_eq!(bar, 3);
        assert_eq!(beat, 1);
    }

    #[test]
    fn legacy_beats_field_in_manifest_json_is_ignored() {
        let json = r#"{
            "id": "legacy",
            "title": "Legacy Fixed Song",
            "bpm": 120.0,
            "first_downbeat_sec": 0.5,
            "beats": [0.5, 0.9, 1.8, 2.2],
            "sections": [],
            "stems": {}
        }"#;
        let parsed: SongManifest = serde_json::from_str(json).expect("ignore legacy beats");
        assert!((parsed.bar_to_seconds(2) - 2.5).abs() < 1e-6);
    }

    #[test]
    fn test_manifest_validation_rejects_invalid_bpm() {
        let mut m = SongManifest {
            id: "test".into(),
            title: "Test".into(),
            artist: "".into(),
            bpm: 5.0, // Invalid: <= 10.0
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            original_key: "C".into(),
            click_mode: ClickMode::AlwaysOn,
            provenance: None,
            sections: vec![],
            stems: HashMap::new(),
        };
        assert!(m.validate().is_err());

        m.bpm = 350.0; // Invalid: > 300.0
        assert!(m.validate().is_err());

        m.bpm = 72.0; // Valid
        assert!(m.validate().is_ok());
    }

    #[test]
    fn test_manifest_validation_rejects_zero_time_sig() {
        let m = SongManifest {
            id: "test".into(),
            title: "Test".into(),
            artist: "".into(),
            bpm: 120.0,
            time_signature: [0, 4], // Invalid: numerator must be > 0
            first_downbeat_sec: 0.0,
            original_key: "C".into(),
            click_mode: ClickMode::AlwaysOn,
            provenance: None,
            sections: vec![],
            stems: HashMap::new(),
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_manifest_validation_rejects_invalid_section_bars() {
        let m = SongManifest {
            id: "test".into(),
            title: "Test".into(),
            artist: "".into(),
            bpm: 120.0,
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            original_key: "C".into(),
            click_mode: ClickMode::AlwaysOn,
            provenance: None,
            sections: vec![SectionDef {
                id: "intro".into(),
                name: "Intro".into(),
                start_bar: 5,
                end_bar: 4, // Invalid: end_bar <= start_bar
                start_sec: None,
                end_sec: None,
                color: None,
            }],
            stems: HashMap::new(),
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_canonicalize_youtube_source_ref() {
        assert_eq!(
            canonicalize_source_ref("youtube", "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=12s"),
            "youtube:dQw4w9WgXcQ"
        );
        assert_eq!(
            canonicalize_source_ref("youtube", "https://youtu.be/dQw4w9WgXcQ?si=abc"),
            "youtube:dQw4w9WgXcQ"
        );
    }

    #[test]
    fn test_stem_def_is_pitched_and_cues() {
        let drums = StemDef::Detailed {
            file: "drums.wav".into(),
            pitched: false,
            is_cues: None,
            default_gain: 0.9,
            muted: false,
        };
        assert!(!drums.is_pitched("drums"));
        assert!(!drums.is_cues("drums"));
        assert_eq!(drums.file_name(), "drums.wav");

        let bass = StemDef::Detailed {
            file: "bass.wav".into(),
            pitched: true,
            is_cues: Some(false),
            default_gain: 0.9,
            muted: false,
        };
        assert!(bass.is_pitched("bass"));
        assert!(!bass.is_cues("bass"));

        let cues = StemDef::Detailed {
            file: "guide.wav".into(),
            pitched: false,
            is_cues: Some(true),
            default_gain: 0.8,
            muted: true,
        };
        assert!(!cues.is_pitched("custom_guide"));
        assert!(cues.is_cues("custom_guide"));

        let bare_drums = StemDef::File("drums.wav".into());
        assert!(!bare_drums.is_pitched("drums"));
        let bare_synth = StemDef::File("synth.wav".into());
        assert!(bare_synth.is_pitched("synth"));
    }

    #[test]
    fn test_setlist_manifest_deserialization() {
        let json = r#"{
            "id": "sunday-service",
            "name": "Sunday Morning Set",
            "items": [
                { "song_id": "living-hope", "key_shift_semitones": 2, "bpm_override": 74.0 },
                { "song_id": "gratitude", "key_shift_semitones": 0 }
            ]
        }"#;

        let setlist: SetlistManifest = serde_json::from_str(json).expect("deserialize setlist");
        assert_eq!(setlist.id, "sunday-service");
        assert_eq!(setlist.items.len(), 2);
        assert_eq!(setlist.items[0].key_shift_semitones, 2);
        assert_eq!(setlist.items[0].bpm_override, Some(74.0));
        assert_eq!(setlist.items[0].transition_mode, TransitionMode::AutoPause);
        assert!(setlist.items[0].ambient_pad_enabled);
        assert_eq!(setlist.items[0].pad_lock, None);
        assert_eq!(setlist.items[1].key_shift_semitones, 0);
        assert_eq!(setlist.items[1].bpm_override, None);
        assert_eq!(setlist.items[1].pad_lock, None);
        assert!(setlist.validate().is_ok());
    }

    #[test]
    fn test_setlist_manifest_validation() {
        let mut setlist = SetlistManifest {
            id: "set1".into(),
            name: "Sunday Set".into(),
            sections: vec![],
            items: vec![SetlistItem {
                song_id: "song1".into(),
                song_dir: Some("/songs/song1".into()),
                title: Some("Song 1".into()),
                original_key: Some("G".into()),
                base_bpm: Some(72.0),
                base_click_mode: Some(ClickMode::IntroOnly),
                key_shift_semitones: 15, // invalid
                bpm_override: None,
                click_mode: Some(ClickMode::IntroOnly),
                transition_mode: TransitionMode::AutoAdvance,
                crossfade_sec: 2.0,
                ambient_pad_enabled: true,
                pad_key_override: None,
                pad_lock: None,
                ..Default::default()
            }],
            mixer: None,
        };
        assert!(setlist.validate().is_err());

        setlist.items[0].key_shift_semitones = 2;
        setlist.items[0].bpm_override = Some(300.0); // invalid
        assert!(setlist.validate().is_err());

        setlist.items[0].bpm_override = Some(76.0);
        assert!(setlist.validate().is_ok());
    }

    #[test]
    fn test_transition_mode_serialization() {
        for mode in [
            TransitionMode::AutoAdvance,
            TransitionMode::AutoPause,
            TransitionMode::Manual,
            TransitionMode::ContinueUntilStopped,
        ] {
            let json = serde_json::to_string(&mode).expect("serialize");
            let parsed: TransitionMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn test_click_mode_serialization_and_intro_end_bar() {
        for mode in [ClickMode::AlwaysOn, ClickMode::IntroOnly, ClickMode::Off] {
            let json = serde_json::to_string(&mode).expect("serialize");
            let parsed: ClickMode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(parsed, mode);
        }

        let m = SongManifest {
            id: "intro-song".into(),
            title: "Intro Only Song".into(),
            artist: "".into(),
            bpm: 100.0,
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            original_key: "C".into(),
            click_mode: ClickMode::IntroOnly,
            provenance: None,
            sections: vec![
                SectionDef {
                    id: "s1".into(),
                    name: "Intro (8 bars)".into(),
                    start_bar: 1,
                    end_bar: 9,
                    start_sec: None,
                    end_sec: None,
                    color: None,
                },
                SectionDef {
                    id: "s2".into(),
                    name: "Verse 1".into(),
                    start_bar: 9,
                    end_bar: 17,
                    start_sec: None,
                    end_sec: None,
                    color: None,
                },
            ],
            stems: HashMap::new(),
        };
        assert_eq!(m.intro_end_bar(), 9);
    }

    #[test]
    fn test_decimal_timeline_sections_validation_and_intro_end_seconds() {
        let mut m = SongManifest {
            id: "yt-live".into(),
            title: "Live YouTube Rip".into(),
            artist: "".into(),
            bpm: 120.0,
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            original_key: "C".into(),
            click_mode: ClickMode::IntroOnly,
            provenance: None,
            sections: vec![
                SectionDef {
                    id: "intro".into(),
                    name: "Intro".into(),
                    start_bar: 1,
                    end_bar: 2,
                    start_sec: Some(1.37),
                    end_sec: Some(12.84),
                    color: None,
                },
                SectionDef {
                    id: "verse".into(),
                    name: "Verse 1".into(),
                    start_bar: 2,
                    end_bar: 3,
                    start_sec: Some(12.84),
                    end_sec: Some(29.45),
                    color: None,
                },
            ],
            stems: HashMap::new(),
        };
        assert!(m.validate().is_ok());
        assert!((m.intro_end_seconds() - 12.84).abs() < 1e-6);
        assert!(m.section_contains_at(&m.sections[0], 12.83, 7));
        assert!(!m.section_contains_at(&m.sections[0], 12.84, 7));
        assert!(m.section_contains_at(&m.sections[1], 12.84, 7));

        // Invalid decimal time range (end_sec <= start_sec) fails validation
        m.sections[0].end_sec = Some(1.30);
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_setlist_sections_backward_compat_deserialization() {
        let legacy_json = r#"{
            "id": "legacy-setlist",
            "name": "Legacy Sunday",
            "items": [
                {
                    "song_id": "song-1",
                    "key_shift_semitones": 2,
                    "transition_mode": "AutoPause",
                    "crossfade_sec": 2.0,
                    "ambient_pad_enabled": true
                }
            ]
        }"#;
        let parsed: SetlistManifest = serde_json::from_str(legacy_json).expect("deserialize legacy setlist");
        assert!(parsed.sections.is_empty());
        assert_eq!(parsed.items.len(), 1);
        assert_eq!(parsed.items[0].slot_kind, SetlistSlotKind::SongBundle);
        assert!(parsed.items[0].section_id.is_none());
        assert!(parsed.items[0].percussion_lock.is_none());
        assert!(parsed.validate().is_ok());

        let modern_json = r##"{
            "id": "sunday-sec",
            "name": "Sunday With Sections",
            "sections": [
                { "id": "sec-praise", "name": "Praise", "kind": "Praise", "color": "#f59e0b", "collapsed": false },
                { "id": "sec-sermon", "name": "Sermon", "kind": "Sermon", "collapsed": true }
            ],
            "items": [
                {
                    "song_id": "pad-sermon",
                    "title": "Altar Underscore",
                    "original_key": "D",
                    "base_bpm": 68.0,
                    "slot_kind": "PadOnly",
                    "section_id": "sec-sermon",
                    "time_signature": [4, 4],
                    "key_shift_semitones": 0,
                    "transition_mode": "Manual",
                    "crossfade_sec": 3.5,
                    "ambient_pad_enabled": true
                }
            ]
        }"##;
        let parsed_modern: SetlistManifest = serde_json::from_str(modern_json).expect("deserialize modern setlist");
        assert_eq!(parsed_modern.sections.len(), 2);
        assert_eq!(parsed_modern.sections[0].kind, ServiceSectionKind::Praise);
        assert_eq!(parsed_modern.sections[1].kind, ServiceSectionKind::Sermon);
        assert_eq!(parsed_modern.items[0].slot_kind, SetlistSlotKind::PadOnly);
        assert_eq!(parsed_modern.items[0].section_id.as_deref(), Some("sec-sermon"));
        assert!(parsed_modern.validate().is_ok());
    }
}
