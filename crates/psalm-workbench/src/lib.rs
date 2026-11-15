pub mod config;
pub mod pipeline;
pub mod planner;
pub mod youtube;

pub use config::{
    load_library_config, load_workbench_config, resolve_app_config_dir, save_library_config,
    save_workbench_config, SeparationEngine, StemPreset, WorkbenchConfig, APP_IDENTIFIER,
};
pub use pipeline::{
    analyze_waveform_with_timing, detect_toolchain_status, find_cached_library_bundle,
    ingest_songs_batch, resolve_effective_separation, run_import_job_pipeline, BatchIngestItemResult,
    BatchIngestRequest, BatchIngestResult, CallbackProgressSink, ImportJobRequest, ImportSource,
    JobStatus, NoopProgressSink, ProgressSink, StderrProgressSink, ToolInfo, ToolchainStatus,
    WorkbenchJobProgress, WorkbenchManager,
};
pub use planner::{
    build_sunday_setlist, create_or_update_setlist_from_items, plan_sunday_setlist,
    sanitize_setlist_slug, ManualSetlistItemInput, PlannedSongResolution, RequestedSetlistSong,
    SetlistBuildRequest, SetlistBuildResult, SetlistCreateRequest, SetlistPlanRequest,
    SetlistPlanResult, SongResolutionStatus,
};
pub use youtube::{
    chapters_to_sections, parse_youtube_search_json, probe_youtube, search_youtube,
    YouTubeChapter, YouTubeMetadata, YouTubeSearchCandidate,
};

pub const BUNDLED_SKILL_MD: &str =
    include_str!("../../../docs/ai-skills/psalm-stage-setlist/SKILL.md");

