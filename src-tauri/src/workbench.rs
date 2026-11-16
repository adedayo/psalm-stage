use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

pub use psalm_workbench::{
    analyze_waveform_with_timing, generate_section_cues_for_song,
    save_managed_setlist as core_save_managed_setlist, GeneratedSectionCues, ImportJobRequest,
    ProgressSink, SectionCueOptions, SectionCueTimingOverride, ToolchainStatus, WorkbenchConfig,
    WorkbenchJobProgress, WorkbenchManager, YouTubeMetadata, YouTubeSearchCandidate,
    BUNDLED_SKILL_MD,
};

pub struct TauriProgressSink {
    app: AppHandle,
}

impl TauriProgressSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl ProgressSink for TauriProgressSink {
    fn emit(&self, progress: WorkbenchJobProgress) {
        let _ = self.app.emit("workbench:job-progress", &progress);
    }
}

pub fn start_tauri_import_job(
    manager: &WorkbenchManager,
    req: ImportJobRequest,
    app: AppHandle,
) -> Result<String, String> {
    let sink = Arc::new(TauriProgressSink::new(app));
    manager.start_import_job_with_sink(req, sink)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiHarnessInfo {
    pub cli_path: String,
    pub cli_available: bool,
    pub mcp_config_json: String,
    pub skill_markdown: String,
    pub symlink_supported: bool,
    pub symlink_path: Option<String>,
    pub symlink_installed: bool,
}

pub fn resolve_bundled_cli_path() -> (String, bool) {
    let exe_name = if cfg!(target_os = "windows") {
        "psalm-stage-cli.exe"
    } else {
        "psalm-stage-cli"
    };

    if let Ok(cur_exe) = std::env::current_exe() {
        if let Some(dir) = cur_exe.parent() {
            let direct = dir.join(exe_name);
            if is_valid_executable(&direct) {
                return (direct.to_string_lossy().to_string(), true);
            }
            if let Ok(rd) = std::fs::read_dir(dir) {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with("psalm-stage-cli") && is_valid_executable(&p) {
                            return (p.to_string_lossy().to_string(), true);
                        }
                    }
                }
            }
        }
    }

    if let Some(path_var) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path_var) {
            let cand = dir.join(exe_name);
            if is_valid_executable(&cand) {
                return (cand.to_string_lossy().to_string(), true);
            }
        }
    }

    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        let local_bin = home.join(".local/bin").join(exe_name);
        if is_valid_executable(&local_bin) {
            return (local_bin.to_string_lossy().to_string(), true);
        }
    }

    (exe_name.to_string(), false)
}

fn is_valid_executable(path: &Path) -> bool {
    path.exists()
        && path.is_file()
        && std::fs::metadata(path).is_ok_and(|m| m.len() > 0)
}

pub fn build_ai_harness_info() -> AiHarnessInfo {
    let (cli_path, cli_available) = resolve_bundled_cli_path();
    let mcp_config = serde_json::json!({
        "mcpServers": {
            "psalm-stage": {
                "command": cli_path,
                "args": ["mcp"]
            }
        }
    });
    let mcp_config_json = serde_json::to_string_pretty(&mcp_config).unwrap_or_default();

    let (symlink_supported, symlink_path, symlink_installed) = if cfg!(unix) {
        let link = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| h.join(".local/bin/psalm-stage-cli"));
        let installed = link.as_ref().is_some_and(|p| p.exists());
        (
            true,
            link.map(|p| p.to_string_lossy().to_string()),
            installed,
        )
    } else {
        (false, None, false)
    };

    AiHarnessInfo {
        cli_path,
        cli_available,
        mcp_config_json,
        skill_markdown: BUNDLED_SKILL_MD.to_string(),
        symlink_supported,
        symlink_path,
        symlink_installed,
    }
}

pub fn install_cli_symlink() -> Result<AiHarnessInfo, String> {
    #[cfg(unix)]
    {
        let (cli_path, cli_available) = resolve_bundled_cli_path();
        if !cli_available {
            return Err(
                "Bundled psalm-stage-cli binary could not be located next to the application."
                    .to_string(),
            );
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME directory is not set".to_string())?;
        let bin_dir = home.join(".local/bin");
        std::fs::create_dir_all(&bin_dir).map_err(|e| e.to_string())?;
        let target_link = bin_dir.join("psalm-stage-cli");
        if target_link.exists() || target_link.symlink_metadata().is_ok() {
            let _ = std::fs::remove_file(&target_link);
        }
        std::os::unix::fs::symlink(Path::new(&cli_path), &target_link)
            .map_err(|e| format!("Failed to create symlink at {}: {e}", target_link.display()))?;
        Ok(build_ai_harness_info())
    }
    #[cfg(not(unix))]
    {
        Err("CLI symlink installation is only supported on macOS and Linux.".to_string())
    }
}

/// Computes a fast fingerprint of Library & Setlists directories so the UI refreshes
/// automatically whenever an external AI harness writes new songs or setlists.
pub fn compute_workspace_watch_fingerprint(manager: &WorkbenchManager) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let cfg = manager.get_library_config();
    let mut hasher = DefaultHasher::new();

    let mut dirs = vec![PathBuf::from(&cfg.library_root), PathBuf::from(&cfg.setlists_root)];
    for extra in &cfg.linked_folders {
        dirs.push(PathBuf::from(extra));
    }

    for dir in dirs {
        dir.to_string_lossy().hash(&mut hasher);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            let mut entries: Vec<(String, u64)> = rd
                .flatten()
                .map(|entry| {
                    let p = entry.path();
                    let name = entry.file_name().to_string_lossy().to_string();
                    let manifest_mtime = if p.is_dir() {
                        std::fs::metadata(p.join("manifest.json"))
                            .and_then(|m| m.modified())
                            .ok()
                    } else {
                        std::fs::metadata(&p).and_then(|m| m.modified()).ok()
                    };
                    let ms = manifest_mtime
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_millis() as u64)
                        .unwrap_or(0);
                    (name, ms)
                })
                .collect();
            entries.sort();
            entries.hash(&mut hasher);
        }
    }

    hasher.finish()
}
