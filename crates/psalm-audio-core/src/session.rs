use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::engine::{EngineSnapshot, QuantizeMode};
use crate::manifest::{ClickMode, SetlistItem, SetlistManifest};
use crate::pad::{normalize_key_label, PadLayerConfig};
use crate::percussion::{PercussionChannelConfig, DEFAULT_PERCUSSION_ROOM_MIX};
use crate::percussion_patterns::{
    CustomPercussionPattern, GrooveIntensity, PercussionMode, PercussionStyle,
};
use crate::routing::OutputRoutingMode;
use crate::Result;

pub const STAGE_SESSION_FILENAME: &str = "stage-session.json";

/// Persistent live stage configuration & active setlist session saved across app restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StageSessionState {
    pub version: u32,
    pub device_name: Option<String>,
    pub routing_mode: OutputRoutingMode,
    pub setlist: SetlistManifest,
    pub active_setlist_slot: Option<usize>,
    pub active_song_dir: Option<String>,
    pub key_shift_semitones: i32,
    pub current_bpm: f64,
    pub master_gain: f32,
    pub backing_gain: f32,
    pub click_gain: f32,
    pub click_muted: bool,
    pub click_mode: ClickMode,
    pub quantize_mode: QuantizeMode,
    pub loop_enabled: bool,
    pub continue_until_stopped: bool,
    // Tonic Pad configuration (pad_enabled always boots false on cold launch for safety).
    pub pad_muted: bool,
    pub pad_gain: f32,
    pub pad_key: String,
    pub pad_is_minor: bool,
    pub pad_follow_song_key: bool,
    pub pad_preset_id: Option<String>,
    pub pad_preset_name: String,
    pub pad_layers: Vec<PadLayerConfig>,
    pub pad_brightness: f32,
    pub pad_shimmer: f32,
    pub pad_pan: f32,
    pub pad_crossfade_sec: f32,
    // Percussion Track configuration
    pub percussion_mode: PercussionMode,
    pub percussion_style: PercussionStyle,
    pub percussion_intensity: GrooveIntensity,
    pub percussion_follow_sections: bool,
    pub percussion_gain: f32,
    pub percussion_iem_send: f32,
    pub percussion_room_mix: f32,
    pub percussion_kit_name: Option<String>,
    pub percussion_kit_dir: Option<String>,
    pub percussion_custom_pattern: Option<CustomPercussionPattern>,
    pub percussion_channels: Vec<PercussionChannelConfig>,
}

impl Default for StageSessionState {
    fn default() -> Self {
        Self {
            version: 1,
            device_name: None,
            routing_mode: OutputRoutingMode::default(),
            setlist: SetlistManifest {
                id: "live-stage-session".to_string(),
                name: "Sunday Morning Set".to_string(),
                sections: Vec::new(),
                items: Vec::new(),
                mixer: None,
            },
            active_setlist_slot: None,
            active_song_dir: None,
            key_shift_semitones: 0,
            current_bpm: 120.0,
            master_gain: 1.0,
            backing_gain: 1.0,
            click_gain: 0.85,
            click_muted: false,
            click_mode: ClickMode::default(),
            quantize_mode: QuantizeMode::default(),
            loop_enabled: false,
            continue_until_stopped: false,
            pad_muted: false,
            pad_gain: 0.45,
            pad_key: "C".to_string(),
            pad_is_minor: false,
            pad_follow_song_key: true,
            pad_preset_id: None,
            pad_preset_name: "Foundation Pad".to_string(),
            pad_layers: Vec::new(),
            pad_brightness: 0.55,
            pad_shimmer: 0.35,
            pad_pan: 0.0,
            pad_crossfade_sec: 1.5,
            percussion_mode: PercussionMode::Off,
            percussion_style: PercussionStyle::default(),
            percussion_intensity: GrooveIntensity::default(),
            percussion_follow_sections: true,
            percussion_gain: 0.9,
            percussion_iem_send: 0.0,
            percussion_room_mix: DEFAULT_PERCUSSION_ROOM_MIX,
            percussion_kit_name: None,
            percussion_kit_dir: None,
            percussion_custom_pattern: None,
            percussion_channels: Vec::new(),
        }
    }
}

impl StageSessionState {
    /// Captures persistent stage state from a live `EngineSnapshot`.
    pub fn from_snapshot(snap: &EngineSnapshot) -> Self {
        let setlist_name = if snap.setlist_name.trim().is_empty() {
            "Sunday Morning Set".to_string()
        } else {
            snap.setlist_name.clone()
        };
        let items: Vec<SetlistItem> = snap
            .setlist_items
            .iter()
            .map(|item| SetlistItem {
                song_id: item.song_id.clone(),
                song_dir: item.song_dir.clone(),
                title: Some(item.title.clone()),
                original_key: Some(item.original_key.clone()),
                base_bpm: Some(item.base_bpm),
                base_click_mode: Some(item.effective_click_mode),
                key_shift_semitones: item.key_shift_semitones,
                bpm_override: item.bpm_override,
                click_mode: item.click_mode,
                transition_mode: item.transition_mode,
                crossfade_sec: item.crossfade_sec,
                ambient_pad_enabled: item.ambient_pad_enabled,
                pad_key_override: item.pad_key_override.clone(),
                pad_lock: item.pad_lock.clone(),
                section_id: item.section_id.clone(),
                slot_kind: item.slot_kind,
                time_signature: item.time_signature,
                percussion_lock: item.percussion_lock.clone(),
                stem_mix: item.stem_mix.clone(),
            })
            .collect();

        let device_name = {
            let trimmed = snap.device_name.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        };

        Self {
            version: 1,
            device_name,
            routing_mode: snap.routing_mode,
            setlist: SetlistManifest {
                id: "live-stage-session".to_string(),
                name: setlist_name,
                sections: snap.setlist_sections.clone(),
                items,
                mixer: None,
            },
            active_setlist_slot: snap.active_setlist_slot,
            active_song_dir: snap.active_song_dir.clone(),
            key_shift_semitones: snap.key_shift_semitones,
            current_bpm: snap.current_bpm,
            master_gain: snap.master_gain,
            backing_gain: snap.backing_gain,
            click_gain: snap.click_gain,
            click_muted: snap.click_muted,
            click_mode: snap.click_mode,
            quantize_mode: snap.quantize_mode,
            loop_enabled: snap.loop_enabled,
            continue_until_stopped: snap.continue_until_stopped,
            pad_muted: snap.pad_muted,
            pad_gain: snap.pad_gain,
            pad_key: snap.pad_key.clone(),
            pad_is_minor: snap.pad_is_minor,
            pad_follow_song_key: snap.pad_follow_song_key,
            pad_preset_id: snap.pad_preset_id.clone(),
            pad_preset_name: snap.pad_preset_name.clone(),
            pad_layers: snap.pad_layers.clone(),
            pad_brightness: snap.pad_brightness,
            pad_shimmer: snap.pad_shimmer,
            pad_pan: snap.pad_pan,
            pad_crossfade_sec: snap.pad_crossfade_sec,
            percussion_mode: snap.percussion_mode,
            percussion_style: snap.percussion_style,
            percussion_intensity: snap.percussion_intensity,
            percussion_follow_sections: snap.percussion_follow_sections,
            percussion_gain: snap.percussion_gain,
            percussion_iem_send: snap.percussion_iem_send,
            percussion_room_mix: snap.percussion_room_mix,
            percussion_kit_name: snap.percussion_kit_name.clone(),
            percussion_kit_dir: snap.percussion_kit_dir.clone(),
            percussion_custom_pattern: snap.percussion_custom_pattern.clone(),
            percussion_channels: snap.percussion_channels.clone(),
        }
    }

    /// Returns a canonical key string accepted by `PadSynth::set_key` (e.g., `"Am"`, `"F#"`).
    pub fn pad_key_label(&self) -> String {
        let base = normalize_key_label(&self.pad_key);
        if self.pad_is_minor && !base.ends_with('m') {
            format!("{base}m")
        } else {
            base
        }
    }
}

/// Loads `stage-session.json` if present and valid; returns `None` on missing or corrupt data.
pub fn load_stage_session(path: &Path) -> Option<StageSessionState> {
    if !path.is_file() {
        return None;
    }
    match fs::read_to_string(path) {
        Ok(raw) => match serde_json::from_str::<StageSessionState>(&raw) {
            Ok(session) => Some(session),
            Err(err) => {
                log::warn!("Ignoring malformed stage session at {:?}: {err}", path);
                None
            }
        },
        Err(err) => {
            log::warn!("Could not read stage session at {:?}: {err}", path);
            None
        }
    }
}

/// Atomically writes `stage-session.json` via a sibling `.tmp` file rename.
pub fn save_stage_session_atomic(path: &Path, session: &StageSessionState) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(session).map_err(|err| {
        crate::AudioCoreError::Manifest(format!("Failed to serialize stage session: {err}"))
    })?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ServiceSectionKind, SetlistSection, SetlistSlotKind, TransitionMode};
    use crate::percussion_patterns::PercussionInstrument;

    #[test]
    fn test_stage_session_save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "psalm_stage_session_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = dir.join(STAGE_SESSION_FILENAME);

        let session = StageSessionState {
            device_name: Some("USB Audio Pro".into()),
            routing_mode: OutputRoutingMode::MultiOutClickLeftMusicRight,
            setlist: SetlistManifest {
                id: "sunday-am".into(),
                name: "Sunday Morning".into(),
                sections: vec![SetlistSection {
                    id: "sec-worship".into(),
                    name: "Worship".into(),
                    kind: ServiceSectionKind::Worship,
                    color: Some("#6366f1".into()),
                    collapsed: false,
                    note: None,
                }],
                items: vec![SetlistItem {
                    song_id: "pad-prayer".into(),
                    song_dir: None,
                    title: Some("Prayer Pad".into()),
                    original_key: Some("F".into()),
                    base_bpm: Some(70.0),
                    base_click_mode: Some(ClickMode::Off),
                    key_shift_semitones: 2,
                    bpm_override: Some(74.0),
                    click_mode: Some(ClickMode::Off),
                    transition_mode: TransitionMode::AutoAdvance,
                    crossfade_sec: 3.0,
                    ambient_pad_enabled: true,
                    pad_key_override: Some("G".into()),
                    pad_lock: None,
                    section_id: Some("sec-worship".into()),
                    slot_kind: SetlistSlotKind::PadOnly,
                    time_signature: Some([4, 4]),
                    percussion_lock: None,
                    stem_mix: Vec::new(),
                }],
                mixer: None,
            },
            active_setlist_slot: Some(0),
            master_gain: 0.9,
            backing_gain: 0.82,
            click_gain: 0.6,
            click_muted: true,
            pad_gain: 0.55,
            pad_key: "G".into(),
            pad_follow_song_key: false,
            percussion_mode: PercussionMode::Layer,
            percussion_style: PercussionStyle::Afrobeat,
            percussion_channels: vec![PercussionChannelConfig {
                instrument: PercussionInstrument::Shaker,
                enabled: false,
                gain: 0.75,
                pan: -0.35,
            }],
            ..StageSessionState::default()
        };

        save_stage_session_atomic(&path, &session).expect("save session");
        let loaded = load_stage_session(&path).expect("load saved session");
        assert_eq!(loaded, session);
        assert!(!dir.join("stage-session.json.tmp").exists());

        // Malformed JSON should fall back to None rather than crashing app startup.
        fs::write(&path, "{not-valid-json").expect("write corrupt json");
        assert!(load_stage_session(&path).is_none());

        let _ = fs::remove_dir_all(&dir);
    }
}
