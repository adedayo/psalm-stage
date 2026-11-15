use std::path::{Path, PathBuf};

use psalm_audio_core::library::{normalize_library_config, LibraryConfig};
use serde::{Deserialize, Serialize};

pub const APP_IDENTIFIER: &str = "org.psalmstage.app";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum SeparationEngine {
    #[default]
    None,
    AudioSeparator,
    Demucs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StemPreset {
    TwoStems,
    #[default]
    FourStems,
    SixStems,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchConfig {
    #[serde(default)]
    pub yt_dlp_path: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub audio_separator_path: Option<String>,
    #[serde(default)]
    pub demucs_path: Option<String>,
    #[serde(default)]
    pub default_separation_engine: SeparationEngine,
    #[serde(default)]
    pub default_stem_preset: StemPreset,
    #[serde(default = "default_uvr_model")]
    pub custom_uvr_model: String,
}

fn default_uvr_model() -> String {
    "htdemucs_ft.yaml".to_string()
}

impl Default for WorkbenchConfig {
    fn default() -> Self {
        Self {
            yt_dlp_path: None,
            ffmpeg_path: None,
            audio_separator_path: None,
            demucs_path: None,
            default_separation_engine: SeparationEngine::None,
            default_stem_preset: StemPreset::FourStems,
            custom_uvr_model: default_uvr_model(),
        }
    }
}

/// Resolves the shared application configuration directory matching Tauri's
/// `app.path().app_config_dir()` for `org.psalmstage.app`, with optional
/// `PSALM_STAGE_CONFIG_DIR` override for isolated CLI/test invocations.
pub fn resolve_app_config_dir() -> PathBuf {
    if let Some(override_dir) = std::env::var_os("PSALM_STAGE_CONFIG_DIR") {
        let p = PathBuf::from(override_dir);
        if !p.as_os_str().is_empty() {
            return p;
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            return home
                .join("Library")
                .join("Application Support")
                .join(APP_IDENTIFIER);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
            return app_data.join(APP_IDENTIFIER);
        }
    }

    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from) {
            if !xdg.as_os_str().is_empty() {
                return xdg.join(APP_IDENTIFIER);
            }
        }
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            return home.join(".config").join(APP_IDENTIFIER);
        }
    }

    std::env::temp_dir().join("psalm-stage")
}

pub fn load_library_config(config_dir: &Path) -> LibraryConfig {
    let path = config_dir.join("library-config.json");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(parsed) = serde_json::from_str::<LibraryConfig>(&raw) {
            return normalize_library_config(parsed);
        }
    }
    LibraryConfig::default()
}

pub fn save_library_config(
    config_dir: &Path,
    config: LibraryConfig,
) -> Result<LibraryConfig, String> {
    let normalized = normalize_library_config(config);
    let path = config_dir.join("library-config.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&normalized).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(normalized)
}

pub fn load_workbench_config(config_dir: &Path) -> WorkbenchConfig {
    let path = config_dir.join("workbench-config.json");
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(parsed) = serde_json::from_str::<WorkbenchConfig>(&raw) {
            return parsed;
        }
    }
    WorkbenchConfig::default()
}

pub fn save_workbench_config(
    config_dir: &Path,
    config: WorkbenchConfig,
) -> Result<WorkbenchConfig, String> {
    let path = config_dir.join("workbench-config.json");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(config)
}

