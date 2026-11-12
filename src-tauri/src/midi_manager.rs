use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use midir::{Ignore, MidiInput, MidiInputConnection};
use psalm_audio_core::engine::{AudioEngine, EngineCommand};
use psalm_audio_core::midi::{
    parse_raw_midi, MidiBinding, MidiMessageKind, MidiProfile, MidiTarget, MidiTriggerBehavior,
    RawMidiEvent,
};
use psalm_audio_core::{AudioCoreError, Result};
use tauri::{AppHandle, Emitter};

pub struct MidiManager {
    connection: Mutex<Option<MidiInputConnection<()>>>,
    connected_port: RwLock<Option<String>>,
    bindings: Arc<RwLock<Vec<MidiBinding>>>,
    learn_mode: Arc<RwLock<bool>>,
    config_path: PathBuf,
}

impl MidiManager {
    pub fn new(config_dir: PathBuf) -> Self {
        let config_path = config_dir.join("midi-profile.json");
        let bindings = load_bindings_from_disk(&config_path);

        Self {
            connection: Mutex::new(None),
            connected_port: RwLock::new(None),
            bindings: Arc::new(RwLock::new(bindings)),
            learn_mode: Arc::new(RwLock::new(false)),
            config_path,
        }
    }

    pub fn connect_port(
        &self,
        port_index: usize,
        engine: Arc<AudioEngine>,
        app: AppHandle,
    ) -> Result<String> {
        // Drop any previous connection before opening the new port.
        if let Ok(mut guard) = self.connection.lock() {
            *guard = None;
        }

        let mut midi_in = MidiInput::new("psalm-stage-in")
            .map_err(|e| AudioCoreError::Midi(e.to_string()))?;
        midi_in.ignore(Ignore::None);

        let ports = midi_in.ports();
        let port = ports
            .get(port_index)
            .ok_or_else(|| AudioCoreError::Midi(format!("MIDI port {port_index} not found")))?;
        let port_name = midi_in
            .port_name(port)
            .unwrap_or_else(|_| format!("MIDI Port {port_index}"));

        let bindings = Arc::clone(&self.bindings);
        let learn_mode = Arc::clone(&self.learn_mode);
        let dev_name_cb = port_name.clone();

        let conn = midi_in
            .connect(
                port,
                "psalm-stage-listener",
                move |_timestamp, bytes, _| {
                    let Some(event) = parse_raw_midi(&dev_name_cb, bytes) else {
                        return;
                    };

                    let _ = app.emit("midi:raw", &event);

                    let is_learning = learn_mode.read().map(|g| *g).unwrap_or(false);
                    if is_learning {
                        return;
                    }

                    dispatch_midi_event(&event, &bindings, &engine);
                },
                (),
            )
            .map_err(|e| AudioCoreError::Midi(e.to_string()))?;

        if let Ok(mut guard) = self.connection.lock() {
            *guard = Some(conn);
        }
        if let Ok(mut p) = self.connected_port.write() {
            *p = Some(port_name.clone());
        }

        Ok(port_name)
    }

    pub fn disconnect(&self) {
        if let Ok(mut guard) = self.connection.lock() {
            *guard = None;
        }
        if let Ok(mut p) = self.connected_port.write() {
            *p = None;
        }
    }

    pub fn connected_port_name(&self) -> Option<String> {
        self.connected_port.read().ok().and_then(|g| g.clone())
    }

    pub fn set_learn_mode(&self, enabled: bool) {
        if let Ok(mut g) = self.learn_mode.write() {
            *g = enabled;
        }
    }

    pub fn is_learn_mode(&self) -> bool {
        self.learn_mode.read().map(|g| *g).unwrap_or(false)
    }

    pub fn get_bindings(&self) -> Vec<MidiBinding> {
        self.bindings
            .read()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn upsert_binding(&self, binding: MidiBinding) -> Result<Vec<MidiBinding>> {
        let updated = {
            let mut guard = self
                .bindings
                .write()
                .map_err(|_| AudioCoreError::Midi("Lock poisoned".into()))?;
            if let Some(pos) = guard.iter().position(|b| b.id == binding.id) {
                guard[pos] = binding;
            } else {
                guard.push(binding);
            }
            guard.clone()
        };
        save_bindings_to_disk(&self.config_path, &updated)?;
        Ok(updated)
    }

    pub fn remove_binding(&self, id: &str) -> Result<Vec<MidiBinding>> {
        let updated = {
            let mut guard = self
                .bindings
                .write()
                .map_err(|_| AudioCoreError::Midi("Lock poisoned".into()))?;
            guard.retain(|b| b.id != id);
            guard.clone()
        };
        save_bindings_to_disk(&self.config_path, &updated)?;
        Ok(updated)
    }
}

fn dispatch_midi_event(
    event: &RawMidiEvent,
    bindings: &Arc<RwLock<Vec<MidiBinding>>>,
    engine: &Arc<AudioEngine>,
) {
    let Ok(list) = bindings.read() else {
        return;
    };
    for binding in list.iter() {
        if let Some(ref dev) = binding.device_name {
            if dev != &event.device_name {
                continue;
            }
        }
        if let Some(ch) = binding.channel {
            if ch != event.channel {
                continue;
            }
        }
        if binding.kind != event.kind || binding.number != event.number {
            continue;
        }

        // For momentary buttons sending CC, trigger only on press (>= 64).
        if matches!(
            binding.behavior,
            MidiTriggerBehavior::Momentary | MidiTriggerBehavior::Toggle
        ) && event.kind == MidiMessageKind::ControlChange
            && event.value < 64
        {
            continue;
        }

        let norm = if binding.invert {
            1.0 - (event.value as f32 / 127.0)
        } else {
            event.value as f32 / 127.0
        };

        let cmd = match &binding.target {
            MidiTarget::PlayStop => Some(EngineCommand::PlayStop),
            MidiTarget::Play => Some(EngineCommand::Play),
            MidiTarget::Stop => Some(EngineCommand::Stop),
            MidiTarget::LoopToggle => Some(EngineCommand::ToggleLoop),
            MidiTarget::SectionBySlot(slot) => Some(EngineCommand::QueueSectionBySlot(*slot)),
            MidiTarget::SectionByName(name) => Some(EngineCommand::QueueSectionByName(name.clone())),
            MidiTarget::NextSection => Some(EngineCommand::NextSection),
            MidiTarget::PrevSection => Some(EngineCommand::PrevSection),
            MidiTarget::KeyUp => Some(EngineCommand::AdjustKeyShift(1)),
            MidiTarget::KeyDown => Some(EngineCommand::AdjustKeyShift(-1)),
            MidiTarget::TempoUp => Some(EngineCommand::AdjustBpm(1.0)),
            MidiTarget::TempoDown => Some(EngineCommand::AdjustBpm(-1.0)),
            MidiTarget::MasterGain => Some(EngineCommand::SetMasterGain(norm * 1.2)),
            MidiTarget::ClickGain => Some(EngineCommand::SetClickGain(norm * 1.2)),
            MidiTarget::CuesGain => Some(EngineCommand::SetStemGain {
                name: "cues".into(),
                gain: norm * 1.2,
            }),
            MidiTarget::StemGain(name) => Some(EngineCommand::SetStemGain {
                name: name.clone(),
                gain: norm * 1.2,
            }),
        };

        if let Some(c) = cmd {
            let _ = engine.send(c);
        }
    }
}

fn load_bindings_from_disk(path: &PathBuf) -> Vec<MidiBinding> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<MidiProfile>(&raw).ok())
        .map(|p| p.bindings)
        .unwrap_or_default()
}

fn save_bindings_to_disk(path: &PathBuf, bindings: &[MidiBinding]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let profile = MidiProfile {
        name: "Default Stage Profile".into(),
        bindings: bindings.to_vec(),
    };
    let json = serde_json::to_string_pretty(&profile)
        .map_err(|e| AudioCoreError::Midi(e.to_string()))?;
    std::fs::write(path, json)?;
    Ok(())
}
