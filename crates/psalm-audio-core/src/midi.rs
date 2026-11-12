use midir::MidiInput;
use serde::{Deserialize, Serialize};

use crate::{AudioCoreError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiPortInfo {
    pub index: usize,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiMessageKind {
    Note,
    ControlChange,
    ProgramChange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiTriggerBehavior {
    Momentary,
    Toggle,
    Continuous,
}

/// Semantic target IDs matching MainStage-style control assignment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum MidiTarget {
    PlayStop,
    Play,
    Stop,
    LoopToggle,
    SectionBySlot(usize),
    SectionByName(String),
    NextSection,
    PrevSection,
    KeyUp,
    KeyDown,
    TempoUp,
    TempoDown,
    MasterGain,
    ClickGain,
    CuesGain,
    StemGain(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MidiBinding {
    pub id: String,
    pub device_name: Option<String>,
    /// 1..=16, or None to match any channel.
    pub channel: Option<u8>,
    pub kind: MidiMessageKind,
    pub number: u8,
    pub behavior: MidiTriggerBehavior,
    pub target: MidiTarget,
    #[serde(default)]
    pub invert: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MidiProfile {
    pub name: String,
    pub bindings: Vec<MidiBinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawMidiEvent {
    pub device_name: String,
    pub channel: u8,
    pub kind: MidiMessageKind,
    pub number: u8,
    pub value: u8,
}

pub fn list_input_ports() -> Result<Vec<MidiPortInfo>> {
    let midi_in = MidiInput::new("psalm-stage-probe")
        .map_err(|e| AudioCoreError::Midi(e.to_string()))?;

    let mut ports = Vec::new();
    for (index, port) in midi_in.ports().iter().enumerate() {
        let name = midi_in
            .port_name(port)
            .unwrap_or_else(|_| format!("MIDI Port {index}"));
        ports.push(MidiPortInfo { index, name });
    }
    Ok(ports)
}

pub fn parse_raw_midi(device_name: &str, bytes: &[u8]) -> Option<RawMidiEvent> {
    if bytes.is_empty() {
        return None;
    }
    let status = bytes[0];
    let msg_type = status & 0xF0;
    let channel = (status & 0x0F) + 1;

    match msg_type {
        0x90 if bytes.len() >= 3 => {
            // Note On with velocity 0 is MIDI Note Off; ignore release for momentary triggers.
            if bytes[2] == 0 {
                return None;
            }
            Some(RawMidiEvent {
                device_name: device_name.to_string(),
                channel,
                kind: MidiMessageKind::Note,
                number: bytes[1],
                value: bytes[2],
            })
        }
        0xB0 if bytes.len() >= 3 => Some(RawMidiEvent {
            device_name: device_name.to_string(),
            channel,
            kind: MidiMessageKind::ControlChange,
            number: bytes[1],
            value: bytes[2],
        }),
        0xC0 if bytes.len() >= 2 => Some(RawMidiEvent {
            device_name: device_name.to_string(),
            channel,
            kind: MidiMessageKind::ProgramChange,
            number: bytes[1],
            value: 127,
        }),
        _ => None,
    }
}
