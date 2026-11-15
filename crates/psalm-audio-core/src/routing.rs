use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

use crate::{AudioCoreError, Result};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioDeviceInfo {
    pub name: String,
    pub max_channels: u16,
    pub default_sample_rate: u32,
    pub is_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputRoutingMode {
    /// 2-output interface mode (e.g., Focusrite Scarlett 2i2 / Solo):
    /// Channel 1 (Left) = Mono Musical Stems, Channel 2 (Right) = Click + Cues.
    SplitLeftRight,
    /// Reverse split if preferred: Left = Click + Cues, Right = Music.
    SplitClickLeftMusicRight,
    /// 4+ output interface mode:
    /// Channels 1-2 = Stereo Musical Stems, Channels 3-4 = Stereo Click + Cues.
    MultiOutStereo,
    /// 4+ output interface mode (swapped):
    /// Channels 1-2 = Stereo Click + Cues, Channels 3-4 = Stereo Musical Stems.
    MultiOutClickLeftMusicRight,
}

impl OutputRoutingMode {
    pub fn is_multichannel(self) -> bool {
        matches!(self, Self::MultiOutStereo | Self::MultiOutClickLeftMusicRight)
    }

    pub fn is_click_first(self) -> bool {
        matches!(
            self,
            Self::SplitClickLeftMusicRight | Self::MultiOutClickLeftMusicRight
        )
    }

    pub fn from_layout_and_side(multichannel: bool, click_first: bool) -> Self {
        match (multichannel, click_first) {
            (false, false) => Self::SplitLeftRight,
            (false, true) => Self::SplitClickLeftMusicRight,
            (true, false) => Self::MultiOutStereo,
            (true, true) => Self::MultiOutClickLeftMusicRight,
        }
    }
}

impl Default for OutputRoutingMode {
    fn default() -> Self {
        Self::SplitLeftRight
    }
}

pub fn list_output_devices() -> Result<Vec<AudioDeviceInfo>> {
    let host = cpal::default_host();
    let default_name = host
        .default_output_device()
        .and_then(|d| d.name().ok());

    let devices = host
        .output_devices()
        .map_err(|e| AudioCoreError::Device(e.to_string()))?;

    let mut result = Vec::new();
    for dev in devices {
        let Ok(name) = dev.name() else { continue };
        let Ok(config) = dev.default_output_config() else {
            continue;
        };
        let supported_max_channels = dev
            .supported_output_configs()
            .ok()
            .and_then(|configs| configs.map(|c| c.channels()).max())
            .unwrap_or(0);
        result.push(AudioDeviceInfo {
            is_default: default_name.as_deref() == Some(name.as_str()),
            name,
            max_channels: config.channels().max(supported_max_channels),
            default_sample_rate: config.sample_rate().0,
        });
    }
    Ok(result)
}

pub fn find_multichannel_output_config(
    device: &cpal::Device,
    default_cfg: &cpal::SupportedStreamConfig,
) -> Option<cpal::SupportedStreamConfig> {
    let target_sr = default_cfg.sample_rate();
    let best = device.supported_output_configs().ok()?.filter(|c| c.channels() >= 4).min_by_key(|c| {
        let format_rank = if c.sample_format() == cpal::SampleFormat::F32 {
            0
        } else {
            1
        };
        let sr_rank = if c.min_sample_rate() <= target_sr && target_sr <= c.max_sample_rate() {
            0
        } else {
            1
        };
        (format_rank, sr_rank, c.channels())
    })?;

    if target_sr >= best.min_sample_rate() && target_sr <= best.max_sample_rate() {
        Some(best.with_sample_rate(target_sr))
    } else if target_sr < best.min_sample_rate() {
        Some(best.with_sample_rate(best.min_sample_rate()))
    } else {
        Some(best.with_max_sample_rate())
    }
}

pub fn find_output_device(preferred_name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal::default_host();
    if let Some(target) = preferred_name {
        if let Ok(mut devs) = host.output_devices() {
            if let Some(found) = devs.find(|d| d.name().ok().as_deref() == Some(target)) {
                return Ok(found);
            }
        }
    }
    host.default_output_device()
        .ok_or_else(|| AudioCoreError::Device("No default audio output device found".into()))
}

/// Returns output devices present in `current` whose names were not present in `previous`.
pub fn detect_newly_connected_outputs(
    previous: &[AudioDeviceInfo],
    current: &[AudioDeviceInfo],
) -> Vec<AudioDeviceInfo> {
    current
        .iter()
        .filter(|dev| !previous.iter().any(|prev| prev.name == dev.name))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_routing_mode() {
        assert_eq!(OutputRoutingMode::default(), OutputRoutingMode::SplitLeftRight);
    }

    #[test]
    fn test_routing_mode_serde() {
        let modes = [
            OutputRoutingMode::SplitLeftRight,
            OutputRoutingMode::SplitClickLeftMusicRight,
            OutputRoutingMode::MultiOutStereo,
            OutputRoutingMode::MultiOutClickLeftMusicRight,
        ];
        for mode in modes {
            let json = serde_json::to_string(&mode).expect("Failed to serialize routing mode");
            let deserialized: OutputRoutingMode =
                serde_json::from_str(&json).expect("Failed to deserialize routing mode");
            assert_eq!(mode, deserialized);
        }
    }

    #[test]
    fn test_routing_mode_layout_and_side_helpers() {
        assert!(!OutputRoutingMode::SplitLeftRight.is_multichannel());
        assert!(!OutputRoutingMode::SplitLeftRight.is_click_first());
        assert!(!OutputRoutingMode::SplitClickLeftMusicRight.is_multichannel());
        assert!(OutputRoutingMode::SplitClickLeftMusicRight.is_click_first());
        assert!(OutputRoutingMode::MultiOutStereo.is_multichannel());
        assert!(!OutputRoutingMode::MultiOutStereo.is_click_first());
        assert!(OutputRoutingMode::MultiOutClickLeftMusicRight.is_multichannel());
        assert!(OutputRoutingMode::MultiOutClickLeftMusicRight.is_click_first());

        for multichannel in [false, true] {
            for click_first in [false, true] {
                let mode = OutputRoutingMode::from_layout_and_side(multichannel, click_first);
                assert_eq!(mode.is_multichannel(), multichannel);
                assert_eq!(mode.is_click_first(), click_first);
            }
        }
    }

    #[test]
    fn test_audio_device_info_serde() {
        let info = AudioDeviceInfo {
            name: "Focusrite Scarlett 2i2 USB".into(),
            max_channels: 2,
            default_sample_rate: 48000,
            is_default: true,
        };
        let json = serde_json::to_string(&info).expect("serialize AudioDeviceInfo");
        let parsed: AudioDeviceInfo = serde_json::from_str(&json).expect("deserialize AudioDeviceInfo");
        assert_eq!(parsed.name, info.name);
        assert_eq!(parsed.max_channels, 2);
        assert_eq!(parsed.default_sample_rate, 48000);
        assert!(parsed.is_default);
    }

    #[test]
    fn test_detect_newly_connected_outputs_finds_hotplugged_interface() {
        let builtin = AudioDeviceInfo {
            name: "MacBook Pro Speakers".into(),
            max_channels: 2,
            default_sample_rate: 48000,
            is_default: true,
        };
        let focusrite = AudioDeviceInfo {
            name: "Scarlett 2i2 USB".into(),
            max_channels: 2,
            default_sample_rate: 48000,
            is_default: false,
        };

        let newly_added = detect_newly_connected_outputs(
            std::slice::from_ref(&builtin),
            &[builtin.clone(), focusrite.clone()],
        );
        assert_eq!(newly_added, vec![focusrite]);

        let unchanged = detect_newly_connected_outputs(
            std::slice::from_ref(&builtin),
            std::slice::from_ref(&builtin),
        );
        assert!(unchanged.is_empty());
    }
}
