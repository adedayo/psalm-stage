use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use psalm_audio_core::analysis::{
    analyze_audio_file_with_timing, analyze_song_folder_mixdown,
    analyze_song_folder_mixdown_with_timing, AudioAnalysisResult,
};
use psalm_audio_core::library::{
    list_managed_setlists, normalize_separated_stem_role, scan_song_library, LibraryConfig,
    LibraryEntryStatus, LibrarySongEntry, SetlistSummary,
};
use psalm_audio_core::loader::{inspect_song_folder, save_song_manifest};
use psalm_audio_core::manifest::{
    canonicalize_source_ref, ImportProvenance, SectionDef, SongManifest, StemDef,
};
use serde::{Deserialize, Serialize};

use crate::config::{
    load_library_config, load_workbench_config, resolve_app_config_dir, save_library_config,
    save_workbench_config, SeparationEngine, StemPreset, WorkbenchConfig,
};
use crate::youtube::{
    chapters_to_sections, probe_youtube, search_youtube, YouTubeChapter, YouTubeMetadata,
    YouTubeSearchCandidate,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    pub id: String,
    pub name: String,
    pub available: bool,
    pub resolved_path: Option<String>,
    pub version: Option<String>,
    pub install_hint: String,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolchainStatus {
    pub platform: String,
    pub ffmpeg: ToolInfo,
    pub yt_dlp: ToolInfo,
    pub audio_separator: ToolInfo,
    pub demucs: ToolInfo,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value")]
pub enum ImportSource {
    LocalFile(String),
    YouTubeUrl(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportJobRequest {
    pub source: ImportSource,
    pub song_title: Option<String>,
    #[serde(default)]
    pub artist: Option<String>,
    pub target_dir: Option<String>,
    pub separation_engine: SeparationEngine,
    pub stem_preset: StemPreset,
    pub custom_model: Option<String>,
    #[serde(default)]
    pub force_reextract: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkbenchJobProgress {
    pub job_id: String,
    pub status: JobStatus,
    pub stage: String,
    pub progress_pct: f32,
    pub message: String,
    pub log_line: Option<String>,
    pub output_dir: Option<String>,
}

pub trait ProgressSink: Send + Sync {
    fn emit(&self, progress: WorkbenchJobProgress);
}

#[derive(Debug, Default)]
pub struct NoopProgressSink;

impl ProgressSink for NoopProgressSink {
    fn emit(&self, _progress: WorkbenchJobProgress) {}
}

#[derive(Debug, Default)]
pub struct StderrProgressSink;

impl ProgressSink for StderrProgressSink {
    fn emit(&self, progress: WorkbenchJobProgress) {
        eprintln!(
            "[{}] {:>5.1}% | {}: {}",
            progress.job_id, progress.progress_pct, progress.stage, progress.message
        );
    }
}

pub struct CallbackProgressSink<F: Fn(WorkbenchJobProgress) + Send + Sync> {
    callback: F,
}

impl<F: Fn(WorkbenchJobProgress) + Send + Sync> CallbackProgressSink<F> {
    pub fn new(callback: F) -> Self {
        Self { callback }
    }
}

impl<F: Fn(WorkbenchJobProgress) + Send + Sync> ProgressSink for CallbackProgressSink<F> {
    fn emit(&self, progress: WorkbenchJobProgress) {
        (self.callback)(progress);
    }
}

struct ActiveJob {
    _id: String,
    cancelled: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
}

pub struct WorkbenchManager {
    config_dir: PathBuf,
    active_job: Arc<Mutex<Option<ActiveJob>>>,
}

impl WorkbenchManager {
    pub fn new(config_dir: PathBuf) -> Self {
        Self {
            config_dir,
            active_job: Arc::new(Mutex::new(None)),
        }
    }

    pub fn with_default_config() -> Self {
        Self::new(resolve_app_config_dir())
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn get_library_config(&self) -> LibraryConfig {
        load_library_config(&self.config_dir)
    }

    pub fn save_library_config(&self, config: LibraryConfig) -> Result<LibraryConfig, String> {
        save_library_config(&self.config_dir, config)
    }

    pub fn ensure_library_root(&self) -> Result<String, String> {
        let cfg = self.get_library_config();
        let root = PathBuf::from(&cfg.library_root);
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let setlists = PathBuf::from(&cfg.setlists_root);
        std::fs::create_dir_all(&setlists).map_err(|e| e.to_string())?;
        Ok(cfg.library_root)
    }

    pub fn scan_library(&self) -> Vec<LibrarySongEntry> {
        let cfg = self.get_library_config();
        scan_song_library(&cfg)
    }

    pub fn list_setlists(&self) -> Vec<SetlistSummary> {
        let cfg = self.get_library_config();
        list_managed_setlists(&cfg)
    }

    pub fn get_workbench_config(&self) -> WorkbenchConfig {
        load_workbench_config(&self.config_dir)
    }

    pub fn save_workbench_config(
        &self,
        config: WorkbenchConfig,
    ) -> Result<WorkbenchConfig, String> {
        save_workbench_config(&self.config_dir, config)
    }

    pub fn detect_tools(&self) -> ToolchainStatus {
        let cfg = self.get_workbench_config();
        detect_toolchain_status(&cfg)
    }

    pub fn probe_youtube(&self, url: &str) -> Result<YouTubeMetadata, String> {
        let tools = self.detect_tools();
        let yt_dlp = tools.yt_dlp.resolved_path.ok_or_else(|| {
            "yt-dlp was not found. Install yt-dlp or configure its path in Toolchain Settings."
                .to_string()
        })?;
        probe_youtube(&yt_dlp, url)
    }

    pub fn search_youtube(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<YouTubeSearchCandidate>, String> {
        let tools = self.detect_tools();
        let yt_dlp = tools.yt_dlp.resolved_path.ok_or_else(|| {
            "yt-dlp was not found. Install yt-dlp or configure its path in Toolchain Settings."
                .to_string()
        })?;
        search_youtube(&yt_dlp, query, limit)
    }

    pub fn cancel_active_job(&self) -> Result<(), String> {
        if let Ok(guard) = self.active_job.lock() {
            if let Some(job) = guard.as_ref() {
                job.cancelled.store(true, Ordering::SeqCst);
                if let Ok(mut child_guard) = job.child.lock() {
                    if let Some(child) = child_guard.as_mut() {
                        let _ = child.kill();
                    }
                }
            }
        }
        Ok(())
    }

    pub fn start_import_job_with_sink(
        &self,
        req: ImportJobRequest,
        sink: Arc<dyn ProgressSink>,
    ) -> Result<String, String> {
        {
            let guard = self.active_job.lock().map_err(|e| e.to_string())?;
            if guard.is_some() {
                return Err("An import or stem separation job is already running.".to_string());
            }
        }

        let job_id = format!(
            "job-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let cancelled = Arc::new(AtomicBool::new(false));
        let child_holder: Arc<Mutex<Option<Child>>> = Arc::new(Mutex::new(None));

        {
            let mut guard = self.active_job.lock().map_err(|e| e.to_string())?;
            *guard = Some(ActiveJob {
                _id: job_id.clone(),
                cancelled: Arc::clone(&cancelled),
                child: Arc::clone(&child_holder),
            });
        }

        let lib_cfg = self.get_library_config();
        let wb_cfg = self.get_workbench_config();
        let active_job_ref = Arc::clone(&self.active_job);
        let returned_job_id = job_id.clone();

        thread::Builder::new()
            .name("psalm-workbench-job".into())
            .spawn(move || {
                let res = run_import_job_pipeline(
                    &job_id,
                    req,
                    &lib_cfg,
                    &wb_cfg,
                    &cancelled,
                    &child_holder,
                    sink.as_ref(),
                );
                if let Err(err_msg) = res {
                    let status = if cancelled.load(Ordering::SeqCst) {
                        JobStatus::Cancelled
                    } else {
                        JobStatus::Failed
                    };
                    emit_progress(
                        sink.as_ref(),
                        &job_id,
                        status,
                        "Stopped",
                        0.0,
                        &err_msg,
                        Some(&err_msg),
                        None,
                    );
                }
                if let Ok(mut guard) = active_job_ref.lock() {
                    *guard = None;
                }
            })
            .map_err(|e| format!("Failed to spawn workbench worker thread: {e}"))?;

        Ok(returned_job_id)
    }
}

/// Resolves the effective stem separation engine for automated AI workflows:
/// - If an explicit engine is provided, returns it directly.
/// - Otherwise uses `wb_cfg.default_separation_engine`, gracefully falling back to
///   `SeparationEngine::None` with a warning if that ML tool is not installed.
pub fn resolve_effective_separation(
    explicit_engine: Option<SeparationEngine>,
    explicit_preset: Option<StemPreset>,
    wb_cfg: &WorkbenchConfig,
    tools: &ToolchainStatus,
) -> (SeparationEngine, StemPreset, Option<String>) {
    let preset = explicit_preset.unwrap_or(wb_cfg.default_stem_preset);
    if let Some(engine) = explicit_engine {
        return (engine, preset, None);
    }

    match wb_cfg.default_separation_engine {
        SeparationEngine::None => (SeparationEngine::None, preset, None),
        SeparationEngine::AudioSeparator => {
            if tools.audio_separator.available {
                (SeparationEngine::AudioSeparator, preset, None)
            } else {
                (
                    SeparationEngine::None,
                    preset,
                    Some(format!(
                        "Default stem separator UVR (audio-separator) not found — imported stereo backing track instead. Install with: {}",
                        tools.audio_separator.install_hint
                    )),
                )
            }
        }
        SeparationEngine::Demucs => {
            if tools.demucs.available {
                (SeparationEngine::Demucs, preset, None)
            } else {
                (
                    SeparationEngine::None,
                    preset,
                    Some(format!(
                        "Default stem separator Demucs not found — imported stereo backing track instead. Install with: {}",
                        tools.demucs.install_hint
                    )),
                )
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchIngestRequest {
    pub jobs: Vec<ImportJobRequest>,
    #[serde(default)]
    pub use_default_with_fallback: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchIngestItemResult {
    pub index: usize,
    pub song_title: String,
    pub output_dir: Option<String>,
    pub from_cache: bool,
    pub separation_engine: SeparationEngine,
    pub original_key: Option<String>,
    pub bpm: Option<f64>,
    pub key_confidence: Option<f32>,
    pub needs_review: bool,
    pub warnings: Vec<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchIngestResult {
    pub completed: usize,
    pub failed: usize,
    pub items: Vec<BatchIngestItemResult>,
}

/// Runs a batch of import jobs sequentially (reusing cached Library bundles and
/// falling back cleanly when ML separators are absent if `use_default_with_fallback` is true).
pub fn ingest_songs_batch(
    req: BatchIngestRequest,
    lib_cfg: &LibraryConfig,
    wb_cfg: &WorkbenchConfig,
    sink: &dyn ProgressSink,
) -> BatchIngestResult {
    let tools = detect_toolchain_status(wb_cfg);
    let cancelled = AtomicBool::new(false);
    let child_holder = Mutex::new(None);
    let mut results = Vec::with_capacity(req.jobs.len());
    let mut completed = 0usize;
    let mut failed = 0usize;

    for (idx, mut job) in req.jobs.into_iter().enumerate() {
        let mut warnings = Vec::new();
        if req.use_default_with_fallback {
            let (eff_engine, eff_preset, warn) =
                resolve_effective_separation(None, Some(job.stem_preset), wb_cfg, &tools);
            job.separation_engine = eff_engine;
            job.stem_preset = eff_preset;
            if let Some(w) = warn {
                warnings.push(w);
            }
        }

        let display_title = job
            .song_title
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("Song {}", idx + 1));

        let (source_kind, raw_source) = match &job.source {
            ImportSource::LocalFile(p) => ("local_file", p.trim()),
            ImportSource::YouTubeUrl(u) => ("youtube", u.trim()),
        };
        let source_ref = canonicalize_source_ref(source_kind, raw_source);
        let predicted_dir = job
            .target_dir
            .as_ref()
            .filter(|d| !d.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                PathBuf::from(&lib_cfg.library_root).join(sanitize_folder_name(&display_title))
            });
        let was_cached = !job.force_reextract
            && find_cached_library_bundle(&source_ref, &predicted_dir, lib_cfg).is_some();

        let job_id = format!("batch-{}", idx + 1);
        let engine_used = job.separation_engine;

        match run_import_job_pipeline(
            &job_id,
            job,
            lib_cfg,
            wb_cfg,
            &cancelled,
            &child_holder,
            sink,
        ) {
            Ok(out_dir) => {
                completed += 1;
                let manifest =
                    SongManifest::from_file(&PathBuf::from(&out_dir).join("manifest.json")).ok();
                let key_confidence = manifest
                    .as_ref()
                    .and_then(|m| m.provenance.as_ref())
                    .and_then(|p| p.key_confidence);
                let needs_review = manifest
                    .as_ref()
                    .and_then(|m| m.provenance.as_ref())
                    .is_some_and(|p| p.needs_review);
                if let Some(ref m) = manifest {
                    if let Some(ref p) = m.provenance {
                        warnings.extend(p.review_notes.clone());
                    }
                }
                results.push(BatchIngestItemResult {
                    index: idx,
                    song_title: manifest
                        .as_ref()
                        .map(|m| m.title.clone())
                        .unwrap_or(display_title),
                    output_dir: Some(out_dir),
                    from_cache: was_cached,
                    separation_engine: engine_used,
                    original_key: manifest.as_ref().map(|m| m.original_key.clone()),
                    bpm: manifest.as_ref().map(|m| m.bpm),
                    key_confidence,
                    needs_review,
                    warnings,
                    error: None,
                });
            }
            Err(err) => {
                failed += 1;
                results.push(BatchIngestItemResult {
                    index: idx,
                    song_title: display_title,
                    output_dir: None,
                    from_cache: false,
                    separation_engine: engine_used,
                    original_key: None,
                    bpm: None,
                    key_confidence: None,
                    needs_review: true,
                    warnings,
                    error: Some(err),
                });
            }
        }
    }

    BatchIngestResult {
        completed,
        failed,
        items: results,
    }
}

const ANALYSIS_CACHE_FILE: &str = ".psalm-analysis-cache.json";
const ANALYSIS_CACHE_VERSION: u32 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StemFileStamp {
    file: String,
    size_bytes: u64,
    modified_ms: u64,
    pitched: bool,
    is_cues: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedAnalysisRecord {
    version: u32,
    num_buckets: usize,
    beats_per_bar: u8,
    stems: Vec<StemFileStamp>,
    analysis: AudioAnalysisResult,
}

fn build_analysis_stamps(dir: &Path, manifest: Option<&SongManifest>) -> Vec<StemFileStamp> {
    let mut role_map: HashMap<String, (bool, bool)> = HashMap::new();
    if let Some(m) = manifest {
        for (k, def) in &m.stems {
            role_map.insert(def.file_name().to_string(), (def.is_pitched(k), def.is_cues(k)));
        }
    }

    let mut stamps = Vec::new();
    if let Ok(files) = psalm_audio_core::loader::discover_audio_files(dir) {
        for fname in files {
            let meta = std::fs::metadata(dir.join(&fname)).ok();
            let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            let modified_ms = meta
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            let (pitched, is_cues) = role_map.get(&fname).copied().unwrap_or((true, false));
            stamps.push(StemFileStamp {
                file: fname,
                size_bytes,
                modified_ms,
                pitched,
                is_cues,
            });
        }
    }
    stamps.sort_by(|a, b| a.file.cmp(&b.file));
    stamps
}

fn read_analysis_cache(
    dir: &Path,
    manifest: Option<&SongManifest>,
    num_buckets: usize,
    beats_per_bar: u8,
) -> Option<AudioAnalysisResult> {
    let cache_path = dir.join(ANALYSIS_CACHE_FILE);
    let raw = std::fs::read_to_string(cache_path).ok()?;
    let cached: CachedAnalysisRecord = serde_json::from_str(&raw).ok()?;
    if cached.version != ANALYSIS_CACHE_VERSION
        || cached.num_buckets != num_buckets
        || cached.beats_per_bar != beats_per_bar
    {
        return None;
    }
    let expected_stamps = build_analysis_stamps(dir, manifest);
    if cached.stems.is_empty() || cached.stems != expected_stamps {
        return None;
    }
    Some(cached.analysis)
}

fn write_analysis_cache(
    dir: &Path,
    manifest: Option<&SongManifest>,
    num_buckets: usize,
    beats_per_bar: u8,
    analysis: &AudioAnalysisResult,
) {
    let stems = build_analysis_stamps(dir, manifest);
    if stems.is_empty() {
        return;
    }
    let record = CachedAnalysisRecord {
        version: ANALYSIS_CACHE_VERSION,
        num_buckets,
        beats_per_bar,
        stems,
        analysis: analysis.clone(),
    };
    if let Ok(json) = serde_json::to_string(&record) {
        let _ = std::fs::write(dir.join(ANALYSIS_CACHE_FILE), json);
    }
}

pub fn analyze_waveform_with_timing(
    path_or_dir: &Path,
    is_dir: bool,
    num_buckets: usize,
    hint_bpm: Option<f64>,
    beats_per_bar: Option<u8>,
    force_refresh: bool,
) -> Result<AudioAnalysisResult, String> {
    if is_dir {
        let manifest_path = path_or_dir.join("manifest.json");
        let manifest = if manifest_path.exists() {
            psalm_audio_core::manifest::SongManifest::from_file(&manifest_path).ok()
        } else {
            None
        };
        let bpb = beats_per_bar
            .unwrap_or_else(|| manifest.as_ref().map(|m| m.time_signature[0]).unwrap_or(4));
        if hint_bpm.is_none() && !force_refresh {
            if let Some(cached) =
                read_analysis_cache(path_or_dir, manifest.as_ref(), num_buckets, bpb)
            {
                return Ok(cached);
            }
        }
        let result = analyze_song_folder_mixdown_with_timing(
            path_or_dir,
            manifest.as_ref(),
            num_buckets,
            hint_bpm,
            bpb,
        )
        .map_err(|e| e.to_string())?;
        if hint_bpm.is_none() {
            write_analysis_cache(path_or_dir, manifest.as_ref(), num_buckets, bpb, &result);
        }
        Ok(result)
    } else {
        analyze_audio_file_with_timing(
            path_or_dir,
            num_buckets,
            hint_bpm,
            beats_per_bar.unwrap_or(4),
        )
        .map_err(|e| e.to_string())
    }
}

pub fn detect_toolchain_status(cfg: &WorkbenchConfig) -> ToolchainStatus {
    let os = std::env::consts::OS;
    let (ffmpeg_hint, ytdlp_hint, uvr_hint, demucs_hint) = match os {
        "windows" => (
            "winget install Gyan.FFmpeg",
            "winget install yt-dlp.yt-dlp",
            "pipx install \"audio-separator[cpu]\"",
            "pipx install demucs",
        ),
        "linux" => (
            "sudo apt install -y ffmpeg",
            "pipx install yt-dlp",
            "pipx install \"audio-separator[cpu]\"",
            "pipx install demucs",
        ),
        _ => (
            "brew install ffmpeg",
            "brew install yt-dlp",
            "pipx install \"audio-separator[cpu]\"",
            "pipx install demucs",
        ),
    };

    ToolchainStatus {
        platform: os.to_string(),
        ffmpeg: probe_tool(
            "ffmpeg",
            "FFmpeg",
            cfg.ffmpeg_path.as_deref(),
            &["ffmpeg"],
            "-version",
            ffmpeg_hint,
            "Audio decoding, format conversion, and YouTube audio extraction",
        ),
        yt_dlp: probe_tool(
            "yt_dlp",
            "yt-dlp",
            cfg.yt_dlp_path.as_deref(),
            &["yt-dlp"],
            "--version",
            ytdlp_hint,
            "Downloads and searches audio from YouTube links into the Workbench",
        ),
        audio_separator: probe_tool(
            "audio_separator",
            "UVR (audio-separator)",
            cfg.audio_separator_path.as_deref(),
            &["audio-separator"],
            "--version",
            uvr_hint,
            "Local UVR neural stem separation (MDX-Net, Roformer, Demucs)",
        ),
        demucs: probe_tool(
            "demucs",
            "Demucs",
            cfg.demucs_path.as_deref(),
            &["demucs"],
            "--help",
            demucs_hint,
            "Alternative local 4-stem / 6-stem AI separator",
        ),
    }
}

fn expand_child_bin_dirs(parent: &Path, suffix: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(parent) else {
        return;
    };
    for entry in rd.flatten() {
        let candidate = entry.path().join(suffix);
        if candidate.is_dir() {
            out.push(candidate);
        }
    }
}

fn candidate_search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(path_var) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path_var));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from);
    if let Some(ref h) = home {
        dirs.push(h.join(".local/bin"));
        dirs.push(h.join(".cargo/bin"));
        dirs.push(h.join("miniconda3/bin"));
        dirs.push(h.join("anaconda3/bin"));
        dirs.push(h.join(".pyenv/shims"));
        dirs.push(h.join("scoop/shims"));
        expand_child_bin_dirs(&h.join(".local/pipx/venvs"), "bin", &mut dirs);
        expand_child_bin_dirs(&h.join("Library/Python"), "bin", &mut dirs);
    }
    if let Some(local_app) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        dirs.push(local_app.join("Microsoft/WinGet/Links"));
        expand_child_bin_dirs(&local_app.join("Programs/Python"), "Scripts", &mut dirs);
        expand_child_bin_dirs(&local_app.join("pipx/venvs"), "Scripts", &mut dirs);
    }
    if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
        expand_child_bin_dirs(&app_data.join("Python"), "Scripts", &mut dirs);
    }
    if let Some(prog_data) = std::env::var_os("ProgramData").map(PathBuf::from) {
        dirs.push(prog_data.join("chocolatey/bin"));
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs.push(PathBuf::from("/opt/local/bin"));
    dirs.push(PathBuf::from("/usr/local/bin"));
    dirs.push(PathBuf::from("/usr/bin"));
    dirs.push(PathBuf::from("/snap/bin"));
    dirs
}

fn resolve_executable(explicit: Option<&str>, names: &[&str]) -> Option<PathBuf> {
    if let Some(raw) = explicit {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            let p = PathBuf::from(trimmed);
            if p.exists() && p.is_file() {
                return Some(p);
            }
        }
    }

    for dir in candidate_search_dirs() {
        for name in names {
            let candidate = dir.join(name);
            if candidate.exists() && candidate.is_file() {
                return Some(candidate);
            }
            #[cfg(target_os = "windows")]
            {
                let exe_candidate = dir.join(format!("{name}.exe"));
                if exe_candidate.exists() && exe_candidate.is_file() {
                    return Some(exe_candidate);
                }
            }
        }
    }
    None
}

fn probe_tool(
    id: &str,
    name: &str,
    explicit: Option<&str>,
    names: &[&str],
    version_flag: &str,
    install_hint: &str,
    purpose: &str,
) -> ToolInfo {
    if let Some(path) = resolve_executable(explicit, names) {
        let version = Command::new(&path)
            .arg(version_flag)
            .output()
            .ok()
            .and_then(|out| {
                let text = String::from_utf8_lossy(&out.stdout);
                let first_line = text.lines().next().unwrap_or("").trim();
                if first_line.is_empty() {
                    None
                } else {
                    Some(first_line.chars().take(64).collect())
                }
            });
        ToolInfo {
            id: id.to_string(),
            name: name.to_string(),
            available: true,
            resolved_path: Some(path.to_string_lossy().to_string()),
            version,
            install_hint: install_hint.to_string(),
            purpose: purpose.to_string(),
        }
    } else {
        ToolInfo {
            id: id.to_string(),
            name: name.to_string(),
            available: false,
            resolved_path: None,
            version: None,
            install_hint: install_hint.to_string(),
            purpose: purpose.to_string(),
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    sink: &dyn ProgressSink,
    job_id: &str,
    status: JobStatus,
    stage: &str,
    progress_pct: f32,
    message: &str,
    log_line: Option<&str>,
    output_dir: Option<&str>,
) {
    sink.emit(WorkbenchJobProgress {
        job_id: job_id.to_string(),
        status,
        stage: stage.to_string(),
        progress_pct,
        message: message.to_string(),
        log_line: log_line.map(|s| s.to_string()),
        output_dir: output_dir.map(|s| s.to_string()),
    });
}

pub fn sanitize_folder_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let collapsed = clean.trim().trim_matches('-').to_string();
    if collapsed.is_empty() {
        "Imported Song".to_string()
    } else {
        collapsed
    }
}

fn run_command_streamed(
    cmd: &mut Command,
    job_id: &str,
    stage: &str,
    base_pct: f32,
    cancelled: &AtomicBool,
    child_holder: &Mutex<Option<Child>>,
    sink: &dyn ProgressSink,
) -> Result<(), String> {
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch process: {e}"))?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    {
        let mut guard = child_holder.lock().map_err(|e| e.to_string())?;
        *guard = Some(child);
    }

    if let Some(so) = stdout {
        let reader = BufReader::new(so);
        for line in reader.lines().map_while(Result::ok) {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                emit_progress(
                    sink,
                    job_id,
                    JobStatus::Running,
                    stage,
                    base_pct,
                    trimmed,
                    Some(trimmed),
                    None,
                );
            }
        }
    }

    if let Some(se) = stderr {
        let reader = BufReader::new(se);
        for line in reader.lines().map_while(Result::ok) {
            if cancelled.load(Ordering::SeqCst) {
                break;
            }
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                emit_progress(
                    sink,
                    job_id,
                    JobStatus::Running,
                    stage,
                    base_pct,
                    trimmed,
                    Some(trimmed),
                    None,
                );
            }
        }
    }

    let status = {
        let mut guard = child_holder.lock().map_err(|e| e.to_string())?;
        let mut child = guard
            .take()
            .ok_or_else(|| "Process handle lost".to_string())?;
        child.wait().map_err(|e| e.to_string())?
    };

    if cancelled.load(Ordering::SeqCst) {
        return Err("Job cancelled by user".to_string());
    }

    if !status.success() {
        return Err(format!("{stage} exited with status {status}"));
    }
    Ok(())
}

pub fn run_import_job_pipeline(
    job_id: &str,
    req: ImportJobRequest,
    lib_cfg: &LibraryConfig,
    wb_cfg: &WorkbenchConfig,
    cancelled: &AtomicBool,
    child_holder: &Mutex<Option<Child>>,
    sink: &dyn ProgressSink,
) -> Result<String, String> {
    let tools = detect_toolchain_status(wb_cfg);
    emit_progress(
        sink,
        job_id,
        JobStatus::Running,
        "Preparing",
        5.0,
        "Preparing destination folder in Library...",
        None,
        None,
    );

    let (source_kind, raw_source) = match &req.source {
        ImportSource::LocalFile(p) => ("local_file".to_string(), p.trim().to_string()),
        ImportSource::YouTubeUrl(u) => ("youtube".to_string(), u.trim().to_string()),
    };
    let source_ref = canonicalize_source_ref(&source_kind, &raw_source);

    // Optional YouTube metadata probe (supplies title, artist, and chapter markers when available)
    let mut probed_yt: Option<YouTubeMetadata> = None;
    if let ImportSource::YouTubeUrl(url) = &req.source {
        if let Some(ref yt_dlp) = tools.yt_dlp.resolved_path {
            if let Ok(meta) = probe_youtube(yt_dlp, url) {
                probed_yt = Some(meta);
            }
        }
    }

    let inferred_title = match &req.song_title {
        Some(t) if !t.trim().is_empty() => t.trim().to_string(),
        _ => match &req.source {
            ImportSource::LocalFile(p) => Path::new(p)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Imported Song")
                .to_string(),
            ImportSource::YouTubeUrl(_) => probed_yt
                .as_ref()
                .map(|m| m.title.clone())
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| format!("YouTube Import {}", job_id)),
        },
    };

    let inferred_artist = req
        .artist
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            probed_yt
                .as_ref()
                .map(|m| m.uploader.trim().to_string())
                .filter(|s| !s.is_empty())
        });

    let youtube_chapters: Vec<YouTubeChapter> = probed_yt
        .as_ref()
        .map(|m| m.chapters.clone())
        .unwrap_or_default();

    let target_dir = if let Some(custom_dir) = req.target_dir.filter(|d| !d.trim().is_empty()) {
        PathBuf::from(custom_dir)
    } else {
        PathBuf::from(&lib_cfg.library_root).join(sanitize_folder_name(&inferred_title))
    };

    if !req.force_reextract {
        if let Some(cached_dir) = find_cached_library_bundle(&source_ref, &target_dir, lib_cfg) {
            emit_progress(
                sink,
                job_id,
                JobStatus::Completed,
                "Loaded from Library Cache",
                100.0,
                "Existing stems already exist in Library — skipped re-extraction. Enable Force Re-Extract to apply a different algorithm or model.",
                Some(&cached_dir),
                Some(&cached_dir),
            );
            return Ok(cached_dir);
        }
    } else {
        remove_stale_non_cue_stems(&target_dir);
    }

    std::fs::create_dir_all(&target_dir).map_err(|e| e.to_string())?;
    let staging_dir = target_dir.join(".psalm_staging");
    std::fs::create_dir_all(&staging_dir).map_err(|e| e.to_string())?;

    // Step 1: Acquire input audio into staging_dir
    let source_audio_path = match &req.source {
        ImportSource::LocalFile(local_path) => {
            let src = PathBuf::from(local_path);
            if !src.exists() {
                return Err(format!("Source file does not exist: {}", src.display()));
            }
            let ext = src
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("wav")
                .to_ascii_lowercase();

            if matches!(ext.as_str(), "wav" | "flac" | "ogg" | "mp3") {
                src
            } else {
                let ffmpeg = tools.ffmpeg.resolved_path.as_ref().ok_or_else(|| {
                    format!("Converting .{ext} files requires FFmpeg. Install FFmpeg or import WAV/MP3/FLAC/OGG directly.")
                })?;
                emit_progress(
                    sink,
                    job_id,
                    JobStatus::Running,
                    "Converting",
                    18.0,
                    &format!("Transcoding .{ext} to WAV via FFmpeg..."),
                    None,
                    None,
                );
                let out_wav = staging_dir.join("source.wav");
                let mut cmd = Command::new(ffmpeg);
                cmd.args(["-y", "-i"])
                    .arg(&src)
                    .args(["-ar", "48000", "-ac", "2"])
                    .arg(&out_wav);
                run_command_streamed(
                    &mut cmd,
                    job_id,
                    "FFmpeg Transcode",
                    22.0,
                    cancelled,
                    child_holder,
                    sink,
                )?;
                out_wav
            }
        }
        ImportSource::YouTubeUrl(url) => {
            let yt_dlp = tools.yt_dlp.resolved_path.as_ref().ok_or_else(|| {
                "yt-dlp is required to import YouTube URLs. Install yt-dlp or choose a local audio file."
                    .to_string()
            })?;
            emit_progress(
                sink,
                job_id,
                JobStatus::Running,
                "Downloading YouTube Audio",
                15.0,
                "Downloading best audio stream via yt-dlp...",
                Some(url),
                None,
            );
            let out_template = staging_dir.join("youtube_source.%(ext)s");
            let mut cmd = Command::new(yt_dlp);
            cmd.args([
                "--no-playlist",
                "-x",
                "--audio-format",
                "wav",
                "--audio-quality",
                "0",
                "-o",
            ])
            .arg(&out_template);
            if let Some(ref ffmpeg_path) = tools.ffmpeg.resolved_path {
                cmd.arg("--ffmpeg-location").arg(ffmpeg_path);
            }
            cmd.arg(url);

            run_command_streamed(
                &mut cmd,
                job_id,
                "YouTube Audio Download",
                25.0,
                cancelled,
                child_holder,
                sink,
            )?;

            std::fs::read_dir(&staging_dir)
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.is_file())
                .ok_or_else(|| "yt-dlp finished but no audio file was produced.".to_string())?
        }
    };

    if cancelled.load(Ordering::SeqCst) {
        let _ = std::fs::remove_dir_all(&staging_dir);
        return Err("Job cancelled by user".to_string());
    }

    // Step 2: Optional local stem separation
    match req.separation_engine {
        SeparationEngine::None => {
            emit_progress(
                sink,
                job_id,
                JobStatus::Running,
                "Importing",
                60.0,
                "Adding stereo track to Library bundle...",
                None,
                None,
            );
            let ext = source_audio_path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("wav");
            let dest_file = target_dir.join(format!("music.{ext}"));
            if source_audio_path != dest_file {
                std::fs::copy(&source_audio_path, &dest_file).map_err(|e| e.to_string())?;
            }
        }
        SeparationEngine::AudioSeparator => {
            let sep_bin = tools.audio_separator.resolved_path.as_ref().ok_or_else(|| {
                "audio-separator (UVR CLI) not found. Configure its path or choose 'No Separation'."
                    .to_string()
            })?;
            let model_name = match req.stem_preset {
                StemPreset::TwoStems => "UVR-MDX-NET-Inst_HQ_3.onnx".to_string(),
                StemPreset::FourStems => "htdemucs_ft.yaml".to_string(),
                StemPreset::SixStems => "htdemucs_6s.yaml".to_string(),
                StemPreset::Custom => req
                    .custom_model
                    .as_ref()
                    .filter(|m| !m.trim().is_empty())
                    .cloned()
                    .unwrap_or_else(|| wb_cfg.custom_uvr_model.clone()),
            };
            emit_progress(
                sink,
                job_id,
                JobStatus::Running,
                "Separating Stems (UVR)",
                45.0,
                &format!("Running audio-separator with model {model_name}..."),
                Some(&format!("Model: {model_name}")),
                None,
            );
            let mut cmd = Command::new(sep_bin);
            cmd.arg(&source_audio_path)
                .args(["--output_format", "WAV", "--output_dir"])
                .arg(&target_dir)
                .args(["-m", &model_name]);
            run_command_streamed(
                &mut cmd,
                job_id,
                "UVR Stem Separation",
                65.0,
                cancelled,
                child_holder,
                sink,
            )?;
        }
        SeparationEngine::Demucs => {
            let demucs_bin = tools.demucs.resolved_path.as_ref().ok_or_else(|| {
                "demucs CLI not found. Configure its path or choose 'No Separation'.".to_string()
            })?;
            let model_name = match req.stem_preset {
                StemPreset::SixStems => "htdemucs_6s",
                _ => "htdemucs",
            };
            emit_progress(
                sink,
                job_id,
                JobStatus::Running,
                "Separating Stems (Demucs)",
                45.0,
                &format!("Running Demucs ({model_name})..."),
                None,
                None,
            );
            let demucs_out = staging_dir.join("demucs_out");
            let mut cmd = Command::new(demucs_bin);
            cmd.args(["-n", model_name, "-o"]).arg(&demucs_out);
            if req.stem_preset == StemPreset::TwoStems {
                cmd.arg("--two-stems=vocals");
            }
            cmd.arg(&source_audio_path);

            run_command_streamed(
                &mut cmd,
                job_id,
                "Demucs Stem Separation",
                65.0,
                cancelled,
                child_holder,
                sink,
            )?;

            for entry in walk_wav_files(&demucs_out) {
                if let Some(fname) = entry.file_name() {
                    let _ = std::fs::copy(&entry, target_dir.join(fname));
                }
            }
        }
    }

    let _ = std::fs::remove_dir_all(&staging_dir);

    // Step 3: Analyze audio, normalize stem roles, and write initial manifest.json
    emit_progress(
        sink,
        job_id,
        JobStatus::Running,
        "Analyzing & Indexing",
        88.0,
        "Estimating BPM, downbeat, and building manifest.json...",
        None,
        None,
    );

    let mut inspected = inspect_song_folder(&target_dir).map_err(|e| e.to_string())?;
    inspected.manifest.title = inferred_title;
    if let Some(artist) = inferred_artist {
        inspected.manifest.artist = artist;
    }

    let mut normalized_stems = HashMap::new();
    for fname in &inspected.available_files {
        let (mut key, pitched, is_cues, default_gain) = normalize_separated_stem_role(fname);
        while normalized_stems.contains_key(&key) {
            key.push('_');
        }
        normalized_stems.insert(
            key,
            StemDef::Detailed {
                file: fname.clone(),
                pitched,
                is_cues: Some(is_cues),
                default_gain,
                muted: false,
            },
        );
    }
    if !normalized_stems.is_empty() {
        inspected.manifest.stems = normalized_stems;
    }

    let mut analysis_summary =
        "Song stems created and automatically added to Song Library!".to_string();
    let mut cached_import_analysis: Option<AudioAnalysisResult> = None;
    let mut key_confidence: Option<f32> = None;
    let mut review_notes: Vec<String> = Vec::new();

    if let Ok(analysis) = analyze_song_folder_mixdown(&target_dir, Some(&inspected.manifest), 640) {
        inspected.manifest.bpm = analysis.estimated_bpm;
        inspected.manifest.first_downbeat_sec = analysis.estimated_first_downbeat_sec;
        inspected.manifest.original_key = analysis
            .estimated_key
            .clone()
            .unwrap_or_else(|| "Unknown".to_string());
        key_confidence = Some(analysis.key_confidence as f32);
        cached_import_analysis = Some(analysis.clone());

        if inspected.manifest.original_key == "Unknown" {
            review_notes.push("Key could not be detected reliably — verify in Workbench.".into());
        } else if analysis.key_confidence < 0.45 {
            review_notes.push(format!(
                "Low key confidence ({:.0}%) for {} — verify in Workbench.",
                analysis.key_confidence * 100.0,
                inspected.manifest.original_key
            ));
        }

        let key_note = if let Some(ref k) = analysis.estimated_key {
            format!(
                "key {k} ({:.0}% confidence)",
                analysis.key_confidence * 100.0
            )
        } else if !analysis.key_candidates.is_empty() {
            format!(
                "key uncertain (closest: {}; set to Unknown — please verify)",
                analysis.key_candidates.join(" / ")
            )
        } else {
            "key could not be detected reliably (set to Unknown)".to_string()
        };
        analysis_summary = format!(
            "Stems added to Library — {:.1} BPM, Bar 1 @ {:.2}s, {key_note}.",
            analysis.estimated_bpm, analysis.estimated_first_downbeat_sec
        );

        let chapter_sections = chapters_to_sections(
            &youtube_chapters,
            inspected.manifest.bpm,
            inspected.manifest.time_signature[0],
            inspected.manifest.first_downbeat_sec,
            analysis.duration_sec,
        );
        if !chapter_sections.is_empty() {
            inspected.manifest.sections = chapter_sections;
            review_notes.push(
                "Verify Bar 1 downbeat and chapter-seeded sections in Workbench.".into(),
            );
        } else {
            let spb =
                (60.0 / inspected.manifest.bpm) * inspected.manifest.time_signature[0] as f64;
            let total_bars = ((analysis.duration_sec - analysis.estimated_first_downbeat_sec)
                .max(1.0)
                / spb)
                .ceil() as u32;
            inspected.manifest.sections = vec![
                SectionDef {
                    id: "intro".to_string(),
                    name: "Intro".to_string(),
                    start_bar: 1,
                    end_bar: 5.min(total_bars.max(2)),
                    start_sec: None,
                    end_sec: None,
                    color: Some("slate".to_string()),
                },
                SectionDef {
                    id: "verse-1".to_string(),
                    name: "Verse 1".to_string(),
                    start_bar: 5.min(total_bars.max(2)),
                    end_bar: (total_bars + 1).max(9),
                    start_sec: None,
                    end_sec: None,
                    color: Some("sky".to_string()),
                },
            ];
            review_notes.push("Verify Bar 1 downbeat and paint arrangement sections in Workbench.".into());
        }
    } else {
        review_notes.push("Verify BPM, Bar 1 downbeat, key, and sections in Workbench.".into());
    }

    let effective_model = match (req.separation_engine, req.stem_preset) {
        (SeparationEngine::None, _) => None,
        (SeparationEngine::AudioSeparator, StemPreset::TwoStems) => {
            Some("UVR-MDX-NET-Inst_HQ_3.onnx".to_string())
        }
        (SeparationEngine::AudioSeparator, StemPreset::FourStems) => {
            Some("htdemucs_ft.yaml".to_string())
        }
        (SeparationEngine::AudioSeparator, StemPreset::SixStems) => {
            Some("htdemucs_6s.yaml".to_string())
        }
        (SeparationEngine::AudioSeparator, StemPreset::Custom) => Some(
            req.custom_model
                .clone()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| wb_cfg.custom_uvr_model.clone()),
        ),
        (SeparationEngine::Demucs, StemPreset::SixStems) => Some("htdemucs_6s".to_string()),
        (SeparationEngine::Demucs, _) => Some("htdemucs".to_string()),
    };

    inspected.manifest.provenance = Some(ImportProvenance {
        source_kind,
        source_ref,
        original_source: Some(raw_source),
        separation_engine: format!("{:?}", req.separation_engine),
        stem_preset: format!("{:?}", req.stem_preset),
        custom_model: effective_model,
        needs_review: true,
        key_confidence,
        review_notes,
    });

    save_song_manifest(&target_dir, &inspected.manifest).map_err(|e| e.to_string())?;
    if let Some(ref analysis) = cached_import_analysis {
        write_analysis_cache(
            &target_dir,
            Some(&inspected.manifest),
            640,
            inspected.manifest.time_signature[0],
            analysis,
        );
    }

    let out_dir_str = target_dir.to_string_lossy().to_string();
    emit_progress(
        sink,
        job_id,
        JobStatus::Completed,
        "Available in Library",
        100.0,
        &analysis_summary,
        Some(&out_dir_str),
        Some(&out_dir_str),
    );
    Ok(out_dir_str)
}

/// Checks if an imported source already has a ready stem bundle in the Library
/// so we never re-download or re-run separation unless explicitly forced.
pub fn find_cached_library_bundle(
    source_ref: &str,
    target_dir: &Path,
    lib_cfg: &LibraryConfig,
) -> Option<String> {
    let entries = scan_song_library(lib_cfg);
    if let Some(matched) = entries.iter().find(|entry| {
        entry.status == LibraryEntryStatus::Ready
            && entry
                .provenance
                .as_ref()
                .is_some_and(|p| p.source_ref == source_ref)
    }) {
        return Some(matched.dir.clone());
    }

    let manifest_path = target_dir.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(existing) = SongManifest::from_file(&manifest_path) {
            let all_present = !existing.stems.is_empty()
                && existing
                    .stems
                    .values()
                    .all(|s| target_dir.join(s.file_name()).exists());
            if all_present {
                let compatible = existing
                    .provenance
                    .as_ref()
                    .is_none_or(|p| p.source_ref == source_ref);
                if compatible {
                    return Some(target_dir.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

fn remove_stale_non_cue_stems(target_dir: &Path) {
    let manifest_path = target_dir.join("manifest.json");
    let Ok(existing) = SongManifest::from_file(&manifest_path) else {
        return;
    };
    for (key, stem) in existing.stems {
        if stem.is_cues(&key) {
            continue;
        }
        let stem_path = target_dir.join(stem.file_name());
        if stem_path.exists() && stem_path.is_file() {
            let _ = std::fs::remove_file(stem_path);
        }
    }
}

fn walk_wav_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else {
        return out;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(walk_wav_files(&p));
        } else if p
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "wav" | "flac" | "mp3" | "ogg"))
        {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_test_wav(path: &Path) {
        let sr = 44_100u32;
        let frames = (2.0 * sr as f64) as usize;
        let mut pcm = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let s = ((2.0 * std::f32::consts::PI * 196.0 * (f as f32 / sr as f32)).sin()
                * 0.4
                * 32767.0) as i16;
            pcm.extend_from_slice(&s.to_le_bytes());
        }
        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + pcm.len()) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sr.to_le_bytes());
        wav.extend_from_slice(&(sr * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn test_detect_workbench_tools_never_panics() {
        let cfg = WorkbenchConfig::default();
        let status = detect_toolchain_status(&cfg);
        assert_eq!(status.ffmpeg.id, "ffmpeg");
        assert_eq!(status.yt_dlp.id, "yt_dlp");
        assert_eq!(status.audio_separator.id, "audio_separator");
        assert_eq!(status.demucs.id, "demucs");
        assert!(!status.audio_separator.install_hint.is_empty());
    }

    #[test]
    fn test_find_cached_library_bundle_matches_youtube_id() {
        let dir = std::env::temp_dir().join(format!("psalm_wb_cache_{}", std::process::id()));
        let song_dir = dir.join("Gratitude");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&song_dir).unwrap();
        std::fs::write(song_dir.join("bass.wav"), b"RIFF").unwrap();

        let mut stems = HashMap::new();
        stems.insert(
            "bass".to_string(),
            StemDef::Detailed {
                file: "bass.wav".into(),
                pitched: true,
                is_cues: Some(false),
                default_gain: 1.0,
                muted: false,
            },
        );
        let manifest = SongManifest {
            id: "gratitude".into(),
            title: "Gratitude".into(),
            artist: "".into(),
            bpm: 78.0,
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            original_key: "G".into(),
            click_mode: Default::default(),
            provenance: Some(ImportProvenance {
                source_kind: "youtube".into(),
                source_ref: canonicalize_source_ref(
                    "youtube",
                    "https://www.youtube.com/watch?v=abc123XYZ99",
                ),
                original_source: Some("https://www.youtube.com/watch?v=abc123XYZ99".into()),
                separation_engine: "AudioSeparator".into(),
                stem_preset: "SixStems".into(),
                custom_model: Some("htdemucs_6s.yaml".into()),
                needs_review: false,
                key_confidence: Some(0.9),
                review_notes: vec![],
            }),
            sections: vec![SectionDef {
                id: "intro".into(),
                name: "Intro".into(),
                start_bar: 1,
                end_bar: 5,
                start_sec: None,
                end_sec: None,
                color: None,
            }],
            stems,
        };
        save_song_manifest(&song_dir, &manifest).unwrap();

        let lib_cfg = LibraryConfig {
            library_root: dir.to_string_lossy().to_string(),
            setlists_root: dir.join("Setlists").to_string_lossy().to_string(),
            linked_folders: vec![],
            recent_setlists: vec![],
        };

        let lookup_ref =
            canonicalize_source_ref("youtube", "https://youtu.be/abc123XYZ99?t=42");
        let cached =
            find_cached_library_bundle(&lookup_ref, &dir.join("DifferentTitle"), &lib_cfg);
        assert_eq!(cached.as_deref(), Some(song_dir.to_string_lossy().as_ref()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_batch_ingest_reuses_cache_and_falls_back_when_separator_missing() {
        let dir = std::env::temp_dir().join(format!("psalm_batch_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let lib_root = dir.join("Library");
        let src_dir = dir.join("Sources");
        std::fs::create_dir_all(&lib_root).unwrap();
        std::fs::create_dir_all(&src_dir).unwrap();

        let wav_path = src_dir.join("Praise.wav");
        write_test_wav(&wav_path);

        let lib_cfg = LibraryConfig {
            library_root: lib_root.to_string_lossy().to_string(),
            setlists_root: dir.join("Setlists").to_string_lossy().to_string(),
            linked_folders: vec![],
            recent_setlists: vec![],
        };
        // Configure an unavailable explicit separator path to verify graceful fallback
        let wb_cfg = WorkbenchConfig {
            audio_separator_path: Some("/nonexistent/audio-separator-missing".into()),
            demucs_path: Some("/nonexistent/demucs-missing".into()),
            default_separation_engine: SeparationEngine::AudioSeparator,
            default_stem_preset: StemPreset::FourStems,
            ..WorkbenchConfig::default()
        };
        let tools = ToolchainStatus {
            platform: "test".into(),
            ffmpeg: ToolInfo {
                id: "ffmpeg".into(),
                name: "FFmpeg".into(),
                available: false,
                resolved_path: None,
                version: None,
                install_hint: "brew install ffmpeg".into(),
                purpose: "".into(),
            },
            yt_dlp: ToolInfo {
                id: "yt_dlp".into(),
                name: "yt-dlp".into(),
                available: false,
                resolved_path: None,
                version: None,
                install_hint: "brew install yt-dlp".into(),
                purpose: "".into(),
            },
            audio_separator: ToolInfo {
                id: "audio_separator".into(),
                name: "UVR".into(),
                available: false,
                resolved_path: None,
                version: None,
                install_hint: "pipx install audio-separator".into(),
                purpose: "".into(),
            },
            demucs: ToolInfo {
                id: "demucs".into(),
                name: "Demucs".into(),
                available: false,
                resolved_path: None,
                version: None,
                install_hint: "pipx install demucs".into(),
                purpose: "".into(),
            },
        };
        let (eff_engine, _, warn) =
            resolve_effective_separation(None, None, &wb_cfg, &tools);
        assert_eq!(eff_engine, SeparationEngine::None);
        assert!(warn.is_some());

        // Now run batch ingest with fallback on local WAV twice; 2nd run must hit cache
        let req1 = BatchIngestRequest {
            jobs: vec![ImportJobRequest {
                source: ImportSource::LocalFile(wav_path.to_string_lossy().to_string()),
                song_title: Some("Praise".into()),
                artist: Some("Elevation".into()),
                target_dir: None,
                separation_engine: SeparationEngine::None,
                stem_preset: StemPreset::FourStems,
                custom_model: None,
                force_reextract: false,
            }],
            use_default_with_fallback: false,
        };
        let res1 = ingest_songs_batch(req1, &lib_cfg, &wb_cfg, &NoopProgressSink);
        assert_eq!(res1.completed, 1);
        assert!(!res1.items[0].from_cache);
        assert!(res1.items[0].needs_review);

        let req2 = BatchIngestRequest {
            jobs: vec![ImportJobRequest {
                source: ImportSource::LocalFile(wav_path.to_string_lossy().to_string()),
                song_title: Some("Praise".into()),
                artist: None,
                target_dir: None,
                separation_engine: SeparationEngine::None,
                stem_preset: StemPreset::FourStems,
                custom_model: None,
                force_reextract: false,
            }],
            use_default_with_fallback: false,
        };
        let res2 = ingest_songs_batch(req2, &lib_cfg, &wb_cfg, &NoopProgressSink);
        assert_eq!(res2.completed, 1);
        assert!(res2.items[0].from_cache);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_analysis_cache_roundtrip_and_invalidation() {
        let dir = std::env::temp_dir().join("psalm_wb_test_analysis_cache");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let song_dir = dir.join("song_cached");
        std::fs::create_dir_all(&song_dir).unwrap();
        write_test_wav(&song_dir.join("music.wav"));

        let first = analyze_waveform_with_timing(&song_dir, true, 128, None, None, false).unwrap();
        assert_eq!(first.peaks.len(), 128);
        assert!(song_dir.join(ANALYSIS_CACHE_FILE).exists());

        let raw = std::fs::read_to_string(song_dir.join(ANALYSIS_CACHE_FILE)).unwrap();
        let mut parsed: CachedAnalysisRecord = serde_json::from_str(&raw).unwrap();
        parsed.analysis.source_file = "FROM_CACHE".into();
        std::fs::write(
            song_dir.join(ANALYSIS_CACHE_FILE),
            serde_json::to_string(&parsed).unwrap(),
        )
        .unwrap();

        let hit = analyze_waveform_with_timing(&song_dir, true, 128, None, None, false).unwrap();
        assert_eq!(hit.source_file, "FROM_CACHE");

        let refreshed =
            analyze_waveform_with_timing(&song_dir, true, 128, None, None, true).unwrap();
        assert_ne!(refreshed.source_file, "FROM_CACHE");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
