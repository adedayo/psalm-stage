use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use midir::{Ignore, MidiInput, MidiInputConnection};
use psalm_audio_core::engine::{AudioEngine, EngineCommand};
use psalm_audio_core::midi::{
    parse_raw_midi, MidiBinding, MidiMessageKind, MidiProfile, MidiTarget, MidiTriggerBehavior,
    RawMidiEvent,
};
use psalm_audio_core::pad_presets::list_all_pad_presets;
use psalm_audio_core::{AudioCoreError, Result};
use tauri::{AppHandle, Emitter};

pub struct MidiManager {
    connections: Mutex<Vec<MidiInputConnection<()>>>,
    connected_port: RwLock<Option<String>>,
    is_omni: RwLock<bool>,
    bindings: Arc<RwLock<Vec<MidiBinding>>>,
    learn_mode: Arc<RwLock<bool>>,
    last_cc_values: Arc<Mutex<HashMap<(String, u8, u8), u8>>>,
    key_dial_accum: Arc<Mutex<HashMap<(String, u8, u8), i32>>>,
    config_path: PathBuf,
}

impl MidiManager {
    pub fn new(config_dir: PathBuf) -> Self {
        let config_path = config_dir.join("midi-profile.json");
        let bindings = load_bindings_from_disk(&config_path);

        Self {
            connections: Mutex::new(Vec::new()),
            connected_port: RwLock::new(None),
            is_omni: RwLock::new(true),
            bindings: Arc::new(RwLock::new(bindings)),
            learn_mode: Arc::new(RwLock::new(false)),
            last_cc_values: Arc::new(Mutex::new(HashMap::new())),
            key_dial_accum: Arc::new(Mutex::new(HashMap::new())),
            config_path,
        }
    }

    /// Automatically connects to all detected hardware MIDI inputs (Omni mode).
    /// This ensures any plugged-in USB MIDI keyboard or pedalboard is immediately active.
    pub fn connect_all(&self, engine: Arc<AudioEngine>, app: AppHandle) -> Result<String> {
        let mut conns_guard = self.connections.lock().unwrap();
        conns_guard.clear();

        let probe = MidiInput::new("psalm-stage-probe")
            .map_err(|e| AudioCoreError::Midi(e.to_string()))?;
        let ports = probe.ports();

        if ports.is_empty() {
            if let Ok(mut p) = self.connected_port.write() {
                *p = None;
            }
            let _ = app.emit("midi:connected-port", &None::<String>);
            return Ok("No MIDI inputs detected".to_string());
        }

        let mut connected_names = Vec::new();

        for i in 0..ports.len() {
            let mut midi_in = match MidiInput::new(&format!("psalm-stage-in-{i}")) {
                Ok(m) => m,
                Err(_) => continue,
            };
            midi_in.ignore(Ignore::None);

            let current_ports = midi_in.ports();
            let Some(port) = current_ports.get(i) else {
                continue;
            };
            let port_name = midi_in
                .port_name(port)
                .unwrap_or_else(|_| format!("MIDI Port {i}"));

            let bindings = Arc::clone(&self.bindings);
            let learn_mode = Arc::clone(&self.learn_mode);
            let last_cc_values = Arc::clone(&self.last_cc_values);
            let key_dial_accum = Arc::clone(&self.key_dial_accum);
            let dev_name_cb = port_name.clone();
            let app_cb = app.clone();
            let engine_cb = Arc::clone(&engine);
            let config_dir_cb = self
                .config_path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));

            match midi_in.connect(
                port,
                "psalm-stage-listener",
                move |_timestamp, bytes, _| {
                    let Some(event) = parse_raw_midi(&dev_name_cb, bytes) else {
                        return;
                    };

                    let _ = app_cb.emit("midi:raw", &event);

                    let is_learning = learn_mode.read().map(|g| *g).unwrap_or(false);
                    if is_learning {
                        return;
                    }

                    dispatch_midi_event(
                        &event,
                        &bindings,
                        &engine_cb,
                        &config_dir_cb,
                        &last_cc_values,
                        &key_dial_accum,
                    );
                },
                (),
            ) {
                Ok(conn) => {
                    conns_guard.push(conn);
                    connected_names.push(port_name);
                }
                Err(e) => {
                    log::warn!("Could not connect to MIDI port {}: {}", i, e);
                }
            }
        }

        if connected_names.is_empty() {
            if let Ok(mut p) = self.connected_port.write() {
                *p = None;
            }
            let _ = app.emit("midi:connected-port", &None::<String>);
            return Ok("Failed to connect to MIDI inputs".to_string());
        }

        let summary = if connected_names.len() == 1 {
            connected_names[0].clone()
        } else {
            // Find primary keyboard or controller name if available
            let primary = connected_names
                .iter()
                .find(|n| !n.to_lowercase().contains("daw") && !n.to_lowercase().contains("editor"))
                .unwrap_or(&connected_names[0]);
            format!("Omni ({} devices: {})", connected_names.len(), primary)
        };

        if let Ok(mut p) = self.connected_port.write() {
            *p = Some(summary.clone());
        }
        if let Ok(mut o) = self.is_omni.write() {
            *o = true;
        }

        let _ = app.emit("midi:connected-port", &Some(summary.clone()));
        Ok(summary)
    }

    /// Connects to a specific port index, or all ports if `port_index == usize::MAX`.
    pub fn connect_port(
        &self,
        port_index: usize,
        engine: Arc<AudioEngine>,
        app: AppHandle,
    ) -> Result<String> {
        if port_index == usize::MAX {
            return self.connect_all(engine, app);
        }

        let mut conns_guard = self.connections.lock().unwrap();
        conns_guard.clear();

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
        let last_cc_values = Arc::clone(&self.last_cc_values);
        let key_dial_accum = Arc::clone(&self.key_dial_accum);
        let dev_name_cb = port_name.clone();
        let app_cb = app.clone();
        let engine_cb = Arc::clone(&engine);
        let config_dir_cb = self
            .config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        let conn = midi_in
            .connect(
                port,
                "psalm-stage-listener",
                move |_timestamp, bytes, _| {
                    let Some(event) = parse_raw_midi(&dev_name_cb, bytes) else {
                        return;
                    };

                    let _ = app_cb.emit("midi:raw", &event);

                    let is_learning = learn_mode.read().map(|g| *g).unwrap_or(false);
                    if is_learning {
                        return;
                    }

                    dispatch_midi_event(
                        &event,
                        &bindings,
                        &engine_cb,
                        &config_dir_cb,
                        &last_cc_values,
                        &key_dial_accum,
                    );
                },
                (),
            )
            .map_err(|e| AudioCoreError::Midi(e.to_string()))?;

        conns_guard.push(conn);

        if let Ok(mut p) = self.connected_port.write() {
            *p = Some(port_name.clone());
        }
        if let Ok(mut o) = self.is_omni.write() {
            *o = false;
        }

        let _ = app.emit("midi:connected-port", &Some(port_name.clone()));
        Ok(port_name)
    }

    pub fn disconnect(&self, app: Option<&AppHandle>) {
        if let Ok(mut guard) = self.connections.lock() {
            guard.clear();
        }
        if let Ok(mut p) = self.connected_port.write() {
            *p = None;
        }
        if let Ok(mut o) = self.is_omni.write() {
            *o = false;
        }
        if let Some(a) = app {
            let _ = a.emit("midi:connected-port", &None::<String>);
        }
    }

    pub fn is_omni_or_empty(&self) -> bool {
        let is_omni = self.is_omni.read().map(|g| *g).unwrap_or(true);
        let is_empty = self.connected_port.read().map(|g| g.is_none()).unwrap_or(true);
        is_omni || is_empty
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

fn step_pad_preset(engine: &Arc<AudioEngine>, config_dir: &PathBuf, delta: isize) -> Option<EngineCommand> {
    let presets = list_all_pad_presets(config_dir);
    if presets.is_empty() {
        return None;
    }
    let cur_id = engine.snapshot().pad_preset_id;
    let cur_idx = cur_id
        .as_deref()
        .and_then(|id| presets.iter().position(|p| p.id == id))
        .unwrap_or(0);
    let len = presets.len() as isize;
    let next_idx = ((cur_idx as isize + delta).rem_euclid(len)) as usize;
    Some(EngineCommand::SetPadPatch(presets[next_idx].to_patch_spec()))
}

/// Decodes a MIDI CC value into a signed rotary step (`+N` clockwise, `-N` counter-clockwise)
/// along with a flag indicating whether the source looks like a relative encoder.
/// Supports:
/// - Relative 2's complement encoders (1..8 = +N, 120..127 = -(128-N))
/// - Relative binary offset encoders (65..72 = +(V-64), 56..63 = -(64-V))
/// - Standard absolute CC potentiometers (0..127 tracked against previous value)
fn compute_rotary_step(prev: Option<u8>, curr: u8, invert: bool) -> (i32, bool) {
    let (raw_step, is_relative) = match prev {
        None => {
            if curr == 1 || curr == 65 {
                (1, true)
            } else if curr == 127 {
                (-1, true)
            } else if curr == 63 {
                (-1, true)
            } else {
                (0, false)
            }
        }
        Some(p) if p == curr => {
            if (1..=8).contains(&curr) {
                (curr as i32, true)
            } else if (65..=72).contains(&curr) {
                ((curr - 64) as i32, true)
            } else if (120..=127).contains(&curr) {
                (-((128 - curr) as i32), true)
            } else if (56..=63).contains(&curr) {
                (-((64 - curr) as i32), true)
            } else {
                (0, false)
            }
        }
        Some(p) if p <= 8 && curr >= 120 => (-((128 - curr) as i32), true),
        Some(p) if p >= 120 && curr <= 8 => (curr as i32, true),
        Some(65) if curr == 63 => (-1, true),
        Some(63) if curr == 65 => (1, true),
        Some(p) => {
            let diff = (curr as i32) - (p as i32);
            (diff.clamp(-4, 4), false)
        }
    };

    let step = if invert { -raw_step } else { raw_step };
    (step, is_relative)
}

fn dispatch_midi_event(
    event: &RawMidiEvent,
    bindings: &Arc<RwLock<Vec<MidiBinding>>>,
    engine: &Arc<AudioEngine>,
    config_dir: &PathBuf,
    last_cc_values: &Arc<Mutex<HashMap<(String, u8, u8), u8>>>,
    key_dial_accum: &Arc<Mutex<HashMap<(String, u8, u8), i32>>>,
) {
    let cc_key = (event.device_name.clone(), event.channel, event.number);
    let prev_cc = if event.kind == MidiMessageKind::ControlChange {
        if let Ok(mut guard) = last_cc_values.lock() {
            let prev = guard.get(&cc_key).copied();
            guard.insert(cc_key.clone(), event.value);
            prev
        } else {
            None
        }
    } else {
        None
    };

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
            MidiTarget::KeyDial => {
                let (step, is_rel) = match event.kind {
                    MidiMessageKind::ControlChange => {
                        compute_rotary_step(prev_cc, event.value, binding.invert)
                    }
                    MidiMessageKind::PitchBend => {
                        let s = if event.value > 64 {
                            1
                        } else if event.value < 64 {
                            -1
                        } else {
                            0
                        };
                        (if binding.invert { -s } else { s }, true)
                    }
                    _ => (0, false),
                };
                if step == 0 {
                    None
                } else if is_rel {
                    Some(EngineCommand::AdjustKeyShift(step.signum()))
                } else if let Ok(mut acc) = key_dial_accum.lock() {
                    let entry = acc.entry(cc_key.clone()).or_insert(0);
                    *entry += step;
                    if entry.abs() >= 4 {
                        let dir = entry.signum();
                        *entry -= dir * 4;
                        Some(EngineCommand::AdjustKeyShift(dir))
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
            MidiTarget::TempoUp => Some(EngineCommand::AdjustBpm(1.0)),
            MidiTarget::TempoDown => Some(EngineCommand::AdjustBpm(-1.0)),
            MidiTarget::TempoDial => {
                let step = match event.kind {
                    MidiMessageKind::ControlChange => {
                        compute_rotary_step(prev_cc, event.value, binding.invert).0
                    }
                    MidiMessageKind::PitchBend => {
                        let s = if event.value > 64 {
                            1
                        } else if event.value < 64 {
                            -1
                        } else {
                            0
                        };
                        if binding.invert { -s } else { s }
                    }
                    _ => 0,
                };
                if step != 0 {
                    Some(EngineCommand::AdjustBpm(step as f64))
                } else {
                    None
                }
            }
            MidiTarget::NextSong => Some(EngineCommand::NextSong),
            MidiTarget::PrevSong => Some(EngineCommand::PrevSong),
            MidiTarget::TogglePad => Some(EngineCommand::TogglePad),
            MidiTarget::ToggleClick => Some(EngineCommand::ToggleClickMuted),
            MidiTarget::PadGain => Some(EngineCommand::SetPadGain(norm * 1.2)),
            MidiTarget::PadBrightness => Some(EngineCommand::SetPadBrightness(norm)),
            MidiTarget::PadShimmer => Some(EngineCommand::SetPadShimmer(norm)),
            MidiTarget::PadCrossfade => Some(EngineCommand::SetPadCrossfadeSec(0.5 + norm * 11.5)),
            MidiTarget::PadPresetNext => step_pad_preset(engine, config_dir, 1),
            MidiTarget::PadPresetPrev => step_pad_preset(engine, config_dir, -1),
            MidiTarget::PadKeyNext => Some(EngineCommand::AdjustPadKey(1)),
            MidiTarget::PadKeyPrev => Some(EngineCommand::AdjustPadKey(-1)),
            MidiTarget::MasterGain => Some(EngineCommand::SetMasterGain(norm * 1.2)),
            MidiTarget::BackingGain => Some(EngineCommand::SetBackingGain(norm * 1.2)),
            MidiTarget::ClickGain => Some(EngineCommand::SetClickGain(norm * 1.2)),
            MidiTarget::CuesGain => Some(EngineCommand::SetStemGain {
                name: "cues".into(),
                gain: norm * 1.2,
            }),
            MidiTarget::StemGain(name) => Some(EngineCommand::SetStemGain {
                name: name.clone(),
                gain: norm * 1.2,
            }),
            MidiTarget::TogglePercussionMode => {
                use psalm_audio_core::percussion_patterns::PercussionMode;
                let cur = engine.snapshot().percussion_mode;
                let next = match cur {
                    PercussionMode::Off => PercussionMode::Layer,
                    PercussionMode::Layer => PercussionMode::PercussionOnly,
                    PercussionMode::PercussionOnly => PercussionMode::Off,
                };
                Some(EngineCommand::SetPercussionMode(next))
            }
            MidiTarget::PercussionGain => Some(EngineCommand::SetPercussionGain(norm * 1.2)),
            MidiTarget::PercussionIntensityUp => {
                use psalm_audio_core::percussion_patterns::GrooveIntensity;
                let next = match engine.snapshot().percussion_intensity {
                    GrooveIntensity::Low => GrooveIntensity::Medium,
                    GrooveIntensity::Medium | GrooveIntensity::High => GrooveIntensity::High,
                };
                Some(EngineCommand::SetPercussionIntensity(next))
            }
            MidiTarget::PercussionIntensityDown => {
                use psalm_audio_core::percussion_patterns::GrooveIntensity;
                let next = match engine.snapshot().percussion_intensity {
                    GrooveIntensity::High => GrooveIntensity::Medium,
                    GrooveIntensity::Medium | GrooveIntensity::Low => GrooveIntensity::Low,
                };
                Some(EngineCommand::SetPercussionIntensity(next))
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotary_relative_twos_complement_steps_by_one() {
        assert_eq!(compute_rotary_step(None, 1, false), (1, true));
        assert_eq!(compute_rotary_step(Some(1), 1, false), (1, true));
        assert_eq!(compute_rotary_step(Some(1), 127, false), (-1, true));
        assert_eq!(compute_rotary_step(Some(127), 127, false), (-1, true));
        assert_eq!(compute_rotary_step(Some(127), 1, false), (1, true));
    }

    #[test]
    fn rotary_relative_binary_offset_steps_by_one() {
        assert_eq!(compute_rotary_step(None, 65, false), (1, true));
        assert_eq!(compute_rotary_step(Some(65), 65, false), (1, true));
        assert_eq!(compute_rotary_step(Some(65), 63, false), (-1, true));
        assert_eq!(compute_rotary_step(Some(63), 63, false), (-1, true));
        assert_eq!(compute_rotary_step(Some(63), 65, false), (1, true));
    }

    #[test]
    fn rotary_absolute_pot_latches_and_steps_incrementally() {
        assert_eq!(compute_rotary_step(None, 80, false), (0, false));
        assert_eq!(compute_rotary_step(Some(80), 81, false), (1, false));
        assert_eq!(compute_rotary_step(Some(81), 82, false), (1, false));
        assert_eq!(compute_rotary_step(Some(82), 81, false), (-1, false));
        assert_eq!(compute_rotary_step(Some(81), 80, true), (1, false));
    }
}
