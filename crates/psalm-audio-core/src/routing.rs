use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

use crate::{AudioCoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Channels 1-2 = Stereo Musical Stems, Channels 3-4 = Click + Cues.
    MultiOutStereo,
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
        result.push(AudioDeviceInfo {
            is_default: default_name.as_deref() == Some(name.as_str()),
            name,
            max_channels: config.channels(),
            default_sample_rate: config.sample_rate().0,
        });
    }
    Ok(result)
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
