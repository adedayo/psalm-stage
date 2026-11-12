mod midi_manager;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use psalm_audio_core::engine::{AudioEngine, EngineCommand, EngineSnapshot};
use psalm_audio_core::midi::{list_input_ports, MidiBinding, MidiPortInfo};
use psalm_audio_core::routing::{list_output_devices, AudioDeviceInfo};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::midi_manager::MidiManager;

pub struct AppState {
    pub engine: Arc<AudioEngine>,
    pub midi: Arc<MidiManager>,
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
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("psalm-stage"));

            let engine = Arc::new(
                AudioEngine::start(None).expect("failed to initialize Psalm Stage audio engine"),
            );
            let midi = Arc::new(MidiManager::new(config_dir));

            // Automatically connect to all detected MIDI inputs on startup (Omni mode)
            let _ = midi.connect_all(Arc::clone(&engine), app.handle().clone());

            // Push ~30fps live engine snapshots (Bar, Beat, Armed/Active section) to Angular.
            let engine_for_ui = Arc::clone(&engine);
            let app_handle = app.handle().clone();
            thread::Builder::new()
                .name("psalm-ui-emitter".into())
                .spawn(move || loop {
                    let snap = engine_for_ui.snapshot();
                    let _ = app_handle.emit("engine:snapshot", &snap);
                    thread::sleep(Duration::from_millis(33));
                })
                .expect("failed to spawn UI emitter thread");

            // Background thread to continuously detect hotplugged / unplugged MIDI hardware
            let midi_hotplug = Arc::clone(&midi);
            let engine_hotplug = Arc::clone(&engine);
            let app_hotplug = app.handle().clone();
            thread::Builder::new()
                .name("psalm-midi-hotplug".into())
                .spawn(move || {
                    let mut prev_ports: Vec<String> = list_input_ports()
                        .map(|list| list.into_iter().map(|p| p.name).collect())
                        .unwrap_or_default();
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
                    }
                })
                .expect("failed to spawn MIDI hotplug thread");

            app.manage(AppState { engine, midi });
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
            get_midi_bindings,
            save_midi_binding,
            delete_midi_binding,
            set_midi_learn_mode,
            open_in_browser,
        ])
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
