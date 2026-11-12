use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::{AudioCoreError, Result};

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
    /// Optional explicit beat timestamps (seconds) from Phase 2 AI beat tracking.
    #[serde(default)]
    pub beats: Vec<f64>,
    pub sections: Vec<SectionDef>,
    pub stems: HashMap<String, StemDef>,
}

fn default_time_signature() -> [u8; 2] {
    [4, 4]
}

fn default_key() -> String {
    "C".to_string()
}

/// A named worship song section defined in 1-based bar numbers `[start_bar, end_bar)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionDef {
    pub id: String,
    pub name: String,
    pub start_bar: u32,
    pub end_bar: u32,
    #[serde(default)]
    pub color: Option<String>,
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
        #[serde(default = "default_gain")]
        default_gain: f32,
    },
}

fn default_true() -> bool {
    true
}

fn default_gain() -> f32 {
    1.0
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
            StemDef::File(_) => !matches!(stem_key, "drums" | "percussion" | "cues" | "click"),
            StemDef::Detailed { pitched, .. } => *pitched,
        }
    }

    pub fn is_cues(&self, stem_key: &str) -> bool {
        stem_key == "cues"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistManifest {
    pub id: String,
    pub name: String,
    pub items: Vec<SetlistItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistItem {
    pub song_id: String,
    #[serde(default)]
    pub key_shift_semitones: i32,
    #[serde(default)]
    pub bpm_override: Option<f64>,
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
            if sec.start_bar == 0 || sec.end_bar <= sec.start_bar {
                return Err(AudioCoreError::Manifest(format!(
                    "Section '{}' has invalid bar range {}..{}",
                    sec.name, sec.start_bar, sec.end_bar
                )));
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

    /// Converts a 1-based bar number to an absolute source timestamp in seconds.
    pub fn bar_to_seconds(&self, bar_1_based: u32) -> f64 {
        let zero_based_bars = bar_1_based.saturating_sub(1) as f64;
        let beat_index = zero_based_bars * self.beats_per_bar() as f64;
        self.first_downbeat_sec + beat_index * self.seconds_per_beat()
    }

    /// Returns `(bar_1_based, beat_in_bar_1_based, fractional_beat)` for a source timestamp.
    pub fn seconds_to_bar_beat(&self, time_sec: f64) -> (u32, u32, f64) {
        let elapsed_from_downbeat = (time_sec - self.first_downbeat_sec).max(0.0);
        let total_beats = elapsed_from_downbeat / self.seconds_per_beat();
        let bpb = self.beats_per_bar() as f64;
        let bar = (total_beats / bpb).floor() as u32 + 1;
        let beat_in_bar = (total_beats % bpb).floor() as u32 + 1;
        let frac = total_beats.fract();
        (bar, beat_in_bar, frac)
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
            beats: vec![],
            sections: vec![SectionDef {
                id: "verse".into(),
                name: "Verse 1".into(),
                start_bar: 1,
                end_bar: 5,
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
    fn test_manifest_validation_rejects_invalid_bpm() {
        let mut m = SongManifest {
            id: "test".into(),
            title: "Test".into(),
            artist: "".into(),
            bpm: 5.0, // Invalid: <= 10.0
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            original_key: "C".into(),
            beats: vec![],
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
            beats: vec![],
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
            beats: vec![],
            sections: vec![SectionDef {
                id: "intro".into(),
                name: "Intro".into(),
                start_bar: 5,
                end_bar: 4, // Invalid: end_bar <= start_bar
                color: None,
            }],
            stems: HashMap::new(),
        };
        assert!(m.validate().is_err());
    }

    #[test]
    fn test_stem_def_is_pitched_and_cues() {
        let drums = StemDef::Detailed {
            file: "drums.wav".into(),
            pitched: false,
            default_gain: 0.9,
        };
        assert!(!drums.is_pitched("drums"));
        assert!(!drums.is_cues("drums"));
        assert_eq!(drums.file_name(), "drums.wav");

        let bass = StemDef::Detailed {
            file: "bass.wav".into(),
            pitched: true,
            default_gain: 0.9,
        };
        assert!(bass.is_pitched("bass"));
        assert!(!bass.is_cues("bass"));

        let cues = StemDef::Detailed {
            file: "cues.wav".into(),
            pitched: false,
            default_gain: 0.8,
        };
        assert!(!cues.is_pitched("cues"));
        assert!(cues.is_cues("cues"));

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
        assert_eq!(setlist.items[1].key_shift_semitones, 0);
        assert_eq!(setlist.items[1].bpm_override, None);
    }
}
