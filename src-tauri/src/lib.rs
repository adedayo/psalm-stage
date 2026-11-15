mod midi_manager;
mod workbench;

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use psalm_audio_core::analysis::AudioAnalysisResult;
use psalm_audio_core::engine::{AudioEngine, EngineCommand, EngineSnapshot};
use psalm_audio_core::library::{LibraryConfig, LibrarySongEntry, SetlistSummary};
use psalm_audio_core::loader::{
    inspect_song_folder as core_inspect_song_folder, load_setlist_manifest, save_setlist_manifest,
    save_song_manifest as core_save_song_manifest, InspectedSongBundle,
};
use psalm_audio_core::manifest::{SetlistManifest, SongManifest};
use psalm_audio_core::midi::{list_input_ports, MidiBinding, MidiPortInfo};
use psalm_audio_core::pad_presets::{
    delete_user_pad_preset as core_delete_user_pad_preset,
    list_all_pad_presets as core_list_all_pad_presets,
    save_user_pad_preset as core_save_user_pad_preset, PadPreset,
};
use psalm_audio_core::percussion_kit::{
    default_percussion_kits_root, default_percussion_loops_root,
    delete_percussion_loop as core_delete_percussion_loop,
    export_percussion_loop_bundle as core_export_percussion_loop_bundle,
    generate_studio_percussion_kit as core_generate_studio_percussion_kit,
    list_percussion_kits as core_list_percussion_kits,
    list_percussion_loops as core_list_percussion_loops,
    save_percussion_loop as core_save_percussion_loop, ExportLoopBundleRequest,
    ExportLoopBundleResult, PercussionKitInfo, StudioKitProfile,
};
use psalm_audio_core::percussion_patterns::{CustomPercussionPattern, PercussionStyle};
use psalm_audio_core::routing::{list_output_devices, AudioDeviceInfo};
use psalm_audio_core::session::{
    load_stage_session, save_stage_session_atomic, StageSessionState, STAGE_SESSION_FILENAME,
};
use tauri::{AppHandle, Emitter, Manager, RunEvent, State};

use crate::midi_manager::MidiManager;
use crate::workbench::{
    analyze_waveform_with_timing, build_ai_harness_info, compute_workspace_watch_fingerprint,
    install_cli_symlink as install_bundled_cli_symlink, start_tauri_import_job, AiHarnessInfo,
    ImportJobRequest, ToolchainStatus, WorkbenchConfig, WorkbenchManager, YouTubeMetadata,
    YouTubeSearchCandidate,
};

pub struct AppState {
    pub engine: Arc<AudioEngine>,
    pub midi: Arc<MidiManager>,
    pub workbench: Arc<WorkbenchManager>,
    pub config_dir: PathBuf,
    pub keep_awake_child: Mutex<Option<Child>>,
}

/// Prevents the OS display/screensaver/lock screen from triggering while Live Mode is active.
fn set_os_keep_awake(state: &AppState, enabled: bool) -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        let mut guard = state
            .keep_awake_child
            .lock()
            .map_err(|_| "Keep-awake lock poisoned".to_string())?;
        if enabled {
            if let Some(child) = guard.as_mut() {
                if matches!(child.try_wait(), Ok(None)) {
                    return Ok(true);
                }
            }
            let pid = std::process::id().to_string();
            let child = Command::new("caffeinate")
                .args(["-d", "-i", "-w", &pid])
                .spawn()
                .map_err(|e| format!("Failed to start caffeinate keep-awake: {e}"))?;
            *guard = Some(child);
            return Ok(true);
        } else if let Some(mut child) = guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        return Ok(false);
    }

    #[cfg(target_os = "windows")]
    {
        const ES_CONTINUOUS: u32 = 0x8000_0000;
        const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
        const ES_DISPLAY_REQUIRED: u32 = 0x0000_0002;
        unsafe extern "system" {
            fn SetThreadExecutionState(es_flags: u32) -> u32;
        }
        let flags = if enabled {
            ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS
        };
        unsafe {
            SetThreadExecutionState(flags);
        }
        return Ok(enabled);
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = state;
        Ok(enabled)
    }
}

#[tauri::command]
fn set_live_keep_awake(enabled: bool, state: State<'_, AppState>) -> Result<bool, String> {
    set_os_keep_awake(&state, enabled)
}

#[tauri::command]
fn list_audio_devices() -> Result<Vec<AudioDeviceInfo>, String> {
    list_output_devices().map_err(|e| e.to_string())
}

#[tauri::command]
fn list_midi_ports() -> Result<Vec<MidiPortInfo>, String> {
    list_input_ports().map_err(|e| e.to_string())
}

#[tauri::command]
fn connect_midi_port(
    port_index: i32,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    if port_index < 0 {
        state
            .midi
            .connect_all(Arc::clone(&state.engine), app)
            .map_err(|e| e.to_string())
    } else {
        state
            .midi
            .connect_port(port_index as usize, Arc::clone(&state.engine), app)
            .map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn connect_all_midi(state: State<'_, AppState>, app: AppHandle) -> Result<String, String> {
    state
        .midi
        .connect_all(Arc::clone(&state.engine), app)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn disconnect_midi_port(state: State<'_, AppState>, app: AppHandle) {
    state.midi.disconnect(Some(&app));
}

#[tauri::command]
fn connected_midi_port(state: State<'_, AppState>) -> Option<String> {
    state.midi.connected_port_name()
}

#[tauri::command]
fn get_snapshot(state: State<'_, AppState>) -> EngineSnapshot {
    state.engine.snapshot()
}

#[tauri::command]
fn send_command(command: EngineCommand, state: State<'_, AppState>) -> Result<(), String> {
    state.engine.send(command).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_pad_presets(state: State<'_, AppState>) -> Vec<PadPreset> {
    core_list_all_pad_presets(&state.config_dir)
}

#[tauri::command]
fn save_user_pad_preset(preset: PadPreset, state: State<'_, AppState>) -> Result<Vec<PadPreset>, String> {
    core_save_user_pad_preset(&state.config_dir, preset).map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_user_pad_preset(id: String, state: State<'_, AppState>) -> Result<Vec<PadPreset>, String> {
    core_delete_user_pad_preset(&state.config_dir, &id).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_percussion_kits() -> Vec<PercussionKitInfo> {
    core_list_percussion_kits(&default_percussion_kits_root())
}

#[tauri::command]
async fn generate_studio_percussion_kit(
    profile: StudioKitProfile,
) -> Result<PercussionKitInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        core_generate_studio_percussion_kit(&default_percussion_kits_root(), profile)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Kit generation task failed: {e}"))?
}

#[tauri::command]
fn list_percussion_loops() -> Vec<CustomPercussionPattern> {
    core_list_percussion_loops(&default_percussion_loops_root())
}

#[tauri::command]
fn save_percussion_loop(
    pattern: CustomPercussionPattern,
) -> Result<Vec<CustomPercussionPattern>, String> {
    core_save_percussion_loop(&default_percussion_loops_root(), pattern)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_percussion_loop(id: String) -> Result<Vec<CustomPercussionPattern>, String> {
    core_delete_percussion_loop(&default_percussion_loops_root(), &id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn clone_style_to_custom_pattern(
    style: PercussionStyle,
    id: String,
    name: String,
) -> CustomPercussionPattern {
    style.to_custom_pattern(&id, &name)
}

#[tauri::command]
async fn export_percussion_loop_bundle(
    request: ExportLoopBundleRequest,
    state: State<'_, AppState>,
) -> Result<ExportLoopBundleResult, String> {
    let lib_root = PathBuf::from(state.workbench.get_library_config().library_root);
    tauri::async_runtime::spawn_blocking(move || {
        core_export_percussion_loop_bundle(&lib_root, request).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Loop bundle export task failed: {e}"))?
}

#[tauri::command]
fn get_midi_bindings(state: State<'_, AppState>) -> Vec<MidiBinding> {
    state.midi.get_bindings()
}

#[tauri::command]
fn save_midi_binding(
    binding: MidiBinding,
    state: State<'_, AppState>,
) -> Result<Vec<MidiBinding>, String> {
    state
        .midi
        .upsert_binding(binding)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn delete_midi_binding(
    id: String,
    state: State<'_, AppState>,
) -> Result<Vec<MidiBinding>, String> {
    state.midi.remove_binding(&id).map_err(|e| e.to_string())
}

#[tauri::command]
fn set_midi_learn_mode(enabled: bool, state: State<'_, AppState>) {
    state.midi.set_learn_mode(enabled);
}

#[tauri::command]
async fn inspect_song_folder(dir: String) -> Result<InspectedSongBundle, String> {
    tauri::async_runtime::spawn_blocking(move || {
        core_inspect_song_folder(Path::new(&dir)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Inspect task failed: {e}"))?
}

#[tauri::command]
async fn save_song_manifest(
    dir: String,
    manifest: SongManifest,
    reload_if_active: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let engine = Arc::clone(&state.engine);
    tauri::async_runtime::spawn_blocking(move || {
        core_save_song_manifest(Path::new(&dir), &manifest).map_err(|e| e.to_string())?;
        if reload_if_active {
            let _ = engine.send(EngineCommand::LoadSongDir(dir));
        }
        Ok(())
    })
    .await
    .map_err(|e| format!("Save manifest task failed: {e}"))?
}

#[tauri::command]
async fn save_setlist_file(path: String, setlist: SetlistManifest) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        save_setlist_manifest(Path::new(&path), &setlist).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Save setlist task failed: {e}"))?
}

#[tauri::command]
async fn load_setlist_file(path: String) -> Result<SetlistManifest, String> {
    tauri::async_runtime::spawn_blocking(move || {
        load_setlist_manifest(Path::new(&path)).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("Load setlist task failed: {e}"))?
}

#[tauri::command]
fn get_library_config(state: State<'_, AppState>) -> LibraryConfig {
    state.workbench.get_library_config()
}

#[tauri::command]
async fn save_library_config(
    config: LibraryConfig,
    state: State<'_, AppState>,
) -> Result<LibraryConfig, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.save_library_config(config))
        .await
        .map_err(|e| format!("Save library config task failed: {e}"))?
}

#[tauri::command]
async fn ensure_library_root(state: State<'_, AppState>) -> Result<String, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.ensure_library_root())
        .await
        .map_err(|e| format!("Ensure library root task failed: {e}"))?
}

#[tauri::command]
async fn scan_song_library(state: State<'_, AppState>) -> Result<Vec<LibrarySongEntry>, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.scan_library())
        .await
        .map_err(|e| format!("Library scan task failed: {e}"))
}

#[tauri::command]
async fn list_managed_setlists(state: State<'_, AppState>) -> Result<Vec<SetlistSummary>, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.list_setlists())
        .await
        .map_err(|e| format!("Managed setlists scan task failed: {e}"))
}

#[tauri::command]
fn get_workbench_config(state: State<'_, AppState>) -> WorkbenchConfig {
    state.workbench.get_workbench_config()
}

#[tauri::command]
async fn save_workbench_config(
    config: WorkbenchConfig,
    state: State<'_, AppState>,
) -> Result<WorkbenchConfig, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.save_workbench_config(config))
        .await
        .map_err(|e| format!("Save workbench config task failed: {e}"))?
}

#[tauri::command]
async fn detect_workbench_tools(state: State<'_, AppState>) -> Result<ToolchainStatus, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.detect_tools())
        .await
        .map_err(|e| format!("Detect tools task failed: {e}"))
}

#[tauri::command]
async fn probe_youtube_url(url: String, state: State<'_, AppState>) -> Result<YouTubeMetadata, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.probe_youtube(&url))
        .await
        .map_err(|e| format!("YouTube probe task failed: {e}"))?
}

#[tauri::command]
async fn search_youtube_tracks(
    query: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<YouTubeSearchCandidate>, String> {
    let wb = Arc::clone(&state.workbench);
    tauri::async_runtime::spawn_blocking(move || wb.search_youtube(&query, limit.unwrap_or(5)))
        .await
        .map_err(|e| format!("YouTube search task failed: {e}"))?
}

#[tauri::command]
fn get_ai_harness_info() -> AiHarnessInfo {
    build_ai_harness_info()
}

#[tauri::command]
fn install_cli_symlink() -> Result<AiHarnessInfo, String> {
    install_bundled_cli_symlink()
}

#[tauri::command]
fn start_import_job(
    request: ImportJobRequest,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<String, String> {
    start_tauri_import_job(&state.workbench, request, app)
}

#[tauri::command]
fn cancel_import_job(state: State<'_, AppState>) -> Result<(), String> {
    state.workbench.cancel_active_job()
}

#[tauri::command]
async fn analyze_audio_waveform(
    path: String,
    is_dir: bool,
    num_buckets: Option<usize>,
    hint_bpm: Option<f64>,
    beats_per_bar: Option<u8>,
    force_refresh: Option<bool>,
) -> Result<AudioAnalysisResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        analyze_waveform_with_timing(
            Path::new(&path),
            is_dir,
            num_buckets.unwrap_or(640),
            hint_bpm,
            beats_per_bar,
            force_refresh.unwrap_or(false),
        )
    })
    .await
    .map_err(|e| format!("Waveform analysis task failed: {e}"))?
}

#[tauri::command]
fn open_in_browser(url: String) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", &url])
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        std::process::Command::new("xdg-open")
            .arg(&url)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let config_dir = std::env::var_os("PSALM_STAGE_CONFIG_DIR")
                .map(std::path::PathBuf::from)
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| {
                    app.path()
                        .app_config_dir()
                        .unwrap_or_else(|_| std::env::temp_dir().join("psalm-stage"))
                });

            let session_path = config_dir.join(STAGE_SESSION_FILENAME);
            let initial_session = load_stage_session(&session_path);
            let preferred_device = initial_session.as_ref().and_then(|s| s.device_name.as_deref());

            let engine = Arc::new(
                AudioEngine::start(preferred_device)
                    .expect("failed to initialize Psalm Stage audio engine"),
            );
            let had_initial_session = initial_session.is_some();
            if let Some(session) = initial_session {
                let _ = engine.send(EngineCommand::RestoreSession(session));
            }
            let midi = Arc::new(MidiManager::new(config_dir.clone()));
            let workbench = Arc::new(WorkbenchManager::new(config_dir.clone()));

            // Background watcher for externally added songs/setlists (e.g. from AI CLI/MCP)
            let wb_watch = Arc::clone(&workbench);
            let app_watch = app.handle().clone();
            thread::Builder::new()
                .name("psalm-library-watch".into())
                .spawn(move || {
                    let mut last_fp = compute_workspace_watch_fingerprint(&wb_watch);
                    loop {
                        thread::sleep(Duration::from_millis(2500));
                        let next_fp = compute_workspace_watch_fingerprint(&wb_watch);
                        if next_fp != last_fp {
                            last_fp = next_fp;
                            let _ = app_watch.emit("library:external-update", ());
                        }
                    }
                })
                .expect("failed to spawn library watcher thread");

            // Automatically connect to all detected MIDI inputs on startup (Omni mode)
            let _ = midi.connect_all(Arc::clone(&engine), app.handle().clone());

            // Push ~30fps live engine snapshots (Bar, Beat, Armed/Active section) to Angular,
            // and debounced-save persistent stage configuration + setlist state to disk.
            let engine_for_ui = Arc::clone(&engine);
            let app_handle = app.handle().clone();
            let session_path_emitter = session_path.clone();
            thread::Builder::new()
                .name("psalm-ui-emitter".into())
                .spawn(move || {
                    let startup_time = Instant::now();
                    let settle_duration = if had_initial_session {
                        Duration::from_millis(1200)
                    } else {
                        Duration::from_millis(0)
                    };
                    let mut last_saved: Option<StageSessionState> = None;
                    let mut pending_dirty: Option<(StageSessionState, Instant)> = None;

                    loop {
                        let snap = engine_for_ui.snapshot();
                        let _ = app_handle.emit("engine:snapshot", &snap);

                        // Wait for initial session preload commands to finish settling before
                        // recording a baseline, so startup never overwrites saved data early.
                        if startup_time.elapsed() >= settle_duration {
                            let current = StageSessionState::from_snapshot(&snap);
                            match &last_saved {
                                None => {
                                    last_saved = Some(current.clone());
                                    if !had_initial_session {
                                        let _ = save_stage_session_atomic(
                                            &session_path_emitter,
                                            &current,
                                        );
                                    }
                                }
                                Some(saved) if *saved != current => {
                                    let needs_update = pending_dirty
                                        .as_ref()
                                        .map_or(true, |(prev, _)| *prev != current);
                                    if needs_update {
                                        pending_dirty = Some((current, Instant::now()));
                                    } else if let Some((to_save, changed_at)) = &pending_dirty {
                                        if changed_at.elapsed() >= Duration::from_millis(400) {
                                            if save_stage_session_atomic(
                                                &session_path_emitter,
                                                to_save,
                                            )
                                            .is_ok()
                                            {
                                                last_saved = Some(to_save.clone());
                                            }
                                            pending_dirty = None;
                                        }
                                    }
                                }
                                Some(_) => {
                                    pending_dirty = None;
                                }
                            }
                        }

                        thread::sleep(Duration::from_millis(33));
                    }
                })
                .expect("failed to spawn UI emitter thread");

            // Background thread to continuously detect hotplugged / unplugged audio outputs and MIDI hardware
            let midi_hotplug = Arc::clone(&midi);
            let engine_hotplug = Arc::clone(&engine);
            let app_hotplug = app.handle().clone();
            thread::Builder::new()
                .name("psalm-hardware-hotplug".into())
                .spawn(move || {
                    let mut prev_ports: Vec<String> = list_input_ports()
                        .map(|list| list.into_iter().map(|p| p.name).collect())
                        .unwrap_or_default();
                    let mut prev_audio_devices: Vec<AudioDeviceInfo> =
                        list_output_devices().unwrap_or_default();
                    loop {
                        thread::sleep(Duration::from_millis(1500));
                        if let Ok(current_ports) = list_input_ports() {
                            let current_names: Vec<String> =
                                current_ports.iter().map(|p| p.name.clone()).collect();
                            if current_names != prev_ports {
                                prev_ports = current_names;
                                let _ = app_hotplug.emit("midi:ports", &current_ports);
                                if midi_hotplug.is_omni_or_empty() {
                                    let _ = midi_hotplug.connect_all(
                                        Arc::clone(&engine_hotplug),
                                        app_hotplug.clone(),
                                    );
                                }
                            }
                        }
                        if let Ok(current_audio) = list_output_devices() {
                            if current_audio != prev_audio_devices {
                                prev_audio_devices = current_audio.clone();
                                let _ = app_hotplug.emit("audio:devices", &current_audio);
                            }
                        }
                    }
                })
                .expect("failed to spawn hardware hotplug thread");

            app.manage(AppState {
                engine,
                midi,
                workbench,
                config_dir,
                keep_awake_child: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_audio_devices,
            list_midi_ports,
            connect_midi_port,
            connect_all_midi,
            disconnect_midi_port,
            connected_midi_port,
            get_snapshot,
            send_command,
            list_pad_presets,
            save_user_pad_preset,
            delete_user_pad_preset,
            list_percussion_kits,
            generate_studio_percussion_kit,
            list_percussion_loops,
            save_percussion_loop,
            delete_percussion_loop,
            clone_style_to_custom_pattern,
            export_percussion_loop_bundle,
            get_midi_bindings,
            save_midi_binding,
            delete_midi_binding,
            set_midi_learn_mode,
            inspect_song_folder,
            save_song_manifest,
            save_setlist_file,
            load_setlist_file,
            get_library_config,
            save_library_config,
            ensure_library_root,
            scan_song_library,
            list_managed_setlists,
            get_workbench_config,
            save_workbench_config,
            detect_workbench_tools,
            probe_youtube_url,
            search_youtube_tracks,
            get_ai_harness_info,
            install_cli_symlink,
            start_import_job,
            cancel_import_job,
            analyze_audio_waveform,
            open_in_browser,
            set_live_keep_awake,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let _ = set_os_keep_awake(&state, false);
                    let session_path = state.config_dir.join(STAGE_SESSION_FILENAME);
                    let snap = state.engine.snapshot();
                    let session = StageSessionState::from_snapshot(&snap);
                    let _ = save_stage_session_atomic(&session_path, &session);
                }
            }
        });
}
