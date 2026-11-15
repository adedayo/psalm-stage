use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::pad::{
    PadLayerConfig, PadPatchSpec, PadRegister, PadTonality, PadVoiceArchetype, MAX_PAD_LAYERS,
};
use crate::{AudioCoreError, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadPreset {
    pub id: String,
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub is_factory: bool,
    pub layers: Vec<PadLayerConfig>,
    pub brightness: f32,
    pub shimmer: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub crossfade_sec: Option<f32>,
}

impl PadPreset {
    pub fn to_patch_spec(&self) -> PadPatchSpec {
        PadPatchSpec {
            preset_id: Some(self.id.clone()),
            preset_name: self.name.clone(),
            layers: self.layers.iter().take(MAX_PAD_LAYERS).cloned().collect(),
            brightness: self.brightness.clamp(0.0, 1.0),
            shimmer: self.shimmer.clamp(0.0, 1.0),
            pan: self.pan.clamp(-1.0, 1.0),
            crossfade_sec: self.crossfade_sec,
        }
    }
}

fn layer(
    archetype: PadVoiceArchetype,
    register: PadRegister,
    tonality: PadTonality,
    gain: f32,
) -> PadLayerConfig {
    PadLayerConfig {
        enabled: true,
        archetype,
        register,
        tonality,
        gain,
    }
}

pub fn factory_pad_presets() -> Vec<PadPreset> {
    vec![
        PadPreset {
            id: "classic-foundation".into(),
            name: "Classic Foundation".into(),
            category: "Classic".into(),
            is_factory: true,
            layers: vec![layer(
                PadVoiceArchetype::WarmAnalog,
                PadRegister::Mid,
                PadTonality::Auto,
                0.85,
            )],
            brightness: 0.55,
            shimmer: 0.20,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "classic-bright".into(),
            name: "Classic Bright".into(),
            category: "Classic".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::BrightSynth, PadRegister::Mid, PadTonality::Auto, 0.80),
                layer(PadVoiceArchetype::AiryChoir, PadRegister::High, PadTonality::OpenFifth, 0.40),
            ],
            brightness: 0.82,
            shimmer: 0.45,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "classic-warm".into(),
            name: "Classic Warm".into(),
            category: "Classic".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.65),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Mid, PadTonality::Auto, 0.80),
            ],
            brightness: 0.36,
            shimmer: 0.12,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "classic-dark".into(),
            name: "Classic Dark".into(),
            category: "Classic".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.85),
                layer(PadVoiceArchetype::StringsCello, PadRegister::Low, PadTonality::Auto, 0.55),
            ],
            brightness: 0.18,
            shimmer: 0.05,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "evolving-motion".into(),
            name: "Evolving Motion".into(),
            category: "Motion".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::EvolvingMotion, PadRegister::Mid, PadTonality::Auto, 0.80),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.55),
                layer(PadVoiceArchetype::AiryChoir, PadRegister::High, PadTonality::Melodic, 0.42),
            ],
            brightness: 0.64,
            shimmer: 0.48,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "moody-strings".into(),
            name: "Moody Strings".into(),
            category: "Textural".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::StringsCello, PadRegister::Low, PadTonality::OpenFifth, 0.72),
                layer(PadVoiceArchetype::StringsCello, PadRegister::Mid, PadTonality::Auto, 0.78),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.45),
            ],
            brightness: 0.48,
            shimmer: 0.25,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "warm-organ".into(),
            name: "Warm Organ".into(),
            category: "Textural".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::OrganPad, PadRegister::Mid, PadTonality::OpenFifth, 0.82),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.55),
            ],
            brightness: 0.52,
            shimmer: 0.14,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "guitar-swell".into(),
            name: "Guitar Swell".into(),
            category: "Textural".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::GuitarSwell, PadRegister::Mid, PadTonality::Auto, 0.82),
                layer(PadVoiceArchetype::GuitarSwell, PadRegister::High, PadTonality::Melodic, 0.46),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.48),
            ],
            brightness: 0.70,
            shimmer: 0.62,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "reverse-piano".into(),
            name: "Reverse Piano".into(),
            category: "Motion".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::ReversePiano, PadRegister::Mid, PadTonality::Auto, 0.85),
                layer(PadVoiceArchetype::ReversePiano, PadRegister::High, PadTonality::Melodic, 0.45),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Mid, PadTonality::OpenFifth, 0.45),
            ],
            brightness: 0.68,
            shimmer: 0.52,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "shimmer-cathedral".into(),
            name: "Shimmer Cathedral".into(),
            category: "Textural".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::AiryChoir, PadRegister::High, PadTonality::OpenFifth, 0.68),
                layer(PadVoiceArchetype::BrightSynth, PadRegister::Mid, PadTonality::Auto, 0.72),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.55),
                layer(PadVoiceArchetype::EvolvingMotion, PadRegister::High, PadTonality::Melodic, 0.35),
            ],
            brightness: 0.78,
            shimmer: 0.88,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "deep-sub-drone".into(),
            name: "Deep Sub Drone".into(),
            category: "Classic".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.92),
                layer(PadVoiceArchetype::StringsCello, PadRegister::Low, PadTonality::OpenFifth, 0.50),
            ],
            brightness: 0.24,
            shimmer: 0.08,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "airy-choir".into(),
            name: "Airy Choir".into(),
            category: "Textural".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::AiryChoir, PadRegister::Mid, PadTonality::Auto, 0.78),
                layer(PadVoiceArchetype::AiryChoir, PadRegister::High, PadTonality::OpenFifth, 0.52),
                layer(PadVoiceArchetype::EvolvingMotion, PadRegister::Mid, PadTonality::Auto, 0.40),
            ],
            brightness: 0.72,
            shimmer: 0.58,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "melodic-horizon".into(),
            name: "Melodic Horizon".into(),
            category: "Motion".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::EvolvingMotion, PadRegister::Mid, PadTonality::Melodic, 0.78),
                layer(PadVoiceArchetype::ReversePiano, PadRegister::High, PadTonality::Melodic, 0.52),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.58),
            ],
            brightness: 0.66,
            shimmer: 0.54,
            pan: 0.0,
            crossfade_sec: None,
        },
        PadPreset {
            id: "minor-lament".into(),
            name: "Minor Lament".into(),
            category: "Minor".into(),
            is_factory: true,
            layers: vec![
                layer(PadVoiceArchetype::StringsCello, PadRegister::Mid, PadTonality::Minor, 0.80),
                layer(PadVoiceArchetype::WarmAnalog, PadRegister::Low, PadTonality::OpenFifth, 0.65),
                layer(PadVoiceArchetype::AiryChoir, PadRegister::High, PadTonality::Minor, 0.45),
            ],
            brightness: 0.44,
            shimmer: 0.36,
            pan: 0.0,
            crossfade_sec: None,
        },
    ]
}

fn user_presets_file(config_dir: &Path) -> PathBuf {
    config_dir.join("pad-presets.json")
}

fn sanitize_preset_slug(raw: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in raw.trim().to_ascii_lowercase().chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

pub fn load_user_pad_presets(config_dir: &Path) -> Vec<PadPreset> {
    let path = user_presets_file(config_dir);
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(mut presets) = serde_json::from_str::<Vec<PadPreset>>(&raw) else {
        return Vec::new();
    };
    for p in &mut presets {
        p.is_factory = false;
        p.layers.truncate(MAX_PAD_LAYERS);
    }
    presets.sort_by(|a, b| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()));
    presets
}

pub fn list_all_pad_presets(config_dir: &Path) -> Vec<PadPreset> {
    let mut all = factory_pad_presets();
    all.extend(load_user_pad_presets(config_dir));
    all
}

pub fn save_user_pad_preset(config_dir: &Path, mut preset: PadPreset) -> Result<Vec<PadPreset>> {
    let name = preset.name.trim().to_string();
    if name.is_empty() {
        return Err(AudioCoreError::Manifest(
            "Preset name cannot be empty".into(),
        ));
    }

    let factory_ids: HashSet<String> = factory_pad_presets()
        .into_iter()
        .map(|p| p.id)
        .collect();

    let mut id = preset.id.trim().to_string();
    if id.is_empty() || factory_ids.contains(&id) {
        let base = sanitize_preset_slug(&name);
        let suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() % 100_000)
            .unwrap_or(1);
        id = if base.is_empty() {
            format!("user-pad-{suffix}")
        } else {
            format!("user-{base}-{suffix}")
        };
    }

    let layers: Vec<PadLayerConfig> = preset
        .layers
        .into_iter()
        .take(MAX_PAD_LAYERS)
        .map(|l| l.clamped())
        .collect();
    if layers.is_empty() || !layers.iter().any(|l| l.enabled) {
        return Err(AudioCoreError::Manifest(
            "Preset must include at least one enabled layer".into(),
        ));
    }

    preset.id = id;
    preset.name = name;
    preset.category = if preset.category.trim().is_empty() {
        "Custom".to_string()
    } else {
        preset.category.trim().to_string()
    };
    preset.is_factory = false;
    preset.layers = layers;
    preset.brightness = preset.brightness.clamp(0.0, 1.0);
    preset.shimmer = preset.shimmer.clamp(0.0, 1.0);
    preset.pan = preset.pan.clamp(-1.0, 1.0);
    preset.crossfade_sec = preset.crossfade_sec.map(|c| c.clamp(0.5, 12.0));

    let mut existing = load_user_pad_presets(config_dir);
    if let Some(slot) = existing.iter_mut().find(|p| p.id == preset.id) {
        *slot = preset;
    } else {
        existing.push(preset);
    }

    std::fs::create_dir_all(config_dir)?;
    let json = serde_json::to_string_pretty(&existing)
        .map_err(|e| AudioCoreError::Manifest(e.to_string()))?;
    std::fs::write(user_presets_file(config_dir), json)?;

    Ok(list_all_pad_presets(config_dir))
}

pub fn delete_user_pad_preset(config_dir: &Path, id: &str) -> Result<Vec<PadPreset>> {
    if factory_pad_presets().iter().any(|p| p.id == id) {
        return Err(AudioCoreError::Manifest(
            "Cannot delete a built-in factory pad preset".into(),
        ));
    }
    let mut existing = load_user_pad_presets(config_dir);
    let before = existing.len();
    existing.retain(|p| p.id != id);
    if existing.len() != before {
        std::fs::create_dir_all(config_dir)?;
        let json = serde_json::to_string_pretty(&existing)
            .map_err(|e| AudioCoreError::Manifest(e.to_string()))?;
        std::fs::write(user_presets_file(config_dir), json)?;
    }
    Ok(list_all_pad_presets(config_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_presets_are_valid_and_unique() {
        let presets = factory_pad_presets();
        assert!(presets.len() >= 14);
        let mut ids = HashSet::new();
        for preset in presets {
            assert!(preset.is_factory);
            assert!(ids.insert(preset.id.clone()), "Duplicate preset id");
            assert!(!preset.layers.is_empty());
            assert!(preset.layers.len() <= MAX_PAD_LAYERS);
            assert!(preset.layers.iter().any(|l| l.enabled));
            assert!((0.0..=1.0).contains(&preset.brightness));
            assert!((0.0..=1.0).contains(&preset.shimmer));
        }
    }

    #[test]
    fn test_user_pad_preset_persistence_and_factory_guard() {
        let dir = std::env::temp_dir().join(format!(
            "psalm_stage_pad_presets_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let initial = list_all_pad_presets(&dir);
        let factory_count = initial.len();

        let saved = save_user_pad_preset(
            &dir,
            PadPreset {
                id: String::new(),
                name: "Sunday Morning Warmth".into(),
                category: "Custom".into(),
                is_factory: false,
                layers: vec![layer(
                    PadVoiceArchetype::GuitarSwell,
                    PadRegister::Mid,
                    PadTonality::Auto,
                    0.8,
                )],
                brightness: 0.62,
                shimmer: 0.45,
                pan: 0.1,
                crossfade_sec: Some(6.0),
            },
        )
        .unwrap();

        assert_eq!(saved.len(), factory_count + 1);
        let created = saved.iter().find(|p| p.name == "Sunday Morning Warmth").unwrap();
        assert!(!created.is_factory);
        let created_id = created.id.clone();

        // Factory deletion is rejected
        assert!(delete_user_pad_preset(&dir, "classic-foundation").is_err());

        // User preset deletion succeeds
        let after_del = delete_user_pad_preset(&dir, &created_id).unwrap();
        assert_eq!(after_del.len(), factory_count);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
