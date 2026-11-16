use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};

use crate::click::{ClickDelayBuffer, ClickSynth};
use crate::library::compute_semitone_shift;
use crate::loader::{
    generate_demo_song, generate_standalone_slot_song, load_song_from_dir, LoadedSong,
};
use crate::manifest::{
    ClickMode, PadLockConfig, SectionDef, SetlistItem, SetlistManifest, SetlistSection,
    SetlistSlotKind, SongManifest, StemMixOverride, TransitionMode,
};
use crate::pad::{
    default_foundation_layers, transpose_pad_key, PadLayerConfig, PadPatchSpec, PadSynth,
};
use crate::percussion::{
    section_intensity_from_name, PercussionChannelConfig, PercussionEngine, PercussionLockConfig,
    DEFAULT_PERCUSSION_ROOM_MIX,
};
use crate::percussion_kit::load_percussion_kit_from_dir;
use crate::percussion_patterns::{
    CustomPercussionPattern, GrooveIntensity, HitVariant, PercussionInstrument, PercussionMode,
    PercussionStyle,
};
use crate::session::StageSessionState;
use crate::routing::{find_multichannel_output_config, find_output_device, OutputRoutingMode};
use crate::stretcher::StemStretcher;
use crate::{AudioCoreError, Result};

/// Quantization boundary for live section jumps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuantizeMode {
    NextBar,
    EndOfSection,
}

impl Default for QuantizeMode {
    fn default() -> Self {
        Self::NextBar
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "error")]
pub enum PreloadState {
    Unloaded,
    Loading,
    Ready,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistItemSnapshot {
    pub slot: usize,
    pub song_id: String,
    pub song_dir: Option<String>,
    pub title: String,
    pub original_key: String,
    pub effective_key: String,
    pub base_bpm: f64,
    pub effective_bpm: f64,
    pub key_shift_semitones: i32,
    pub bpm_override: Option<f64>,
    pub transition_mode: TransitionMode,
    pub crossfade_sec: f32,
    pub ambient_pad_enabled: bool,
    pub pad_key_override: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pad_lock: Option<PadLockConfig>,
    pub click_mode: Option<ClickMode>,
    pub effective_click_mode: ClickMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub section_id: Option<String>,
    #[serde(default)]
    pub slot_kind: SetlistSlotKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_signature: Option<[u8; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percussion_lock: Option<PercussionLockConfig>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stem_mix: Vec<StemMixOverride>,
    pub estimated_duration_sec: f64,
    pub preload_state: PreloadState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StemSnapshot {
    pub name: String,
    pub pitched: bool,
    pub is_cues: bool,
    pub gain: f32,
    pub muted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionSnapshot {
    pub id: String,
    pub name: String,
    pub start_bar: u32,
    pub end_bar: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_sec: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_sec: Option<f64>,
    pub color: Option<String>,
}

/// Lightweight snapshot emitted to the UI at ~30fps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub song_id: Option<String>,
    pub active_song_dir: Option<String>,
    pub song_title: String,
    pub original_key: String,
    pub current_key: String,
    pub is_playing: bool,
    pub current_bar: u32,
    pub current_beat: u32,
    pub beats_per_bar: u32,
    pub current_position_sec: f64,
    pub total_duration_sec: f64,
    pub active_section_id: Option<String>,
    pub armed_section_id: Option<String>,
    pub loop_enabled: bool,
    pub continue_until_stopped: bool,
    pub quantize_mode: QuantizeMode,
    pub key_shift_semitones: i32,
    pub base_bpm: f64,
    pub current_bpm: f64,
    pub master_gain: f32,
    pub backing_gain: f32,
    pub click_gain: f32,
    pub click_muted: bool,
    pub click_mode: ClickMode,
    pub click_intro_end_bar: u32,
    pub pad_enabled: bool,
    pub pad_muted: bool,
    pub pad_gain: f32,
    pub pad_key: String,
    pub pad_is_minor: bool,
    pub pad_follow_song_key: bool,
    pub pad_preset_id: Option<String>,
    pub pad_preset_name: String,
    pub pad_layers: Vec<PadLayerConfig>,
    pub pad_brightness: f32,
    pub pad_shimmer: f32,
    pub pad_pan: f32,
    pub pad_crossfade_sec: f32,
    pub setlist_name: String,
    pub setlist_sections: Vec<SetlistSection>,
    pub setlist_items: Vec<SetlistItemSnapshot>,
    pub active_setlist_slot: Option<usize>,
    pub song_crossfade_progress: Option<f32>,
    pub crossfading_from_title: Option<String>,
    pub percussion_mode: PercussionMode,
    pub percussion_style: PercussionStyle,
    pub percussion_intensity: GrooveIntensity,
    pub percussion_follow_sections: bool,
    pub percussion_gain: f32,
    pub percussion_iem_send: f32,
    pub percussion_room_mix: f32,
    pub percussion_kit_name: Option<String>,
    pub percussion_kit_dir: Option<String>,
    pub percussion_custom_pattern: Option<CustomPercussionPattern>,
    pub percussion_current_step: Option<u8>,
    pub percussion_audition_playing: bool,
    pub percussion_audition_bpm: f64,
    pub percussion_channels: Vec<PercussionChannelConfig>,
    pub routing_mode: OutputRoutingMode,
    pub device_name: String,
    pub device_sample_rate: u32,
    pub device_channels: u16,
    pub sections: Vec<SectionSnapshot>,
    pub stems: Vec<StemSnapshot>,
}

fn deserialize_some<'de, T, D>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    T: Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    Deserialize::deserialize(deserializer).map(Some)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SetlistBatchPatch {
    #[serde(default)]
    pub slots: Vec<usize>,
    #[serde(default)]
    pub target_key: Option<String>,
    #[serde(default)]
    pub key_shift_semitones: Option<i32>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub bpm_override: Option<Option<f64>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub click_mode: Option<Option<ClickMode>>,
    #[serde(default)]
    pub transition_mode: Option<TransitionMode>,
    #[serde(default)]
    pub crossfade_sec: Option<f32>,
    #[serde(default)]
    pub ambient_pad_enabled: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub pad_lock: Option<Option<PadLockConfig>>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub section_id: Option<Option<String>>,
    #[serde(default)]
    pub percussion_mode: Option<PercussionMode>,
    #[serde(default)]
    pub percussion_style: Option<PercussionStyle>,
    #[serde(default)]
    pub percussion_intensity: Option<GrooveIntensity>,
    #[serde(default, deserialize_with = "deserialize_some")]
    pub percussion_custom_pattern: Option<Option<CustomPercussionPattern>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum EngineCommand {
    LoadDemoSong,
    LoadSongDir(String),
    Play,
    Stop,
    PlayStop,
    SeekSec(f64),
    QueueSectionById(String),
    QueueSectionBySlot(usize),
    QueueSectionByName(String),
    NextSection,
    PrevSection,
    ToggleLoop,
    SetLoop(bool),
    ToggleContinueTrack,
    SetContinueTrack(bool),
    SetQuantizeMode(QuantizeMode),
    SetKeyShift(i32),
    AdjustKeyShift(i32),
    SetBpm(f64),
    AdjustBpm(f64),
    SetSongTiming {
        bpm: f64,
        first_downbeat_sec: f64,
        beats_per_bar: u8,
        #[serde(default)]
        click_mode: Option<ClickMode>,
        #[serde(default)]
        sections: Option<Vec<SectionDef>>,
    },
    SetMasterGain(f32),
    SetBackingGain(f32),
    SetClickGain(f32),
    SetClickMuted(bool),
    ToggleClickMuted,
    SetClickMode(ClickMode),
    SetStemGain { name: String, gain: f32 },
    SetStemMuted { name: String, muted: bool },
    SetPadEnabled(bool),
    SetPadMuted(bool),
    TogglePad,
    SetPadGain(f32),
    SetPadKey(String),
    AdjustPadKey(i32),
    SetPadFollowSongKey(bool),
    SetPadCrossfadeSec(f32),
    SetPadPatch(PadPatchSpec),
    SetPadLayer {
        index: usize,
        layer: Option<PadLayerConfig>,
    },
    SetPadLayerGain {
        index: usize,
        gain: f32,
    },
    SetPadBrightness(f32),
    SetPadShimmer(f32),
    SetPadPan(f32),
    LockCurrentPadToSlot(usize),
    ClearSlotPadLock(usize),
    AddSetlistSong {
        dir: String,
        insert_at: Option<usize>,
    },
    RemoveSetlistSlot(usize),
    MoveSetlistSlot {
        from: usize,
        to: usize,
    },
    SelectSetlistSlot(usize),
    NextSong,
    PrevSong,
    SetSetlistName(String),
    UpdateSetlistSlotConfig {
        slot: usize,
        #[serde(default)]
        key_shift_semitones: Option<i32>,
        #[serde(default, deserialize_with = "deserialize_some")]
        bpm_override: Option<Option<f64>>,
        #[serde(default)]
        transition_mode: Option<TransitionMode>,
        #[serde(default)]
        crossfade_sec: Option<f32>,
        #[serde(default)]
        ambient_pad_enabled: Option<bool>,
        #[serde(default, deserialize_with = "deserialize_some")]
        pad_key_override: Option<Option<String>>,
        #[serde(default, deserialize_with = "deserialize_some")]
        pad_lock: Option<Option<PadLockConfig>>,
        #[serde(default, deserialize_with = "deserialize_some")]
        click_mode: Option<Option<ClickMode>>,
        #[serde(default, deserialize_with = "deserialize_some")]
        section_id: Option<Option<String>>,
        #[serde(default, deserialize_with = "deserialize_some")]
        percussion_lock: Option<Option<PercussionLockConfig>>,
    },
    BatchUpdateSetlistSlots(SetlistBatchPatch),
    UpsertSetlistSection(SetlistSection),
    RemoveSetlistSection(String),
    MoveSetlistSection {
        id: String,
        to_index: usize,
    },
    ToggleSetlistSectionCollapsed {
        id: String,
        collapsed: bool,
    },
    AddStandaloneSetlistSlot {
        title: String,
        slot_kind: SetlistSlotKind,
        key: String,
        bpm: f64,
        time_signature: [u8; 2],
        section_id: Option<String>,
        insert_at: Option<usize>,
    },
    SetPercussionMode(PercussionMode),
    SetPercussionStyle(PercussionStyle),
    SetPercussionIntensity(GrooveIntensity),
    SetPercussionFollowSections(bool),
    SetPercussionGain(f32),
    SetPercussionIemSend(f32),
    SetPercussionRoom(f32),
    SetCustomPercussionPattern(Option<CustomPercussionPattern>),
    LoadPercussionKitDir(Option<String>),
    TriggerPercussionPreview {
        instrument: PercussionInstrument,
        variant: HitVariant,
        velocity: f32,
    },
    StartPercussionAudition {
        bpm: f64,
        beats_per_bar: u8,
        with_click: bool,
    },
    StopPercussionAudition,
    SetPercussionAuditionBpm(f64),
    SetPercussionChannel {
        instrument: PercussionInstrument,
        enabled: Option<bool>,
        gain: Option<f32>,
        pan: Option<f32>,
    },
    ResetPercussionMix,
    LockCurrentPercussionToSlot(usize),
    ClearSlotPercussionLock(usize),
    LoadSetlist(SetlistManifest),
    ClearSetlist,
    SetRoutingMode(OutputRoutingMode),
    SelectOutputDevice(String),
    RestoreSession(StageSessionState),
}

/// 4-channel frame pushed through `rtrb`: `[music_l, music_r, iem_l, iem_r]`.
#[derive(Debug, Clone, Copy, Default)]
struct BusFrame {
    music_l: f32,
    music_r: f32,
    iem_l: f32,
    iem_r: f32,
}

struct SharedRouting {
    mode: OutputRoutingMode,
    master_gain: f32,
}

pub struct AudioEngine {
    cmd_tx: mpsc::Sender<EngineCommand>,
    snapshot: Arc<RwLock<EngineSnapshot>>,
    device_sample_rate: u32,
    shutdown: Arc<AtomicBool>,
}

impl AudioEngine {
    pub fn start(preferred_device: Option<&str>) -> Result<Self> {
        let shared_routing = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (cmd_tx, cmd_rx) = mpsc::channel::<EngineCommand>();
        let (init_tx, init_rx) = mpsc::sync_channel::<Result<(EngineSnapshot, u32)>>(1);

        let preferred = preferred_device.map(|s| s.to_string());
        let shutdown_worker = Arc::clone(&shutdown);
        let shared_routing_worker = Arc::clone(&shared_routing);

        // Allocate initial snapshot container once initialized by the worker.
        let snapshot_holder: Arc<RwLock<Option<Arc<RwLock<EngineSnapshot>>>>> =
            Arc::new(RwLock::new(None));
        let snapshot_holder_worker = Arc::clone(&snapshot_holder);

        thread::Builder::new()
            .name("psalm-mixer-dsp".into())
            .spawn(move || {
                let opened = open_audio_stream(
                    preferred.as_deref(),
                    Arc::clone(&shared_routing_worker),
                );
                let (dev_name, sample_rate, channels, stream, producer) = match opened {
                    Ok(v) => v,
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };

                let initial_song = generate_demo_song(sample_rate);
                let initial_snap = build_snapshot(
                    &initial_song,
                    &dev_name,
                    sample_rate,
                    channels,
                    OutputRoutingMode::SplitLeftRight,
                );
                let snapshot = Arc::new(RwLock::new(initial_snap.clone()));
                if let Ok(mut h) = snapshot_holder_worker.write() {
                    *h = Some(Arc::clone(&snapshot));
                }

                let _ = init_tx.send(Ok((initial_snap, sample_rate)));

                let (preload_req_tx, preload_req_rx) = mpsc::channel::<(String, u32)>();
                let (preload_res_tx, preload_res_rx) = mpsc::channel::<PreloadResult>();
                let shutdown_preloader = Arc::clone(&shutdown_worker);

                let _ = thread::Builder::new()
                    .name("psalm-song-preloader".into())
                    .spawn(move || {
                        while !shutdown_preloader.load(Ordering::Relaxed) {
                            let Ok((dir, sr)) = preload_req_rx.recv_timeout(Duration::from_millis(250)) else {
                                continue;
                            };
                            if shutdown_preloader.load(Ordering::Relaxed) {
                                break;
                            }
                            let res = load_song_from_dir(Path::new(&dir), sr).map_err(|e| e.to_string());
                            if preload_res_tx
                                .send(PreloadResult {
                                    dir,
                                    sample_rate: sr,
                                    result: res,
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    });

                let mut state = MixerState::new(
                    initial_song,
                    dev_name,
                    sample_rate,
                    channels,
                    stream,
                    producer,
                    Some(preload_req_tx),
                );

                while !shutdown_worker.load(Ordering::Relaxed) {
                    while let Ok(res) = preload_res_rx.try_recv() {
                        state.handle_preload_result(res);
                    }

                    while let Ok(cmd) = cmd_rx.try_recv() {
                        state.handle_command(cmd, &shared_routing_worker);
                    }

                    let mut pushed = false;
                    if state.has_active_audio() {
                        let avail = state.producer.slots();
                        if avail >= 256 {
                            let frames_to_render = avail.min(512);
                            let rendered = state.render_block(frames_to_render);
                            for frame in rendered {
                                let _ = state.producer.push(frame);
                            }
                            pushed = true;
                        }
                    }

                    state.publish_snapshot(&snapshot, &shared_routing_worker);

                    if !pushed {
                        thread::sleep(Duration::from_millis(4));
                    }
                }
            })
            .map_err(|e| AudioCoreError::Stream(format!("Failed to spawn mixer thread: {e}")))?;

        let (_initial_snap, device_sample_rate) = init_rx
            .recv()
            .map_err(|e| AudioCoreError::Stream(format!("Audio engine init failed: {e}")))??;

        let snapshot = snapshot_holder
            .read()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| AudioCoreError::Stream("Missing engine snapshot".into()))?;

        Ok(Self {
            cmd_tx,
            snapshot,
            device_sample_rate,
            shutdown,
        })
    }

    pub fn send(&self, cmd: EngineCommand) -> Result<()> {
        self.cmd_tx
            .send(cmd)
            .map_err(|e| AudioCoreError::Stream(format!("Engine command channel closed: {e}")))
    }

    pub fn snapshot(&self) -> EngineSnapshot {
        self.snapshot
            .read()
            .expect("snapshot lock poisoned")
            .clone()
    }

    pub fn device_sample_rate(&self) -> u32 {
        self.device_sample_rate
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
    }
}

#[inline]
fn route_bus_frame(frame: BusFrame, mode: OutputRoutingMode, master: f32, out_frame: &mut [f32]) {
    let out_channels = out_frame.len();
    if out_channels == 0 {
        return;
    }

    let music_l = (frame.music_l * master).clamp(-1.0, 1.0);
    let music_r = (frame.music_r * master).clamp(-1.0, 1.0);
    let music_mono = ((frame.music_l + frame.music_r) * 0.5 * master).clamp(-1.0, 1.0);
    let iem_l = frame.iem_l.clamp(-1.0, 1.0);
    let iem_r = frame.iem_r.clamp(-1.0, 1.0);
    let iem_mono = ((frame.iem_l + frame.iem_r) * 0.5).clamp(-1.0, 1.0);

    match mode {
        OutputRoutingMode::SplitLeftRight => {
            if out_channels >= 2 {
                out_frame[0] = music_mono;
                out_frame[1] = iem_mono;
            } else {
                out_frame[0] = music_mono;
            }
            for s in out_frame.iter_mut().skip(2) {
                *s = 0.0;
            }
        }
        OutputRoutingMode::SplitClickLeftMusicRight => {
            if out_channels >= 2 {
                out_frame[0] = iem_mono;
                out_frame[1] = music_mono;
            } else {
                out_frame[0] = music_mono;
            }
            for s in out_frame.iter_mut().skip(2) {
                *s = 0.0;
            }
        }
        OutputRoutingMode::MultiOutStereo => {
            if out_channels >= 4 {
                out_frame[0] = music_l;
                out_frame[1] = music_r;
                out_frame[2] = iem_l;
                out_frame[3] = iem_r;
                for s in out_frame.iter_mut().skip(4) {
                    *s = 0.0;
                }
            } else if out_channels >= 2 {
                out_frame[0] = music_mono;
                out_frame[1] = iem_mono;
                for s in out_frame.iter_mut().skip(2) {
                    *s = 0.0;
                }
            } else {
                out_frame[0] = music_mono;
            }
        }
        OutputRoutingMode::MultiOutClickLeftMusicRight => {
            if out_channels >= 4 {
                out_frame[0] = iem_l;
                out_frame[1] = iem_r;
                out_frame[2] = music_l;
                out_frame[3] = music_r;
                for s in out_frame.iter_mut().skip(4) {
                    *s = 0.0;
                }
            } else if out_channels >= 2 {
                out_frame[0] = iem_mono;
                out_frame[1] = music_mono;
                for s in out_frame.iter_mut().skip(2) {
                    *s = 0.0;
                }
            } else {
                out_frame[0] = music_mono;
            }
        }
    }
}

fn try_build_output_stream(
    device: &cpal::Device,
    config: cpal::SupportedStreamConfig,
    shared_routing: Arc<RwLock<SharedRouting>>,
) -> Result<(u32, u16, cpal::Stream, Producer<BusFrame>)> {
    let sample_rate = config.sample_rate().0;
    let out_channels = config.channels() as usize;

    let rb_capacity = ((sample_rate as usize) / 25).max(1024);
    let (producer, mut consumer): (Producer<BusFrame>, Consumer<BusFrame>) =
        RingBuffer::new(rb_capacity);

    let stream_config: cpal::StreamConfig = config.into();

    let stream = device
        .build_output_stream(
            &stream_config,
            move |data: &mut [f32], _| {
                let (mode, master) = shared_routing
                    .try_read()
                    .map(|g| (g.mode, g.master_gain))
                    .unwrap_or((OutputRoutingMode::SplitLeftRight, 1.0));

                for out_frame in data.chunks_mut(out_channels) {
                    let frame = consumer.pop().unwrap_or_default();
                    route_bus_frame(frame, mode, master, out_frame);
                }
            },
            |err| log::error!("cpal output stream error: {err}"),
            None,
        )
        .map_err(|e| AudioCoreError::Stream(e.to_string()))?;

    stream
        .play()
        .map_err(|e| AudioCoreError::Stream(e.to_string()))?;

    Ok((sample_rate, out_channels as u16, stream, producer))
}

fn open_audio_stream(
    preferred_device: Option<&str>,
    shared_routing: Arc<RwLock<SharedRouting>>,
) -> Result<(String, u32, u16, cpal::Stream, Producer<BusFrame>)> {
    let device = find_output_device(preferred_device)?;
    let device_name = device.name().unwrap_or_else(|_| "Unknown Device".into());
    let default_config = device
        .default_output_config()
        .map_err(|e| AudioCoreError::Device(e.to_string()))?;

    let prefer_multichannel = shared_routing
        .read()
        .map(|r| r.mode.is_multichannel())
        .unwrap_or(false);

    if prefer_multichannel && default_config.channels() < 4 {
        if let Some(multi_cfg) = find_multichannel_output_config(&device, &default_config) {
            if let Ok((sr, ch, stream, producer)) =
                try_build_output_stream(&device, multi_cfg, Arc::clone(&shared_routing))
            {
                return Ok((device_name, sr, ch, stream, producer));
            }
        }
    }

    let (sample_rate, channels, stream, producer) =
        try_build_output_stream(&device, default_config, shared_routing)?;
    Ok((device_name, sample_rate, channels, stream, producer))
}

struct PreloadResult {
    dir: String,
    sample_rate: u32,
    result: std::result::Result<LoadedSong, String>,
}

enum CacheEntry {
    Loading,
    Ready(LoadedSong),
    Error(String),
}

const MAX_XFADE_BLOCK: usize = 2048;

/// Pre-allocated outgoing song deck for zero-allocation equal-power song crossfades (`SPEC-018`).
struct CrossfadeDeck {
    active: bool,
    song: Option<LoadedSong>,
    sample_rate: u32,
    source_frame_pos: f64,
    key_shift_semitones: i32,
    tempo_ratio: f64,
    percussion_only: bool,
    fade_frame_idx: usize,
    fade_total_frames: usize,
    from_title: String,
    pitched_stretcher: StemStretcher,
    drums_stretcher: StemStretcher,
    pitched_in: Vec<f32>,
    drums_in: Vec<f32>,
    pitched_out: Vec<f32>,
    drums_out: Vec<f32>,
    out_buf: Vec<[f32; 2]>,
}

impl CrossfadeDeck {
    fn new(sample_rate: u32) -> Self {
        Self {
            active: false,
            song: None,
            sample_rate: sample_rate.max(8000),
            source_frame_pos: 0.0,
            key_shift_semitones: 0,
            tempo_ratio: 1.0,
            percussion_only: false,
            fade_frame_idx: 0,
            fade_total_frames: 0,
            from_title: String::new(),
            pitched_stretcher: StemStretcher::new(sample_rate),
            drums_stretcher: StemStretcher::new(sample_rate),
            pitched_in: vec![0.0; MAX_XFADE_BLOCK * 2],
            drums_in: vec![0.0; MAX_XFADE_BLOCK * 2],
            pitched_out: vec![0.0; MAX_XFADE_BLOCK * 2],
            drums_out: vec![0.0; MAX_XFADE_BLOCK * 2],
            out_buf: vec![[0.0; 2]; MAX_XFADE_BLOCK],
        }
    }

    fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(8000);
        self.pitched_stretcher.set_sample_rate(sample_rate);
        self.drums_stretcher.set_sample_rate(sample_rate);
        self.clear();
    }

    fn clear(&mut self) {
        self.active = false;
        self.song = None;
        self.fade_frame_idx = 0;
        self.fade_total_frames = 0;
        self.from_title.clear();
    }

    fn trigger(
        &mut self,
        outgoing_song: LoadedSong,
        outgoing_pos: f64,
        key_shift_semitones: i32,
        tempo_ratio: f64,
        percussion_only: bool,
        crossfade_sec: f32,
    ) {
        let total = (crossfade_sec.clamp(0.0, 10.0) * self.sample_rate as f32).round() as usize;
        if total < 16 || outgoing_song.stems.is_empty() || percussion_only {
            self.clear();
            return;
        }
        self.from_title = outgoing_song.manifest.title.clone();
        self.source_frame_pos = outgoing_pos.max(0.0);
        self.key_shift_semitones = key_shift_semitones.clamp(-12, 12);
        self.tempo_ratio = tempo_ratio.clamp(0.5, 2.0);
        self.percussion_only = percussion_only;
        self.fade_frame_idx = 0;
        self.fade_total_frames = total;
        self.pitched_stretcher
            .set_semitones(self.key_shift_semitones as f32);

        let bypassed =
            self.key_shift_semitones == 0 && (self.tempo_ratio - 1.0).abs() < 0.001;
        if !bypassed {
            let preroll_frames = self
                .pitched_stretcher
                .input_latency_frames()
                .min(MAX_XFADE_BLOCK);
            let cur = self.source_frame_pos.floor() as usize;
            let start = cur.saturating_sub(preroll_frames);
            let len = cur.saturating_sub(start).min(MAX_XFADE_BLOCK);
            if len == 0 {
                self.pitched_stretcher.reset_with_preroll(&[], self.tempo_ratio);
                self.drums_stretcher.reset_with_preroll(&[], self.tempo_ratio);
            } else {
                Self::fill_slices(
                    &outgoing_song,
                    start,
                    len,
                    &mut self.pitched_in,
                    &mut self.drums_in,
                );
                self.pitched_stretcher
                    .reset_with_preroll(&self.pitched_in[..len * 2], self.tempo_ratio);
                self.drums_stretcher
                    .reset_with_preroll(&self.drums_in[..len * 2], self.tempo_ratio);
            }
        }

        self.song = Some(outgoing_song);
        self.active = true;
    }

    fn fill_slices(
        song: &LoadedSong,
        start_frame: usize,
        frames: usize,
        pitched_in: &mut [f32],
        drums_in: &mut [f32],
    ) {
        let len = frames * 2;
        pitched_in[..len].fill(0.0);
        drums_in[..len].fill(0.0);
        for stem in &song.stems {
            if stem.is_cues || stem.muted || stem.gain <= 0.0001 {
                continue;
            }
            let target = if stem.pitched {
                &mut pitched_in[..len]
            } else {
                &mut drums_in[..len]
            };
            let avail = stem.frame_count().saturating_sub(start_frame).min(frames);
            for i in 0..avail {
                let s_idx = (start_frame + i) * 2;
                let d_idx = i * 2;
                target[d_idx] += stem.samples[s_idx] * stem.gain;
                target[d_idx + 1] += stem.samples[s_idx + 1] * stem.gain;
            }
        }
    }

    /// Renders `out_frames` (clamped to `MAX_XFADE_BLOCK`) into `self.out_buf` and returns
    /// `(slice, fade_start_idx, fade_total_frames)`.
    fn render_block(&mut self, out_frames: usize) -> Option<(usize, usize)> {
        if !self.active {
            return None;
        }
        let n = out_frames.min(MAX_XFADE_BLOCK);
        let Some(ref song) = self.song else {
            self.active = false;
            return None;
        };

        let start_idx = self.fade_frame_idx;
        let total = self.fade_total_frames.max(1);
        let bypassed =
            self.key_shift_semitones == 0 && (self.tempo_ratio - 1.0).abs() < 0.001;

        if bypassed {
            let cur_start = self.source_frame_pos.round() as usize;
            Self::fill_slices(song, cur_start, n, &mut self.pitched_in, &mut self.drums_in);
            for i in 0..n {
                self.out_buf[i][0] = self.pitched_in[i * 2] + self.drums_in[i * 2];
                self.out_buf[i][1] = self.pitched_in[i * 2 + 1] + self.drums_in[i * 2 + 1];
            }
            self.source_frame_pos = (cur_start + n) as f64;
        } else {
            let cur_start = self.source_frame_pos.round() as usize;
            let cur_end = (self.source_frame_pos + (n as f64) * self.tempo_ratio).round() as usize;
            let src_frames = cur_end
                .saturating_sub(cur_start)
                .max(1)
                .min(MAX_XFADE_BLOCK);
            Self::fill_slices(
                song,
                cur_start,
                src_frames,
                &mut self.pitched_in,
                &mut self.drums_in,
            );
            let in_len = src_frames * 2;
            let out_len = n * 2;
            self.pitched_stretcher
                .process(&self.pitched_in[..in_len], &mut self.pitched_out[..out_len]);
            self.drums_stretcher
                .process(&self.drums_in[..in_len], &mut self.drums_out[..out_len]);
            for i in 0..n {
                self.out_buf[i][0] = self.pitched_out[i * 2] + self.drums_out[i * 2];
                self.out_buf[i][1] = self.pitched_out[i * 2 + 1] + self.drums_out[i * 2 + 1];
            }
            self.source_frame_pos += (n as f64) * self.tempo_ratio;
        }

        self.fade_frame_idx = (self.fade_frame_idx + n).min(total);
        if self.fade_frame_idx >= total {
            self.active = false;
            self.song = None;
        }
        Some((start_idx, total))
    }
}

struct SessionPostLoadOverrides {
    click_muted: bool,
    pad_follow_song_key: bool,
    pad_key: String,
    key_shift_semitones: Option<i32>,
    bpm: Option<f64>,
}

struct MixerState {
    song: LoadedSong,
    active_song_dir: Option<String>,
    sample_rate: u32,
    is_playing: bool,
    source_frame_pos: f64,
    last_beat_index: Option<i64>,
    active_section_idx: Option<usize>,
    armed_section_idx: Option<usize>,
    loop_enabled: bool,
    continue_until_stopped: bool,
    quantize_mode: QuantizeMode,
    key_shift_semitones: i32,
    current_bpm: f64,
    backing_gain: f32,
    click_gain: f32,
    click_muted: bool,
    active_click_mode: ClickMode,
    intro_click_auto_muted: bool,
    pad: PadSynth,
    percussion: PercussionEngine,
    percussion_bridge_beat: Option<f64>,
    percussion_bridge_continuous: bool,
    percussion_audition_playing: bool,
    percussion_audition_bpm: f64,
    percussion_audition_bpb: u8,
    percussion_audition_beat: f64,
    percussion_audition_with_click: bool,
    percussion_audition_last_beat: Option<i64>,
    device_name: String,
    device_channels: u16,

    // Setlist & non-blocking background preloader
    setlist_name: String,
    setlist_sections: Vec<SetlistSection>,
    setlist_items: Vec<SetlistItem>,
    active_setlist_slot: Option<usize>,
    preload_req_tx: Option<mpsc::Sender<(String, u32)>>,
    song_cache: HashMap<String, CacheEntry>,
    pending_single_dir: Option<String>,
    pending_timing_override: Option<(
        String,
        f64,
        f64,
        u8,
        Option<ClickMode>,
        Option<Vec<SectionDef>>,
    )>,
    pending_slot_switch: Option<(usize, bool, bool)>,
    pending_session_override: Option<SessionPostLoadOverrides>,
    pending_setlist_click_muted: Option<bool>,

    // Owned exclusively on the mixer thread so `AudioEngine` remains Send + Sync.
    _stream: Option<cpal::Stream>,
    producer: Producer<BusFrame>,

    click_synth: ClickSynth,
    click_delay: ClickDelayBuffer,
    pitched_stretcher: StemStretcher,
    drums_stretcher: StemStretcher,
    cues_stretcher: StemStretcher,

    pitched_in: Vec<f32>,
    drums_in: Vec<f32>,
    cues_in: Vec<f32>,
    pitched_out: Vec<f32>,
    drums_out: Vec<f32>,
    cues_out: Vec<f32>,
    crossfade_remaining: usize,
    crossfade_total: usize,
    prev_tail: Vec<BusFrame>,
    song_crossfade: CrossfadeDeck,
}

impl MixerState {
    fn new(
        song: LoadedSong,
        device_name: String,
        sample_rate: u32,
        device_channels: u16,
        stream: cpal::Stream,
        producer: Producer<BusFrame>,
        preload_req_tx: Option<mpsc::Sender<(String, u32)>>,
    ) -> Self {
        let base_bpm = song.manifest.bpm;
        let initial_click_mode = song.manifest.click_mode;
        let initial_key = song.manifest.original_key.clone();
        Self {
            song,
            active_song_dir: None,
            sample_rate,
            is_playing: false,
            source_frame_pos: 0.0,
            last_beat_index: None,
            active_section_idx: Some(0),
            armed_section_idx: None,
            loop_enabled: false,
            continue_until_stopped: false,
            quantize_mode: QuantizeMode::NextBar,
            key_shift_semitones: 0,
            current_bpm: base_bpm,
            backing_gain: 1.0,
            click_gain: 0.85,
            click_muted: initial_click_mode == ClickMode::Off,
            active_click_mode: initial_click_mode,
            intro_click_auto_muted: false,
            pad: PadSynth::new(sample_rate, &initial_key),
            percussion: PercussionEngine::new(sample_rate),
            percussion_bridge_beat: None,
            percussion_bridge_continuous: false,
            percussion_audition_playing: false,
            percussion_audition_bpm: base_bpm,
            percussion_audition_bpb: 4,
            percussion_audition_beat: 0.0,
            percussion_audition_with_click: false,
            percussion_audition_last_beat: None,
            device_name,
            device_channels,
            setlist_name: "Sunday Morning Set".into(),
            setlist_sections: Vec::new(),
            setlist_items: Vec::new(),
            active_setlist_slot: None,
            preload_req_tx,
            song_cache: HashMap::new(),
            pending_single_dir: None,
            pending_timing_override: None,
            pending_slot_switch: None,
            pending_session_override: None,
            pending_setlist_click_muted: None,
            _stream: Some(stream),
            producer,
            click_synth: ClickSynth::new(sample_rate),
            click_delay: ClickDelayBuffer::new(32768),
            pitched_stretcher: StemStretcher::new(sample_rate),
            drums_stretcher: StemStretcher::new(sample_rate),
            cues_stretcher: StemStretcher::new(sample_rate),
            pitched_in: Vec::new(),
            drums_in: Vec::new(),
            cues_in: Vec::new(),
            pitched_out: Vec::new(),
            drums_out: Vec::new(),
            cues_out: Vec::new(),
            crossfade_remaining: 0,
            crossfade_total: (sample_rate as usize / 80).max(64),
            prev_tail: Vec::new(),
            song_crossfade: CrossfadeDeck::new(sample_rate),
        }
    }

    #[cfg(test)]
    fn new_for_test(song: LoadedSong, sample_rate: u32, producer: Producer<BusFrame>) -> Self {
        let base_bpm = song.manifest.bpm;
        let initial_click_mode = song.manifest.click_mode;
        let initial_key = song.manifest.original_key.clone();
        Self {
            song,
            active_song_dir: None,
            sample_rate,
            is_playing: false,
            source_frame_pos: 0.0,
            last_beat_index: None,
            active_section_idx: Some(0),
            armed_section_idx: None,
            loop_enabled: false,
            continue_until_stopped: false,
            quantize_mode: QuantizeMode::NextBar,
            key_shift_semitones: 0,
            current_bpm: base_bpm,
            backing_gain: 1.0,
            click_gain: 0.85,
            click_muted: initial_click_mode == ClickMode::Off,
            active_click_mode: initial_click_mode,
            intro_click_auto_muted: false,
            pad: PadSynth::new(sample_rate, &initial_key),
            percussion: PercussionEngine::new(sample_rate),
            percussion_bridge_beat: None,
            percussion_bridge_continuous: false,
            percussion_audition_playing: false,
            percussion_audition_bpm: base_bpm,
            percussion_audition_bpb: 4,
            percussion_audition_beat: 0.0,
            percussion_audition_with_click: false,
            percussion_audition_last_beat: None,
            device_name: "test".into(),
            device_channels: 2,
            setlist_name: "Sunday Morning Set".into(),
            setlist_sections: Vec::new(),
            setlist_items: Vec::new(),
            active_setlist_slot: None,
            preload_req_tx: None,
            song_cache: HashMap::new(),
            pending_single_dir: None,
            pending_timing_override: None,
            pending_slot_switch: None,
            pending_session_override: None,
            pending_setlist_click_muted: None,
            _stream: None,
            producer,
            click_synth: ClickSynth::new(sample_rate),
            click_delay: ClickDelayBuffer::new(32768),
            pitched_stretcher: StemStretcher::new(sample_rate),
            drums_stretcher: StemStretcher::new(sample_rate),
            cues_stretcher: StemStretcher::new(sample_rate),
            pitched_in: Vec::new(),
            drums_in: Vec::new(),
            cues_in: Vec::new(),
            pitched_out: Vec::new(),
            drums_out: Vec::new(),
            cues_out: Vec::new(),
            crossfade_remaining: 0,
            crossfade_total: 100,
            prev_tail: Vec::new(),
            song_crossfade: CrossfadeDeck::new(sample_rate),
        }
    }

    fn has_active_audio(&self) -> bool {
        self.is_playing
            || self.percussion_audition_playing
            || self.pad.is_active()
            || self.percussion.is_active()
            || self.song_crossfade.active
    }

    fn request_preload(&mut self, dir: &str, force: bool) {
        if !force {
            if let Some(CacheEntry::Ready(_) | CacheEntry::Loading) = self.song_cache.get(dir) {
                return;
            }
        }
        self.song_cache.insert(dir.to_string(), CacheEntry::Loading);
        if let Some(ref tx) = self.preload_req_tx {
            let _ = tx.send((dir.to_string(), self.sample_rate));
        } else {
            // Synchronous fallback when constructed in headless unit tests.
            let res = load_song_from_dir(Path::new(dir), self.sample_rate)
                .map_err(|e| e.to_string());
            self.handle_preload_result(PreloadResult {
                dir: dir.to_string(),
                sample_rate: self.sample_rate,
                result: res,
            });
        }
    }

    fn request_adjacent_preloads(&mut self, slot: usize) {
        let mut dirs = Vec::new();
        if let Some(item) = self.setlist_items.get(slot) {
            if let Some(ref d) = item.song_dir {
                dirs.push(d.clone());
            }
        }
        if let Some(next_item) = self.setlist_items.get(slot + 1) {
            if let Some(ref d) = next_item.song_dir {
                dirs.push(d.clone());
            }
        }
        if slot > 0 {
            if let Some(prev_item) = self.setlist_items.get(slot - 1) {
                if let Some(ref d) = prev_item.song_dir {
                    dirs.push(d.clone());
                }
            }
        }
        for dir in dirs {
            self.request_preload(&dir, false);
        }
    }

    fn handle_preload_result(&mut self, res: PreloadResult) {
        if res.sample_rate != self.sample_rate {
            return;
        }
        match res.result {
            Ok(song) => {
                // Keep setlist slot metadata fresh when song finishes decoding.
                for item in &mut self.setlist_items {
                    if item.song_dir.as_deref() == Some(res.dir.as_str()) {
                        item.song_id = song.manifest.id.clone();
                        item.title = Some(song.manifest.title.clone());
                        item.original_key = Some(song.manifest.original_key.clone());
                        item.base_bpm = Some(song.manifest.bpm);
                        item.base_click_mode = Some(song.manifest.click_mode);
                    }
                }
                self.song_cache
                    .insert(res.dir.clone(), CacheEntry::Ready(song.clone()));

                if self.pending_single_dir.as_deref() == Some(res.dir.as_str()) {
                    self.pending_single_dir = None;
                    self.active_setlist_slot = None;
                    let timing_override = self
                        .pending_timing_override
                        .take()
                        .filter(|(dir, _, _, _, _, _)| dir == &res.dir);
                    self.apply_song_with_overrides(
                        song,
                        Some(res.dir.clone()),
                        0,
                        None,
                        None,
                        false,
                        None,
                    );
                    if let Some((_, bpm, downbeat, bpb, click_mode, sections)) = timing_override
                    {
                        self.apply_song_timing(bpm, downbeat, bpb, click_mode, sections);
                    }
                } else if self.active_song_dir.as_deref() == Some(res.dir.as_str())
                    && self.active_setlist_slot.is_some()
                    && self.pending_slot_switch.is_none()
                    && !self.is_playing
                {
                    // Refresh active song if its manifest was edited and saved.
                    if let Some(slot) = self.active_setlist_slot {
                        self.activate_cached_slot(slot, false, false);
                    }
                }

                if let Some((target_slot, autoplay, engage_pad)) = self.pending_slot_switch {
                    let target_dir = self
                        .setlist_items
                        .get(target_slot)
                        .and_then(|item| item.song_dir.as_deref());
                    if target_dir == Some(res.dir.as_str()) {
                        self.pending_slot_switch = None;
                        self.activate_cached_slot(target_slot, autoplay, engage_pad);
                    }
                }
            }
            Err(err) => {
                log::error!("Failed to preload song at {}: {err}", res.dir);
                self.song_cache.insert(res.dir.clone(), CacheEntry::Error(err));
                if self.pending_single_dir.as_deref() == Some(res.dir.as_str()) {
                    self.pending_single_dir = None;
                    self.pending_session_override = None;
                }
                if let Some((target_slot, _, _)) = self.pending_slot_switch {
                    let target_dir = self
                        .setlist_items
                        .get(target_slot)
                        .and_then(|item| item.song_dir.as_deref());
                    if target_dir == Some(res.dir.as_str()) {
                        self.pending_slot_switch = None;
                        self.pending_session_override = None;
                    }
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: EngineCommand, shared_routing: &Arc<RwLock<SharedRouting>>) {
        match cmd {
            EngineCommand::LoadDemoSong => {
                self.pending_single_dir = None;
                self.pending_timing_override = None;
                self.pending_slot_switch = None;
                self.pending_setlist_click_muted = None;
                self.active_setlist_slot = None;
                self.percussion_bridge_beat = None;
                self.percussion_bridge_continuous = false;
                let demo = generate_demo_song(self.sample_rate);
                self.apply_song_with_overrides(demo, None, 0, None, None, false, None);
            }
            EngineCommand::LoadSongDir(path) => {
                self.pending_slot_switch = None;
                self.pending_session_override = None;
                self.pending_setlist_click_muted = None;
                self.pending_single_dir = Some(path.clone());
                self.pending_timing_override = None;
                self.percussion_bridge_beat = None;
                self.percussion_bridge_continuous = false;
                self.song_cache.remove(&path);
                self.request_preload(&path, true);
            }
            EngineCommand::Play => {
                self.percussion_audition_playing = false;
                if !self.is_playing {
                    self.is_playing = true;
                    self.last_beat_index = None;
                    self.click_delay.clear();
                    self.percussion_bridge_beat = None;
                    self.percussion_bridge_continuous = false;
                    self.percussion.reset_transport();
                    self.prime_stretchers();
                }
            }
            EngineCommand::Stop => {
                self.is_playing = false;
                self.source_frame_pos = 0.0;
                self.last_beat_index = None;
                self.click_delay.clear();
                self.percussion_bridge_beat = None;
                self.percussion_bridge_continuous = false;
                self.percussion.reset_transport();
                self.pad.enabled = false;
                self.song_crossfade.clear();
                self.reset_intro_click_if_needed();
                self.armed_section_idx = None;
                self.active_section_idx = if self.song.manifest.sections.is_empty() {
                    None
                } else {
                    Some(0)
                };
            }
            EngineCommand::PlayStop => {
                if self.is_playing {
                    self.handle_command(EngineCommand::Stop, shared_routing);
                } else {
                    self.handle_command(EngineCommand::Play, shared_routing);
                }
            }
            EngineCommand::SeekSec(sec) => {
                let max_sec = self.song.total_frames as f64 / self.sample_rate as f64;
                let clamped_sec = sec.clamp(0.0, max_sec);
                self.source_frame_pos = clamped_sec * self.sample_rate as f64;
                self.last_beat_index = None;
                self.click_delay.clear();
                self.percussion_bridge_beat = None;
                self.percussion_bridge_continuous = false;
                self.percussion.reset_transport();
                self.reset_intro_click_if_needed();
                self.armed_section_idx = None;
                let (bar, _, _) = self.song.manifest.seconds_to_bar_beat(clamped_sec);
                self.active_section_idx = self
                    .song
                    .manifest
                    .sections
                    .iter()
                    .position(|s| self.song.manifest.section_contains_at(s, clamped_sec, bar))
                    .or_else(|| {
                        if self.song.manifest.sections.is_empty() {
                            None
                        } else {
                            Some(0)
                        }
                    });
                if self.percussion.follow_sections {
                    if let Some(sec) = self
                        .active_section_idx
                        .and_then(|idx| self.song.manifest.sections.get(idx))
                    {
                        self.percussion.intensity = section_intensity_from_name(&sec.name);
                    }
                }
                if self.is_playing {
                    self.prime_stretchers();
                }
            }
            EngineCommand::QueueSectionById(id) => {
                if let Some(idx) = self.song.manifest.sections.iter().position(|s| s.id == id) {
                    self.arm_or_jump_section(idx);
                }
            }
            EngineCommand::QueueSectionBySlot(slot) => {
                if slot < self.song.manifest.sections.len() {
                    self.arm_or_jump_section(slot);
                }
            }
            EngineCommand::QueueSectionByName(name) => {
                let target = name.to_lowercase();
                if let Some(idx) = self
                    .song
                    .manifest
                    .sections
                    .iter()
                    .position(|s| s.name.to_lowercase().contains(&target))
                {
                    self.arm_or_jump_section(idx);
                }
            }
            EngineCommand::NextSection => {
                let len = self.song.manifest.sections.len();
                if len > 0 {
                    let cur = self.active_section_idx.unwrap_or(0);
                    let next = (cur + 1).min(len - 1);
                    self.arm_or_jump_section(next);
                }
            }
            EngineCommand::PrevSection => {
                if !self.song.manifest.sections.is_empty() {
                    let cur = self.active_section_idx.unwrap_or(0);
                    let prev = cur.saturating_sub(1);
                    self.arm_or_jump_section(prev);
                }
            }
            EngineCommand::ToggleLoop => {
                self.loop_enabled = !self.loop_enabled;
            }
            EngineCommand::SetLoop(val) => {
                self.loop_enabled = val;
            }
            EngineCommand::ToggleContinueTrack => {
                self.continue_until_stopped = !self.continue_until_stopped;
            }
            EngineCommand::SetContinueTrack(val) => {
                self.continue_until_stopped = val;
            }
            EngineCommand::SetQuantizeMode(q) => {
                self.quantize_mode = q;
            }
            EngineCommand::SetKeyShift(semi) => {
                let was_bypassed = self.is_stretch_bypassed();
                let prev_rate = self.tempo_ratio();
                self.key_shift_semitones = semi.clamp(-12, 12);
                self.pitched_stretcher
                    .set_semitones(self.key_shift_semitones as f32);
                self.sync_stretch_transition(was_bypassed, prev_rate);
                self.sync_active_slot_overrides();
                self.update_pad_key_from_song();
            }
            EngineCommand::AdjustKeyShift(delta) => {
                let was_bypassed = self.is_stretch_bypassed();
                let prev_rate = self.tempo_ratio();
                let next = (self.key_shift_semitones + delta).clamp(-12, 12);
                self.key_shift_semitones = next;
                self.pitched_stretcher.set_semitones(next as f32);
                self.sync_stretch_transition(was_bypassed, prev_rate);
                self.sync_active_slot_overrides();
                self.update_pad_key_from_song();
            }
            EngineCommand::SetBpm(bpm) => {
                let was_bypassed = self.is_stretch_bypassed();
                let prev_rate = self.tempo_ratio();
                self.current_bpm = bpm.clamp(30.0, 260.0);
                self.sync_stretch_transition(was_bypassed, prev_rate);
                self.sync_active_slot_overrides();
                self.sync_audition_tempo_from_song();
            }
            EngineCommand::AdjustBpm(delta) => {
                let was_bypassed = self.is_stretch_bypassed();
                let prev_rate = self.tempo_ratio();
                self.current_bpm = (self.current_bpm + delta).clamp(30.0, 260.0);
                self.sync_stretch_transition(was_bypassed, prev_rate);
                self.sync_active_slot_overrides();
                self.sync_audition_tempo_from_song();
            }
            EngineCommand::SetSongTiming {
                bpm,
                first_downbeat_sec,
                beats_per_bar,
                click_mode,
                sections,
            } => {
                if let Some(ref pending_dir) = self.pending_single_dir {
                    self.pending_timing_override = Some((
                        pending_dir.clone(),
                        bpm,
                        first_downbeat_sec,
                        beats_per_bar,
                        click_mode,
                        sections,
                    ));
                } else {
                    self.apply_song_timing(
                        bpm,
                        first_downbeat_sec,
                        beats_per_bar,
                        click_mode,
                        sections,
                    );
                }
            }
            EngineCommand::SetMasterGain(gain) => {
                if let Ok(mut r) = shared_routing.write() {
                    r.master_gain = gain.clamp(0.0, 1.5);
                }
            }
            EngineCommand::SetBackingGain(gain) => {
                self.backing_gain = gain.clamp(0.0, 1.5);
            }
            EngineCommand::SetClickGain(gain) => {
                self.click_gain = gain.clamp(0.0, 1.5);
            }
            EngineCommand::SetClickMuted(muted) => {
                self.pending_setlist_click_muted = None;
                self.click_muted = muted;
                if !muted && self.active_click_mode == ClickMode::IntroOnly {
                    let cur_src_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
                    let intro_end_sec = self.song.manifest.intro_end_seconds();
                    self.intro_click_auto_muted = cur_src_sec >= intro_end_sec;
                } else if muted && self.active_click_mode != ClickMode::IntroOnly {
                    self.intro_click_auto_muted = false;
                }
            }
            EngineCommand::ToggleClickMuted => {
                let next_muted = !self.click_muted;
                self.handle_command(EngineCommand::SetClickMuted(next_muted), shared_routing);
            }
            EngineCommand::SetClickMode(mode) => {
                self.pending_setlist_click_muted = None;
                self.active_click_mode = mode;
                self.apply_click_mode_state();
                self.sync_active_slot_overrides();
            }
            EngineCommand::SetStemGain { name, gain } => {
                if let Some(stem) = self.song.stems.iter_mut().find(|s| s.name == name) {
                    stem.gain = gain.clamp(0.0, 1.5);
                }
                self.sync_active_stem_mix();
            }
            EngineCommand::SetStemMuted { name, muted } => {
                if let Some(stem) = self.song.stems.iter_mut().find(|s| s.name == name) {
                    stem.muted = muted;
                }
                self.sync_active_stem_mix();
            }
            EngineCommand::SetPadEnabled(enabled) => {
                self.pending_session_override = None;
                self.pad.enabled = enabled;
                if enabled && self.pad.follow_song_key {
                    self.update_pad_key_from_song();
                }
            }
            EngineCommand::SetPadMuted(muted) => {
                self.pad.muted = muted;
            }
            EngineCommand::TogglePad => {
                self.pad.enabled = !self.pad.enabled;
                if self.pad.enabled {
                    self.pad.muted = false;
                    if self.pad.follow_song_key {
                        self.update_pad_key_from_song();
                    }
                }
            }
            EngineCommand::SetPadGain(gain) => {
                self.pad.gain = gain.clamp(0.0, 1.5);
            }
            EngineCommand::SetPadKey(key) => {
                self.pad.follow_song_key = false;
                self.pad.set_key(&key);
            }
            EngineCommand::AdjustPadKey(delta) => {
                self.pad.follow_song_key = false;
                let next = transpose_pad_key(self.pad.current_key(), delta);
                self.pad.set_key(&next);
            }
            EngineCommand::SetPadFollowSongKey(follow) => {
                self.pad.follow_song_key = follow;
                if follow {
                    self.update_pad_key_from_song();
                }
            }
            EngineCommand::SetPadCrossfadeSec(sec) => {
                self.pad.set_crossfade_sec(sec);
            }
            EngineCommand::SetPadPatch(patch) => {
                self.pad.apply_patch(patch);
            }
            EngineCommand::SetPadLayer { index, layer } => {
                self.pad.set_layer(index, layer);
            }
            EngineCommand::SetPadLayerGain { index, gain } => {
                self.pad.set_layer_gain(index, gain);
            }
            EngineCommand::SetPadBrightness(val) => {
                self.pad.set_brightness(val);
            }
            EngineCommand::SetPadShimmer(val) => {
                self.pad.set_shimmer(val);
            }
            EngineCommand::SetPadPan(val) => {
                self.pad.set_pan(val);
            }
            EngineCommand::LockCurrentPadToSlot(slot) => {
                let lock = PadLockConfig {
                    enabled: self.pad.enabled && !self.pad.muted,
                    gain: self.pad.gain,
                    preset_id: self.pad.preset_id().map(|s| s.to_string()),
                    preset_name: Some(self.pad.preset_name().to_string()),
                    layers: self.pad.layers().to_vec(),
                    brightness: self.pad.brightness,
                    shimmer: self.pad.shimmer,
                    pan: self.pad.pan,
                };
                if let Some(item) = self.setlist_items.get_mut(slot) {
                    item.pad_lock = Some(lock);
                }
            }
            EngineCommand::ClearSlotPadLock(slot) => {
                if let Some(item) = self.setlist_items.get_mut(slot) {
                    item.pad_lock = None;
                }
            }
            EngineCommand::AddSetlistSong { dir, insert_at } => {
                let manifest_opt = SongManifest::from_file(&Path::new(&dir).join("manifest.json")).ok();
                let fallback_title = Path::new(&dir)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("Song")
                    .to_string();
                let item = SetlistItem {
                    song_id: manifest_opt
                        .as_ref()
                        .map(|m| m.id.clone())
                        .unwrap_or_else(|| fallback_title.to_lowercase().replace(' ', "-")),
                    song_dir: Some(dir.clone()),
                    title: Some(
                        manifest_opt
                            .as_ref()
                            .map(|m| m.title.clone())
                            .unwrap_or(fallback_title),
                    ),
                    original_key: manifest_opt.as_ref().map(|m| m.original_key.clone()),
                    base_bpm: manifest_opt.as_ref().map(|m| m.bpm),
                    base_click_mode: manifest_opt.as_ref().map(|m| m.click_mode),
                    key_shift_semitones: 0,
                    bpm_override: None,
                    click_mode: None,
                    transition_mode: TransitionMode::AutoPause,
                    crossfade_sec: 2.0,
                    ambient_pad_enabled: true,
                    pad_key_override: None,
                    pad_lock: None,
                    ..Default::default()
                };
                let idx = insert_at
                    .unwrap_or(self.setlist_items.len())
                    .min(self.setlist_items.len());
                self.setlist_items.insert(idx, item);
                if let Some(active) = self.active_setlist_slot {
                    if idx <= active {
                        self.active_setlist_slot = Some(active + 1);
                    }
                }
                self.request_preload(&dir, false);
                if self.setlist_items.len() == 1 && self.active_setlist_slot.is_none() {
                    self.select_setlist_slot(0, false);
                } else if let Some(active) = self.active_setlist_slot {
                    self.request_adjacent_preloads(active);
                }
            }
            EngineCommand::RemoveSetlistSlot(slot) => {
                if slot < self.setlist_items.len() {
                    self.setlist_items.remove(slot);
                    if let Some(active) = self.active_setlist_slot {
                        if active == slot {
                            if self.setlist_items.is_empty() {
                                self.active_setlist_slot = None;
                            } else {
                                let next = slot.min(self.setlist_items.len() - 1);
                                self.select_setlist_slot(next, false);
                            }
                        } else if slot < active {
                            self.active_setlist_slot = Some(active - 1);
                        }
                    }
                }
            }
            EngineCommand::MoveSetlistSlot { from, to } => {
                let len = self.setlist_items.len();
                if from < len && to < len && from != to {
                    let item = self.setlist_items.remove(from);
                    self.setlist_items.insert(to, item);
                    if let Some(active) = self.active_setlist_slot {
                        self.active_setlist_slot = Some(if active == from {
                            to
                        } else if from < active && to >= active {
                            active - 1
                        } else if from > active && to <= active {
                            active + 1
                        } else {
                            active
                        });
                    }
                    if let Some(active) = self.active_setlist_slot {
                        self.request_adjacent_preloads(active);
                    }
                }
            }
            EngineCommand::SelectSetlistSlot(slot) => {
                self.pending_session_override = None;
                let autoplay = self.is_playing;
                if self.active_setlist_slot == Some(slot) && autoplay {
                    return;
                }
                self.select_setlist_slot(slot, autoplay);
            }
            EngineCommand::NextSong => {
                if !self.setlist_items.is_empty() {
                    let cur = self.active_setlist_slot;
                    let next = match cur {
                        Some(c) => (c + 1).min(self.setlist_items.len() - 1),
                        None => 0,
                    };
                    if cur != Some(next) {
                        let autoplay = self.is_playing;
                        self.select_setlist_slot(next, autoplay);
                    }
                }
            }
            EngineCommand::PrevSong => {
                if !self.setlist_items.is_empty() {
                    let cur = self.active_setlist_slot;
                    let prev = cur.unwrap_or(0).saturating_sub(1);
                    if cur != Some(prev) {
                        let autoplay = self.is_playing;
                        self.select_setlist_slot(prev, autoplay);
                    }
                }
            }
            EngineCommand::SetSetlistName(name) => {
                if !name.trim().is_empty() {
                    self.setlist_name = name.trim().to_string();
                }
            }
            EngineCommand::UpdateSetlistSlotConfig {
                slot,
                key_shift_semitones,
                bpm_override,
                transition_mode,
                crossfade_sec,
                ambient_pad_enabled,
                pad_key_override,
                pad_lock,
                click_mode,
                section_id,
                percussion_lock,
            } => {
                if let Some(item) = self.setlist_items.get_mut(slot) {
                    if let Some(k) = key_shift_semitones {
                        item.key_shift_semitones = k.clamp(-12, 12);
                    }
                    if let Some(bpm_opt) = bpm_override {
                        item.bpm_override = bpm_opt.map(|b| b.clamp(30.0, 260.0));
                    }
                    if let Some(tm) = transition_mode {
                        item.transition_mode = tm;
                    }
                    if let Some(cf) = crossfade_sec {
                        item.crossfade_sec = cf.clamp(0.0, 10.0);
                    }
                    if let Some(pad_on) = ambient_pad_enabled {
                        item.ambient_pad_enabled = pad_on;
                    }
                    if let Some(pad_k) = pad_key_override {
                        item.pad_key_override = pad_k;
                    }
                    let mut pad_lock_changed = false;
                    if let Some(lock_opt) = pad_lock {
                        item.pad_lock = lock_opt;
                        pad_lock_changed = true;
                    }
                    if let Some(cm_opt) = click_mode {
                        item.click_mode = cm_opt;
                    }
                    let assigned_sec = match section_id {
                        Some(ref sec_opt) => sec_opt.clone(),
                        None => None,
                    };
                    if let Some(sec_opt) = section_id {
                        item.section_id = sec_opt;
                    }
                    let mut perc_lock_changed = false;
                    if let Some(perc_opt) = percussion_lock {
                        item.percussion_lock = perc_opt;
                        perc_lock_changed = true;
                    }
                    let applied_lock = item.pad_lock.clone();
                    let applied_perc = item.percussion_lock.clone();
                    let updated_key_shift = item.key_shift_semitones;
                    let updated_bpm_override = item.bpm_override;
                    let updated_click_mode = item.click_mode;
                    if self.active_setlist_slot == Some(slot) {
                        let was_bypassed = self.is_stretch_bypassed();
                        let prev_rate = self.tempo_ratio();
                        self.key_shift_semitones = updated_key_shift;
                        self.pitched_stretcher
                            .set_semitones(self.key_shift_semitones as f32);
                        self.current_bpm = updated_bpm_override.unwrap_or(self.song.manifest.bpm);
                        self.sync_stretch_transition(was_bypassed, prev_rate);
                        self.sync_audition_tempo_from_song();
                        if click_mode.is_some() {
                            self.active_click_mode =
                                updated_click_mode.unwrap_or(self.song.manifest.click_mode);
                            self.apply_click_mode_state();
                        }
                        if pad_lock_changed {
                            if let Some(lock) = applied_lock {
                                self.apply_pad_lock(&lock);
                            }
                        }
                        if perc_lock_changed {
                            if let Some(lock) = applied_perc {
                                self.apply_percussion_lock_and_kit(&lock);
                            }
                        }
                        self.update_pad_key_from_song();
                    }
                    if let Some(ref sid) = assigned_sec {
                        if transition_mode.is_none() {
                            self.apply_section_transition_defaults(sid);
                        }
                    }
                }
            }
            EngineCommand::BatchUpdateSetlistSlots(patch) => {
                let mut touched_active = false;
                for &slot in &patch.slots {
                    let Some(item) = self.setlist_items.get_mut(slot) else {
                        continue;
                    };
                    if let Some(ref target_key) = patch.target_key {
                        if target_key.eq_ignore_ascii_case("original") {
                            item.key_shift_semitones = 0;
                        } else if let Some(ref orig) = item.original_key {
                            if let Ok(shift) = compute_semitone_shift(orig, target_key) {
                                item.key_shift_semitones = shift.clamp(-12, 12);
                            }
                        }
                    } else if let Some(k) = patch.key_shift_semitones {
                        item.key_shift_semitones = k.clamp(-12, 12);
                    }
                    if let Some(bpm_opt) = patch.bpm_override {
                        item.bpm_override = bpm_opt.map(|b| b.clamp(30.0, 260.0));
                    }
                    if let Some(cm_opt) = patch.click_mode {
                        item.click_mode = cm_opt;
                    }
                    if let Some(tm) = patch.transition_mode {
                        item.transition_mode = tm;
                    }
                    if let Some(cf) = patch.crossfade_sec {
                        item.crossfade_sec = cf.clamp(0.0, 10.0);
                    }
                    if let Some(pad_on) = patch.ambient_pad_enabled {
                        item.ambient_pad_enabled = pad_on;
                    }
                    if let Some(ref lock_opt) = patch.pad_lock {
                        item.pad_lock = lock_opt.clone();
                    }
                    if let Some(ref sec_opt) = patch.section_id {
                        item.section_id = sec_opt.clone();
                    }
                    if patch.percussion_mode.is_some()
                        || patch.percussion_style.is_some()
                        || patch.percussion_intensity.is_some()
                        || patch.percussion_custom_pattern.is_some()
                    {
                        let mut lock = item
                            .percussion_lock
                            .clone()
                            .unwrap_or_default();
                        if let Some(m) = patch.percussion_mode {
                            lock.mode = m;
                        }
                        if let Some(s) = patch.percussion_style {
                            lock.style = s;
                        }
                        if let Some(i) = patch.percussion_intensity {
                            lock.intensity = i;
                        }
                        if let Some(ref custom_opt) = patch.percussion_custom_pattern {
                            lock.custom_pattern =
                                custom_opt.as_ref().map(|p| p.clone().clamped_and_sorted());
                            if custom_opt.is_some()
                                && patch.percussion_mode.is_none()
                                && lock.mode == PercussionMode::Off
                            {
                                lock.mode = PercussionMode::PercussionOnly;
                            }
                        }
                        item.percussion_lock = Some(lock);
                    }
                    if self.active_setlist_slot == Some(slot) {
                        touched_active = true;
                    }
                }
                if touched_active {
                    if let Some(slot) = self.active_setlist_slot {
                        if let Some(item) = self.setlist_items.get(slot).cloned() {
                            let was_bypassed = self.is_stretch_bypassed();
                            let prev_rate = self.tempo_ratio();
                            self.key_shift_semitones = item.key_shift_semitones;
                            self.pitched_stretcher
                                .set_semitones(self.key_shift_semitones as f32);
                            self.current_bpm = item.bpm_override.unwrap_or(self.song.manifest.bpm);
                            self.sync_stretch_transition(was_bypassed, prev_rate);
                            self.sync_audition_tempo_from_song();
                            if patch.click_mode.is_some() {
                                self.active_click_mode =
                                    item.click_mode.unwrap_or(self.song.manifest.click_mode);
                                self.apply_click_mode_state();
                            }
                            if let Some(pad_on) = patch.ambient_pad_enabled {
                                self.pad.enabled = pad_on;
                                if pad_on {
                                    self.pad.muted = false;
                                }
                            }
                            if let Some(Some(ref lock)) = patch.pad_lock {
                                self.apply_pad_lock(lock);
                            }
                            if let Some(ref perc_lock) = item.percussion_lock {
                                if patch.percussion_mode.is_some()
                                    || patch.percussion_style.is_some()
                                    || patch.percussion_intensity.is_some()
                                    || patch.percussion_custom_pattern.is_some()
                                {
                                    self.apply_percussion_lock_and_kit(perc_lock);
                                }
                            }
                            self.update_pad_key_from_song();
                        }
                    }
                }
                if let Some(Some(ref sid)) = patch.section_id {
                    if patch.transition_mode.is_none() {
                        self.apply_section_transition_defaults(sid);
                    }
                }
            }
            EngineCommand::UpsertSetlistSection(sec) => {
                if sec.id.trim().is_empty() || sec.name.trim().is_empty() {
                    return;
                }
                if let Some(existing) = self.setlist_sections.iter_mut().find(|s| s.id == sec.id) {
                    *existing = sec;
                } else {
                    self.setlist_sections.push(sec);
                }
            }
            EngineCommand::RemoveSetlistSection(sec_id) => {
                self.setlist_sections.retain(|s| s.id != sec_id);
                for item in &mut self.setlist_items {
                    if item.section_id.as_deref() == Some(sec_id.as_str()) {
                        item.section_id = None;
                    }
                }
            }
            EngineCommand::MoveSetlistSection { id, to_index } => {
                if let Some(from) = self.setlist_sections.iter().position(|s| s.id == id) {
                    let target = to_index.min(self.setlist_sections.len().saturating_sub(1));
                    if from != target {
                        let sec = self.setlist_sections.remove(from);
                        self.setlist_sections.insert(target, sec);
                    }
                }
            }
            EngineCommand::ToggleSetlistSectionCollapsed { id, collapsed } => {
                if let Some(sec) = self.setlist_sections.iter_mut().find(|s| s.id == id) {
                    sec.collapsed = collapsed;
                }
            }
            EngineCommand::AddStandaloneSetlistSlot {
                title,
                slot_kind,
                key,
                bpm,
                time_signature,
                section_id,
                insert_at,
            } => {
                let clean_title = if title.trim().is_empty() {
                    match slot_kind {
                        SetlistSlotKind::PadOnly => "Pad Underscore".to_string(),
                        _ => "Percussion Groove".to_string(),
                    }
                } else {
                    title.trim().to_string()
                };
                let slug = clean_title
                    .to_ascii_lowercase()
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                    .collect::<String>()
                    .trim_matches('-')
                    .to_string();
                let safe_bpm = if bpm.is_finite() {
                    bpm.clamp(30.0, 260.0)
                } else {
                    80.0
                };
                let bpb = time_signature[0].clamp(1, 16);
                let unit = time_signature[1].clamp(1, 16);
                let percussion_lock = if slot_kind == SetlistSlotKind::PercussionGroove {
                    Some(PercussionLockConfig {
                        mode: PercussionMode::PercussionOnly,
                        style: if bpb == 6 || bpb == 12 {
                            PercussionStyle::Worship68
                        } else {
                            PercussionStyle::Worship44
                        },
                        ..Default::default()
                    })
                } else {
                    None
                };
                let item = SetlistItem {
                    song_id: if slug.is_empty() {
                        format!("slot-{}", self.setlist_items.len() + 1)
                    } else {
                        slug
                    },
                    song_dir: None,
                    title: Some(clean_title),
                    original_key: Some(if key.trim().is_empty() {
                        "C".to_string()
                    } else {
                        key.trim().to_string()
                    }),
                    base_bpm: Some(safe_bpm),
                    base_click_mode: Some(if slot_kind == SetlistSlotKind::PadOnly {
                        ClickMode::Off
                    } else {
                        ClickMode::AlwaysOn
                    }),
                    transition_mode: TransitionMode::Manual,
                    crossfade_sec: 2.0,
                    ambient_pad_enabled: true,
                    section_id,
                    slot_kind,
                    time_signature: Some([bpb, unit]),
                    percussion_lock,
                    ..Default::default()
                };
                let idx = insert_at
                    .unwrap_or(self.setlist_items.len())
                    .min(self.setlist_items.len());
                self.setlist_items.insert(idx, item);
                if let Some(active) = self.active_setlist_slot {
                    if idx <= active {
                        self.active_setlist_slot = Some(active + 1);
                    }
                }
                if self.setlist_items.len() == 1 && self.active_setlist_slot.is_none() {
                    self.select_setlist_slot(0, false);
                }
            }
            EngineCommand::SetPercussionMode(mode) => {
                self.percussion.mode = mode;
                if mode == PercussionMode::Off {
                    self.percussion.reset_transport();
                } else if self.is_playing {
                    let cur_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
                    if let Some(beat) = self.song.manifest.seconds_to_continuous_beat(cur_sec) {
                        let bpb = self.song.manifest.beats_per_bar();
                        let steps_per_bar = self.percussion.active_steps_per_bar();
                        let step = ((beat / bpb.max(1) as f64) * steps_per_bar as f64)
                            .floor() as i64;
                        // Avoid burst-triggering an already passed step
                        let _ = step;
                    }
                }
                self.sync_active_percussion_lock();
            }
            EngineCommand::SetPercussionStyle(style) => {
                self.percussion.style = style;
                self.percussion.set_custom_pattern(None);
                self.sync_active_percussion_lock();
            }
            EngineCommand::SetPercussionIntensity(intensity) => {
                self.percussion.intensity = intensity;
                self.sync_active_percussion_lock();
            }
            EngineCommand::SetPercussionFollowSections(follow) => {
                self.percussion.follow_sections = follow;
                if follow {
                    if let Some(idx) = self.active_section_idx {
                        if let Some(sec) = self.song.manifest.sections.get(idx) {
                            self.percussion.intensity = section_intensity_from_name(&sec.name);
                        }
                    }
                }
                self.sync_active_percussion_lock();
            }
            EngineCommand::SetPercussionGain(gain) => {
                self.percussion.gain = gain.clamp(0.0, 1.5);
                self.sync_active_percussion_lock();
            }
            EngineCommand::SetPercussionIemSend(send) => {
                self.percussion.iem_send = send.clamp(0.0, 1.0);
                self.sync_active_percussion_lock();
            }
            EngineCommand::SetPercussionRoom(room) => {
                self.percussion.room_mix = if room.is_finite() {
                    room.clamp(0.0, 1.0)
                } else {
                    DEFAULT_PERCUSSION_ROOM_MIX
                };
                self.sync_active_percussion_lock();
            }
            EngineCommand::SetCustomPercussionPattern(pattern) => {
                self.percussion.set_custom_pattern(pattern);
                self.sync_active_percussion_lock();
            }
            EngineCommand::LoadPercussionKitDir(dir_opt) => {
                self.load_percussion_kit_dir(dir_opt);
                self.sync_active_percussion_lock();
            }
            EngineCommand::TriggerPercussionPreview {
                instrument,
                variant,
                velocity,
            } => {
                self.percussion.trigger_preview(instrument, variant, velocity);
            }
            EngineCommand::StartPercussionAudition {
                bpm,
                beats_per_bar,
                with_click,
            } => {
                if self.is_playing {
                    self.handle_command(EngineCommand::Stop, shared_routing);
                }
                self.percussion_audition_playing = true;
                self.percussion_audition_bpm = if bpm.is_finite() {
                    bpm.clamp(40.0, 240.0)
                } else {
                    self.current_bpm.clamp(40.0, 240.0)
                };
                self.percussion_audition_bpb = beats_per_bar.clamp(2, 16);
                self.percussion_audition_beat = 0.0;
                self.percussion_audition_with_click = with_click;
                self.percussion_audition_last_beat = None;
                self.percussion.reset_transport();
            }
            EngineCommand::StopPercussionAudition => {
                self.percussion_audition_playing = false;
                self.percussion_audition_last_beat = None;
                self.percussion.reset_step_cursor();
            }
            EngineCommand::SetPercussionAuditionBpm(bpm) => {
                if bpm.is_finite() {
                    self.percussion_audition_bpm = bpm.clamp(40.0, 240.0);
                }
            }
            EngineCommand::SetPercussionChannel {
                instrument,
                enabled,
                gain,
                pan,
            } => {
                self.percussion.set_channel(instrument, enabled, gain, pan);
                self.sync_active_percussion_lock();
            }
            EngineCommand::ResetPercussionMix => {
                self.percussion.reset_mix();
                self.sync_active_percussion_lock();
            }
            EngineCommand::LockCurrentPercussionToSlot(slot) => {
                let lock = self.current_percussion_lock();
                if let Some(item) = self.setlist_items.get_mut(slot) {
                    item.percussion_lock = Some(lock);
                }
            }
            EngineCommand::ClearSlotPercussionLock(slot) => {
                if let Some(item) = self.setlist_items.get_mut(slot) {
                    item.percussion_lock = None;
                }
            }
            EngineCommand::LoadSetlist(setlist) => {
                self.pending_single_dir = None;
                self.pending_slot_switch = None;
                self.pending_session_override = None;
                self.song_crossfade.clear();
                self.setlist_name = setlist.name;
                self.setlist_sections = setlist.sections;
                self.setlist_items = setlist.items;
                self.active_setlist_slot = None;
                if let Some(ref mixer) = setlist.mixer {
                    if mixer.master_gain.is_finite() {
                        if let Ok(mut r) = shared_routing.write() {
                            r.master_gain = mixer.master_gain.clamp(0.0, 1.5);
                        }
                    }
                    if mixer.backing_gain.is_finite() {
                        self.backing_gain = mixer.backing_gain.clamp(0.0, 1.5);
                    }
                    if mixer.click_gain.is_finite() {
                        self.click_gain = mixer.click_gain.clamp(0.0, 1.5);
                    }
                    self.pending_setlist_click_muted =
                        mixer.click_muted.filter(|&muted| muted);
                } else {
                    self.pending_setlist_click_muted = None;
                }
                let dirs: Vec<String> = self
                    .setlist_items
                    .iter()
                    .filter_map(|i| i.song_dir.clone())
                    .collect();
                for dir in dirs {
                    self.request_preload(&dir, false);
                }
                if !self.setlist_items.is_empty() {
                    self.select_setlist_slot(0, false);
                } else if let Some(true) = self.pending_setlist_click_muted.take() {
                    self.click_muted = true;
                }
            }
            EngineCommand::ClearSetlist => {
                self.pending_session_override = None;
                self.pending_setlist_click_muted = None;
                self.setlist_sections.clear();
                self.setlist_items.clear();
                self.active_setlist_slot = None;
                self.pending_slot_switch = None;
                self.song_crossfade.clear();
            }
            EngineCommand::SetRoutingMode(mode) => {
                self.apply_routing_mode(mode, shared_routing);
            }
            EngineCommand::SelectOutputDevice(name) => {
                self.switch_output_device(Some(&name), shared_routing);
            }
            EngineCommand::RestoreSession(session) => {
                self.restore_session(session, shared_routing);
            }
        }
    }

    fn apply_routing_mode(
        &mut self,
        mode: OutputRoutingMode,
        shared_routing: &Arc<RwLock<SharedRouting>>,
    ) {
        if let Ok(mut r) = shared_routing.write() {
            r.mode = mode;
        }
        if self._stream.is_some() && mode.is_multichannel() && self.device_channels < 4 {
            let can_upgrade = find_output_device(Some(&self.device_name))
                .ok()
                .and_then(|dev| {
                    let def = dev.default_output_config().ok()?;
                    find_multichannel_output_config(&dev, &def)
                })
                .is_some();
            if can_upgrade {
                let name = self.device_name.clone();
                self.switch_output_device(Some(&name), shared_routing);
            }
        }
    }

    fn restore_session(
        &mut self,
        session: StageSessionState,
        shared_routing: &Arc<RwLock<SharedRouting>>,
    ) {
        self.apply_routing_mode(session.routing_mode, shared_routing);
        if let Ok(mut r) = shared_routing.write() {
            if session.master_gain.is_finite() {
                r.master_gain = session.master_gain.clamp(0.0, 1.5);
            }
        }
        if session.backing_gain.is_finite() {
            self.backing_gain = session.backing_gain.clamp(0.0, 1.5);
        }
        if session.click_gain.is_finite() {
            self.click_gain = session.click_gain.clamp(0.0, 1.5);
        }
        self.loop_enabled = session.loop_enabled;
        self.continue_until_stopped = session.continue_until_stopped;
        self.quantize_mode = session.quantize_mode;
        self.active_click_mode = session.click_mode;
        self.apply_click_mode_state();
        self.click_muted = session.click_muted;

        // Restore Tonic Pad parameters (never auto-engage drone sound on cold launch).
        if !session.pad_layers.is_empty() {
            self.pad.apply_patch(PadPatchSpec {
                preset_id: session.pad_preset_id.clone(),
                preset_name: session.pad_preset_name.clone(),
                layers: session.pad_layers.clone(),
                brightness: session.pad_brightness,
                shimmer: session.pad_shimmer,
                pan: session.pad_pan,
                crossfade_sec: Some(session.pad_crossfade_sec),
            });
        } else {
            self.pad.set_brightness(session.pad_brightness);
            self.pad.set_shimmer(session.pad_shimmer);
            self.pad.set_pan(session.pad_pan);
            self.pad.set_crossfade_sec(session.pad_crossfade_sec);
        }
        if session.pad_gain.is_finite() {
            self.pad.gain = session.pad_gain.clamp(0.0, 1.5);
        }
        self.pad.muted = session.pad_muted;
        self.pad.enabled = false;
        self.pad.follow_song_key = session.pad_follow_song_key;
        let pad_key_label = session.pad_key_label();
        self.pad.set_key(&pad_key_label);

        // Restore Percussion Track parameters.
        self.percussion.mode = session.percussion_mode;
        self.percussion.style = session.percussion_style;
        self.percussion.intensity = session.percussion_intensity;
        self.percussion.follow_sections = session.percussion_follow_sections;
        if session.percussion_gain.is_finite() {
            self.percussion.gain = session.percussion_gain.clamp(0.0, 1.5);
        }
        if session.percussion_iem_send.is_finite() {
            self.percussion.iem_send = session.percussion_iem_send.clamp(0.0, 1.0);
        }
        if session.percussion_room_mix.is_finite() {
            self.percussion.room_mix = session.percussion_room_mix.clamp(0.0, 1.0);
        }
        self.percussion
            .set_custom_pattern(session.percussion_custom_pattern.clone());
        self.load_percussion_kit_dir(session.percussion_kit_dir.clone());
        for ch in &session.percussion_channels {
            self.percussion.set_channel(
                ch.instrument,
                Some(ch.enabled),
                Some(ch.gain),
                Some(ch.pan),
            );
        }

        // Restore Setlist or standalone song selection.
        self.pending_single_dir = None;
        self.pending_slot_switch = None;
        self.pending_timing_override = None;
        self.song_crossfade.clear();

        if !session.setlist.items.is_empty() {
            if !session.setlist.name.trim().is_empty() {
                self.setlist_name = session.setlist.name;
            }
            self.setlist_sections = session.setlist.sections;
            self.setlist_items = session.setlist.items;
            self.active_setlist_slot = None;
            let dirs: Vec<String> = self
                .setlist_items
                .iter()
                .filter_map(|i| i.song_dir.clone())
                .collect();
            for dir in dirs {
                self.request_preload(&dir, false);
            }
            let target_slot = session
                .active_setlist_slot
                .filter(|&s| s < self.setlist_items.len())
                .unwrap_or(0);
            self.pending_session_override = Some(SessionPostLoadOverrides {
                click_muted: session.click_muted,
                pad_follow_song_key: session.pad_follow_song_key,
                pad_key: pad_key_label,
                key_shift_semitones: None,
                bpm: None,
            });
            self.select_setlist_slot(target_slot, false);
        } else if let Some(dir) = session
            .active_song_dir
            .filter(|d| !d.trim().is_empty())
        {
            self.pending_single_dir = Some(dir.clone());
            self.pending_session_override = Some(SessionPostLoadOverrides {
                click_muted: session.click_muted,
                pad_follow_song_key: session.pad_follow_song_key,
                pad_key: pad_key_label,
                key_shift_semitones: Some(session.key_shift_semitones),
                bpm: Some(session.current_bpm),
            });
            self.request_preload(&dir, true);
        } else {
            self.key_shift_semitones = session.key_shift_semitones.clamp(-12, 12);
            self.pitched_stretcher
                .set_semitones(self.key_shift_semitones as f32);
            if session.current_bpm.is_finite() {
                self.current_bpm = session.current_bpm.clamp(30.0, 260.0);
            }
            if session.pad_follow_song_key {
                self.update_pad_key_from_song();
            }
        }
    }

    fn switch_output_device(
        &mut self,
        preferred_device: Option<&str>,
        shared_routing: &Arc<RwLock<SharedRouting>>,
    ) {
        match open_audio_stream(preferred_device, Arc::clone(shared_routing)) {
            Ok((dev_name, sr, ch, stream, prod)) => {
                self._stream = Some(stream);
                self.producer = prod;
                self.device_name = dev_name;
                self.device_channels = ch;
                if sr != self.sample_rate {
                    self.sample_rate = sr;
                    self.crossfade_total = (sr as usize / 80).max(64);
                    self.click_synth.set_sample_rate(sr);
                    self.pad.set_sample_rate(sr);
                    self.percussion.set_sample_rate(sr);
                    self.song_crossfade.set_sample_rate(sr);
                    self.pitched_stretcher.set_sample_rate(sr);
                    self.drums_stretcher.set_sample_rate(sr);
                    self.cues_stretcher.set_sample_rate(sr);
                    self.song_cache.clear();
                    if let Some(slot) = self.active_setlist_slot {
                        self.request_adjacent_preloads(slot);
                    }
                }
            }
            Err(err) => {
                log::warn!(
                    "Failed to switch output device to {:?}: {err}",
                    preferred_device
                );
            }
        }
    }

    fn sync_active_slot_overrides(&mut self) {
        let Some(slot) = self.active_setlist_slot else {
            return;
        };
        if let Some(item) = self.setlist_items.get_mut(slot) {
            item.key_shift_semitones = self.key_shift_semitones;
            let base = self.song.manifest.bpm;
            item.bpm_override = if (self.current_bpm - base).abs() < 0.01 {
                None
            } else {
                Some(self.current_bpm)
            };
            item.click_mode = if self.active_click_mode == self.song.manifest.click_mode {
                None
            } else {
                Some(self.active_click_mode)
            };
        }
    }

    fn apply_click_mode_state(&mut self) {
        let cur_src_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
        let intro_end_sec = self.song.manifest.intro_end_seconds();
        match self.active_click_mode {
            ClickMode::AlwaysOn => {
                self.intro_click_auto_muted = false;
                self.click_muted = false;
            }
            ClickMode::Off => {
                self.intro_click_auto_muted = false;
                self.click_muted = true;
            }
            ClickMode::IntroOnly => {
                if cur_src_sec >= intro_end_sec {
                    self.intro_click_auto_muted = true;
                    self.click_muted = true;
                } else {
                    self.intro_click_auto_muted = false;
                    self.click_muted = false;
                }
            }
        }
    }

    fn reset_intro_click_if_needed(&mut self) {
        if self.active_click_mode != ClickMode::IntroOnly {
            self.intro_click_auto_muted = false;
            return;
        }
        let cur_src_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
        let intro_end_sec = self.song.manifest.intro_end_seconds();
        if cur_src_sec < intro_end_sec {
            if self.intro_click_auto_muted {
                self.intro_click_auto_muted = false;
                self.click_muted = false;
            }
        } else if !self.intro_click_auto_muted {
            self.intro_click_auto_muted = true;
            self.click_muted = true;
        }
    }

    fn update_pad_key_from_song(&mut self) {
        if !self.pad.follow_song_key {
            return;
        }
        let target_key = transpose_key_name(&self.song.manifest.original_key, self.key_shift_semitones);
        if crate::pad::key_to_pitch_class(&target_key).is_some() {
            self.pad.set_key(&target_key);
        }
    }

    fn apply_song_timing(
        &mut self,
        bpm: f64,
        first_downbeat_sec: f64,
        beats_per_bar: u8,
        click_mode: Option<ClickMode>,
        sections: Option<Vec<SectionDef>>,
    ) {
        let was_bypassed = self.is_stretch_bypassed();
        let prev_rate = self.tempo_ratio();
        let clamped_bpm = bpm.clamp(30.0, 260.0);
        let clamped_downbeat = first_downbeat_sec.max(0.0);
        let clamped_bpb = beats_per_bar.clamp(1, 16);
        let sanitized_sections: Option<Vec<SectionDef>> = sections.and_then(|list| {
            let clean: Vec<SectionDef> = list
                .into_iter()
                .filter(|s| {
                    let valid_bounds = match (s.start_sec, s.end_sec) {
                        (Some(start), Some(end)) => {
                            start.is_finite() && end.is_finite() && start >= 0.0 && end > start + 0.01
                        }
                        _ => s.start_bar >= 1 && s.end_bar > s.start_bar,
                    };
                    valid_bounds && !s.name.trim().is_empty()
                })
                .collect();
            if clean.is_empty() {
                None
            } else {
                Some(clean)
            }
        });

        self.song.manifest.bpm = clamped_bpm;
        self.current_bpm = clamped_bpm;
        self.song.manifest.first_downbeat_sec = clamped_downbeat;
        self.song.manifest.time_signature[0] = clamped_bpb;
        if let Some(ref secs) = sanitized_sections {
            self.song.manifest.sections = secs.clone();
            let cur_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
            let (bar, _, _) = self.song.manifest.seconds_to_bar_beat(cur_sec);
            self.active_section_idx = self
                .song
                .manifest
                .sections
                .iter()
                .position(|s| self.song.manifest.section_contains_at(s, cur_sec, bar))
                .or(Some(0));
            if let Some(armed) = self.armed_section_idx {
                if armed >= secs.len() {
                    self.armed_section_idx = None;
                }
            }
        }
        if let Some(cm) = click_mode {
            self.song.manifest.click_mode = cm;
            let active_slot_override = self
                .active_setlist_slot
                .and_then(|s| self.setlist_items.get(s))
                .and_then(|item| item.click_mode);
            if active_slot_override.is_none() {
                self.active_click_mode = cm;
                self.apply_click_mode_state();
            }
        } else {
            self.reset_intro_click_if_needed();
        }
        self.sync_stretch_transition(was_bypassed, prev_rate);
        self.sync_audition_tempo_from_song();
        if self.is_playing {
            let src_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
            self.last_beat_index = self
                .song
                .manifest
                .seconds_to_continuous_beat(src_sec)
                .map(|b| b.floor() as i64);
        } else {
            self.last_beat_index = None;
            self.click_delay.clear();
        }

        if let Some(ref active_dir) = self.active_song_dir {
            if let Some(CacheEntry::Ready(cached)) = self.song_cache.get_mut(active_dir) {
                cached.manifest.bpm = clamped_bpm;
                cached.manifest.first_downbeat_sec = clamped_downbeat;
                cached.manifest.time_signature[0] = clamped_bpb;
                if let Some(ref secs) = sanitized_sections {
                    cached.manifest.sections = secs.clone();
                }
                if let Some(cm) = click_mode {
                    cached.manifest.click_mode = cm;
                }
            }
            for item in &mut self.setlist_items {
                if item.song_dir.as_deref() == Some(active_dir.as_str()) {
                    item.base_bpm = Some(clamped_bpm);
                    if let Some(cm) = click_mode {
                        item.base_click_mode = Some(cm);
                    }
                }
            }
        }
    }

    fn is_slot_ready(&self, slot: usize) -> bool {
        let Some(item) = self.setlist_items.get(slot) else {
            return false;
        };
        if item.slot_kind != SetlistSlotKind::SongBundle {
            return true;
        }
        item.song_dir
            .as_deref()
            .is_some_and(|d| matches!(self.song_cache.get(d), Some(CacheEntry::Ready(_))))
    }

    fn load_percussion_kit_dir(&mut self, dir_opt: Option<String>) {
        match dir_opt {
            Some(ref d) if !d.trim().is_empty() => {
                let clean = d.trim().to_string();
                if self.percussion.active_kit_dir.as_deref() == Some(clean.as_str()) {
                    return;
                }
                if let Ok(kit) = load_percussion_kit_from_dir(Path::new(&clean)) {
                    self.percussion.set_sample_kit(Some(kit), Some(clean));
                }
            }
            _ => {
                self.percussion.set_sample_kit(None, None);
            }
        }
    }

    fn apply_percussion_lock_and_kit(&mut self, lock: &PercussionLockConfig) {
        self.percussion.apply_lock(lock);
        if lock.kit_dir != self.percussion.active_kit_dir {
            self.load_percussion_kit_dir(lock.kit_dir.clone());
        }
    }

    fn current_percussion_lock(&self) -> PercussionLockConfig {
        PercussionLockConfig {
            mode: self.percussion.mode,
            style: self.percussion.style,
            intensity: self.percussion.intensity,
            gain: self.percussion.gain,
            iem_send: self.percussion.iem_send,
            follow_sections: self.percussion.follow_sections,
            room_mix: self.percussion.room_mix,
            kit_name: self.percussion.active_kit_name.clone(),
            kit_dir: self.percussion.active_kit_dir.clone(),
            custom_pattern: self.percussion.custom_pattern.clone(),
            channels: self.percussion.channels.clone(),
        }
    }

    fn sync_active_percussion_lock(&mut self) {
        let Some(slot) = self.active_setlist_slot else {
            return;
        };
        let lock = self.current_percussion_lock();
        if let Some(item) = self.setlist_items.get_mut(slot) {
            if item.percussion_lock.is_some() || self.percussion.mode != PercussionMode::Off {
                item.percussion_lock = Some(lock);
            }
        }
    }

    fn sync_active_stem_mix(&mut self) {
        let Some(slot) = self.active_setlist_slot else {
            return;
        };
        if let Some(item) = self.setlist_items.get_mut(slot) {
            item.stem_mix = self
                .song
                .stems
                .iter()
                .map(|s| StemMixOverride {
                    name: s.name.clone(),
                    gain: s.gain,
                    muted: s.muted,
                })
                .collect();
        }
    }

    fn sync_audition_tempo_from_song(&mut self) {
        self.percussion_audition_bpm = self.current_bpm.clamp(40.0, 240.0);
        self.percussion_audition_bpb = (self.song.manifest.beats_per_bar() as u8).clamp(2, 16);
    }

    /// Defaults consecutive `SongBundle` songs inside the same section to `AutoAdvance`
    /// while preserving `Manual` overrides and keeping the final song at `AutoPause`.
    fn apply_section_transition_defaults(&mut self, section_id: &str) {
        let indices: Vec<usize> = self
            .setlist_items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                item.section_id.as_deref() == Some(section_id)
                    && item.slot_kind == SetlistSlotKind::SongBundle
            })
            .map(|(i, _)| i)
            .collect();
        if indices.len() < 2 {
            return;
        }
        let last_i = indices[indices.len() - 1];
        for idx in indices {
            if idx != last_i {
                if let Some(item) = self.setlist_items.get_mut(idx) {
                    if item.transition_mode == TransitionMode::AutoPause {
                        item.transition_mode = TransitionMode::AutoAdvance;
                    }
                }
            }
        }
    }

    /// Returns the continuous 0-based beat position for the current playback frame,
    /// bridging seamlessly across same-section transitions when `percussion_bridge_beat` is active.
    fn advance_beat_clock_frame(
        &mut self,
        frame_src_sec: f64,
        first_downbeat_sec: f64,
    ) -> Option<f64> {
        if let Some(ref mut bridge_beat) = self.percussion_bridge_beat {
            if self.percussion_bridge_continuous || frame_src_sec < first_downbeat_sec {
                let b = *bridge_beat;
                let beat_inc = (self.current_bpm / 60.0) / self.sample_rate.max(1) as f64;
                *bridge_beat += beat_inc;
                return Some(b);
            }
            // Audible stems reached Song B Beat 1: hand off cleanly to Song B's timeline without killing voices.
            self.percussion_bridge_beat = None;
            self.percussion_bridge_continuous = false;
            self.last_beat_index = None;
            self.percussion.reset_step_cursor();
        }
        self.song.manifest.seconds_to_continuous_beat(frame_src_sec)
    }

    fn process_beat_and_percussion_frame(
        &mut self,
        frame_src_sec: f64,
        first_downbeat_sec: f64,
        bpb: i64,
    ) {
        if let Some(cont_beat) = self.advance_beat_clock_frame(frame_src_sec, first_downbeat_sec) {
            let beat_idx = cont_beat.floor() as i64;
            if self.last_beat_index != Some(beat_idx) {
                self.last_beat_index = Some(beat_idx);
                let is_downbeat = beat_idx.rem_euclid(bpb.max(1)) == 0;
                self.click_synth.trigger(is_downbeat);
            }
            if self.percussion.is_enabled() {
                self.percussion
                    .advance_and_trigger_step(cont_beat, bpb.max(1) as u32);
            }
        }
    }

    fn select_setlist_slot(&mut self, slot: usize, autoplay: bool) {
        if slot >= self.setlist_items.len() {
            return;
        }
        let item = self.setlist_items[slot].clone();
        if item.slot_kind != SetlistSlotKind::SongBundle {
            self.pending_slot_switch = None;
            let xfade = self
                .active_setlist_slot
                .and_then(|cur| self.setlist_items.get(cur))
                .map(|cur| cur.crossfade_sec)
                .unwrap_or(2.0);
            self.activate_cached_slot_with_crossfade(slot, autoplay, item.ambient_pad_enabled, xfade);
            self.request_adjacent_preloads(slot);
            return;
        }
        let Some(dir) = item.song_dir else {
            return;
        };

        if matches!(self.song_cache.get(&dir), Some(CacheEntry::Ready(_))) {
            self.pending_slot_switch = None;
            let xfade = self
                .active_setlist_slot
                .and_then(|cur| self.setlist_items.get(cur))
                .map(|cur| cur.crossfade_sec)
                .unwrap_or(2.0);
            self.activate_cached_slot_with_crossfade(slot, autoplay, false, xfade);
        } else {
            self.pending_slot_switch = Some((slot, autoplay, false));
            self.request_preload(&dir, true);
        }
        self.request_adjacent_preloads(slot);
    }

    fn apply_pad_lock(&mut self, lock: &PadLockConfig) {
        self.pad.enabled = lock.enabled;
        if lock.enabled {
            self.pad.muted = false;
        }
        self.pad.gain = lock.gain.clamp(0.0, 1.5);
        self.pad.apply_patch(PadPatchSpec {
            preset_id: lock.preset_id.clone(),
            preset_name: lock
                .preset_name
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Locked Pad".to_string()),
            layers: lock.layers.clone(),
            brightness: lock.brightness,
            shimmer: lock.shimmer,
            pan: lock.pan,
            crossfade_sec: None,
        });
    }

    fn activate_cached_slot(&mut self, slot: usize, autoplay: bool, engage_pad: bool) {
        let xfade = self
            .active_setlist_slot
            .and_then(|cur| self.setlist_items.get(cur))
            .map(|cur| cur.crossfade_sec)
            .unwrap_or(0.0);
        self.activate_cached_slot_with_crossfade(slot, autoplay, engage_pad, xfade);
    }

    fn activate_cached_slot_with_crossfade(
        &mut self,
        slot: usize,
        autoplay: bool,
        engage_pad: bool,
        crossfade_sec: f32,
    ) {
        let Some(item) = self.setlist_items.get(slot).cloned() else {
            return;
        };
        let (mut song, dir_opt) = if item.slot_kind == SetlistSlotKind::SongBundle {
            let Some(ref dir) = item.song_dir else {
                return;
            };
            let Some(CacheEntry::Ready(song)) = self.song_cache.get(dir).map(|entry| match entry {
                CacheEntry::Ready(s) => CacheEntry::Ready(s.clone()),
                CacheEntry::Loading => CacheEntry::Loading,
                CacheEntry::Error(e) => CacheEntry::Error(e.clone()),
            }) else {
                return;
            };
            (song, Some(dir.clone()))
        } else {
            let standalone = generate_standalone_slot_song(
                &item.song_id,
                item.title.as_deref().unwrap_or(&item.song_id),
                item.original_key.as_deref().unwrap_or("C"),
                item.base_bpm.unwrap_or(80.0),
                item.time_signature.unwrap_or([4, 4]),
                item.slot_kind,
                self.sample_rate,
            );
            (standalone, None)
        };

        for ov in &item.stem_mix {
            if let Some(stem) = song.stems.iter_mut().find(|s| s.name == ov.name) {
                stem.gain = ov.gain.clamp(0.0, 1.5);
                stem.muted = ov.muted;
            }
        }

        let was_playing = self.is_playing;
        let was_perc_enabled = self.percussion.is_enabled();
        let prev_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
        let prev_beat = self
            .percussion_bridge_beat
            .or_else(|| self.song.manifest.seconds_to_continuous_beat(prev_sec));
        let prev_item_opt = self
            .active_setlist_slot
            .and_then(|cur| self.setlist_items.get(cur).cloned());
        let prev_section_id = prev_item_opt
            .as_ref()
            .and_then(|p| p.section_id.as_ref())
            .cloned();
        let same_section = prev_section_id
            .as_ref()
            .is_some_and(|sid| item.section_id.as_ref() == Some(sid));

        let should_crossfade = self.is_playing
            && autoplay
            && self.active_setlist_slot.is_some_and(|cur| cur != slot)
            && crossfade_sec > 0.01;
        if should_crossfade {
            let outgoing_song = self.song.clone();
            let outgoing_pos = self.source_frame_pos;
            let outgoing_key_shift = self.key_shift_semitones;
            let outgoing_rate = self.tempo_ratio();
            let outgoing_perc_only = self.percussion.mode == PercussionMode::PercussionOnly;
            self.song_crossfade.trigger(
                outgoing_song,
                outgoing_pos,
                outgoing_key_shift,
                outgoing_rate,
                outgoing_perc_only,
                crossfade_sec,
            );
        } else if !autoplay {
            self.song_crossfade.clear();
        }

        self.active_setlist_slot = Some(slot);
        if let Some(ref lock) = item.pad_lock {
            self.apply_pad_lock(lock);
        } else if (engage_pad || item.slot_kind == SetlistSlotKind::PadOnly)
            && item.ambient_pad_enabled
        {
            self.pad.enabled = true;
            self.pad.muted = false;
        }

        if let Some(ref perc_lock) = item.percussion_lock {
            self.apply_percussion_lock_and_kit(perc_lock);
        } else if item.slot_kind == SetlistSlotKind::PercussionGroove {
            self.percussion.mode = PercussionMode::PercussionOnly;
        } else if same_section {
            // Within a service section, keep active percussion style and mode flowing continuously.
        } else if prev_section_id.is_some() || self.percussion.mode == PercussionMode::PercussionOnly
        {
            // At the end of a section (or moving out of PercussionOnly), default is to stop the style.
            self.percussion.mode = PercussionMode::Off;
        }

        let preserve_percussion =
            was_playing && autoplay && was_perc_enabled && self.percussion.is_enabled();
        let incoming_muted_stems = self.percussion.mode == PercussionMode::PercussionOnly
            || item.slot_kind != SetlistSlotKind::SongBundle
            || song
                .stems
                .iter()
                .all(|s| s.is_cues || s.muted || s.gain <= 0.0001);

        if was_playing && autoplay && same_section {
            // Maintain continuous beat & metronome count across same-section transitions,
            // and keep active drum hits / room reverb / delay intact when percussion continues.
            if let Some(b) = prev_beat {
                self.percussion_bridge_beat = Some(b);
                self.percussion_bridge_continuous = incoming_muted_stems;
            } else {
                self.percussion_bridge_beat = None;
                self.percussion_bridge_continuous = false;
            }
            if !preserve_percussion {
                self.percussion.reset_step_cursor();
            }
        } else {
            self.percussion_bridge_beat = None;
            self.percussion_bridge_continuous = false;
            if !preserve_percussion {
                if !autoplay {
                    self.percussion.reset_transport();
                } else {
                    self.percussion.reset_step_cursor();
                }
            } else {
                self.percussion.reset_step_cursor();
            }
        }

        self.apply_song_with_overrides(
            song,
            dir_opt,
            item.key_shift_semitones,
            item.bpm_override,
            item.click_mode,
            autoplay,
            if item.pad_lock.is_some() || item.ambient_pad_enabled {
                Some(item.pad_key_override.as_deref())
            } else {
                None
            },
        );
        if slot == 0 {
            if let Some(muted) = self.pending_setlist_click_muted.take() {
                if self.active_click_mode != ClickMode::Off {
                    self.click_muted = muted;
                }
            }
        }
        self.request_adjacent_preloads(slot);
    }

    fn is_continuing_until_stopped(&self) -> bool {
        self.continue_until_stopped
            || self
                .active_setlist_slot
                .and_then(|slot| self.setlist_items.get(slot))
                .is_some_and(|item| item.transition_mode == TransitionMode::ContinueUntilStopped)
    }

    fn current_stems_effectively_muted(&self) -> bool {
        self.percussion.mode == PercussionMode::PercussionOnly
            || self.song.stems.is_empty()
            || self
                .song
                .stems
                .iter()
                .all(|s| s.is_cues || s.muted || s.gain <= 0.0001)
    }

    fn wrap_percussion_groove_continuously(&mut self) {
        if self.song.total_frames == 0 {
            return;
        }
        if self.percussion_bridge_beat.is_none() {
            let cur_sec = self.source_frame_pos / self.sample_rate.max(1) as f64;
            if let Some(b) = self.song.manifest.seconds_to_continuous_beat(cur_sec) {
                self.percussion_bridge_beat = Some(b);
            }
        }
        self.percussion_bridge_continuous = true;
        self.source_frame_pos %= self.song.total_frames as f64;
        if self.percussion_bridge_beat.is_none() {
            self.last_beat_index = None;
            self.percussion.reset_step_cursor();
        }
    }

    fn maybe_trigger_auto_advance_crossfade(&mut self) {
        if !self.is_playing || self.loop_enabled || self.is_continuing_until_stopped() {
            return;
        }
        let Some(cur) = self.active_setlist_slot else {
            return;
        };
        let Some(cur_item) = self.setlist_items.get(cur).cloned() else {
            return;
        };
        if cur_item.slot_kind != SetlistSlotKind::SongBundle
            || cur_item.transition_mode != TransitionMode::AutoAdvance
            || cur_item.crossfade_sec <= 0.01
        {
            return;
        }
        let next_slot = cur + 1;
        if next_slot >= self.setlist_items.len() || !self.is_slot_ready(next_slot) {
            return;
        }
        let remaining_frames = (self.song.total_frames as f64 - self.source_frame_pos).max(0.0);
        let remaining_sec = (remaining_frames / self.sample_rate.max(1) as f64) / self.tempo_ratio();
        if remaining_sec > 0.02 && remaining_sec <= cur_item.crossfade_sec as f64 {
            let next_engage_pad = self.setlist_items[next_slot].ambient_pad_enabled;
            self.activate_cached_slot_with_crossfade(
                next_slot,
                true,
                next_engage_pad,
                remaining_sec as f32,
            );
        }
    }

    fn handle_song_end(&mut self) {
        if let Some(cur) = self.active_setlist_slot {
            if let Some(cur_item) = self.setlist_items.get(cur) {
                if cur_item.slot_kind != SetlistSlotKind::SongBundle && self.song.total_frames > 0 {
                    // PadOnly and PercussionGroove slots sustain/groove continuously until advanced.
                    self.wrap_percussion_groove_continuously();
                    return;
                }
            }
        }

        if self.is_continuing_until_stopped() {
            if self.song.total_frames > 0 {
                if self.current_stems_effectively_muted() {
                    // Preserve continuous beat clock and percussion step phase seamlessly across wrap.
                    self.wrap_percussion_groove_continuously();
                } else {
                    self.percussion_bridge_beat = None;
                    self.percussion_bridge_continuous = false;
                    self.source_frame_pos %= self.song.total_frames as f64;
                    self.last_beat_index = None;
                    self.click_delay.clear();
                    self.percussion.reset_step_cursor();
                    self.armed_section_idx = None;
                    self.active_section_idx = if self.song.manifest.sections.is_empty() {
                        None
                    } else {
                        Some(0)
                    };
                }
                self.prime_stretchers();
            }
            return;
        }

        if self.loop_enabled {
            self.source_frame_pos = 0.0;
            if !self.percussion_bridge_continuous {
                self.last_beat_index = None;
                self.click_delay.clear();
                self.percussion.reset_step_cursor();
            }
            self.reset_intro_click_if_needed();
            self.prime_stretchers();
            return;
        }

        if let Some(cur) = self.active_setlist_slot {
            if let Some(cur_item) = self.setlist_items.get(cur).cloned() {
                let next_slot = cur + 1;
                if next_slot < self.setlist_items.len() {
                    let next_item = self.setlist_items[next_slot].clone();
                    match cur_item.transition_mode {
                        TransitionMode::AutoAdvance => {
                            if next_item.slot_kind != SetlistSlotKind::SongBundle {
                                self.activate_cached_slot_with_crossfade(
                                    next_slot,
                                    true,
                                    next_item.ambient_pad_enabled,
                                    0.0,
                                );
                                return;
                            }
                            if let Some(ref dir) = next_item.song_dir {
                                if matches!(self.song_cache.get(dir), Some(CacheEntry::Ready(_))) {
                                    self.activate_cached_slot_with_crossfade(
                                        next_slot,
                                        true,
                                        next_item.ambient_pad_enabled,
                                        0.0,
                                    );
                                    return;
                                }
                                if next_item.ambient_pad_enabled {
                                    self.pad.enabled = true;
                                    self.pad.muted = false;
                                }
                                self.is_playing = false;
                                self.pending_slot_switch = Some((next_slot, true, next_item.ambient_pad_enabled));
                                self.request_preload(dir, true);
                                return;
                            }
                        }
                        TransitionMode::AutoPause => {
                            if next_item.slot_kind != SetlistSlotKind::SongBundle {
                                self.activate_cached_slot_with_crossfade(
                                    next_slot,
                                    false,
                                    next_item.ambient_pad_enabled,
                                    0.0,
                                );
                                return;
                            }
                            if let Some(ref dir) = next_item.song_dir {
                                if next_item.ambient_pad_enabled {
                                    self.pad.enabled = true;
                                    self.pad.muted = false;
                                }
                                if matches!(self.song_cache.get(dir), Some(CacheEntry::Ready(_))) {
                                    self.activate_cached_slot_with_crossfade(
                                        next_slot,
                                        false,
                                        next_item.ambient_pad_enabled,
                                        0.0,
                                    );
                                } else {
                                    self.is_playing = false;
                                    self.pending_slot_switch = Some((next_slot, false, next_item.ambient_pad_enabled));
                                    self.request_preload(dir, true);
                                }
                                return;
                            }
                        }
                        TransitionMode::Manual | TransitionMode::ContinueUntilStopped => {}
                    }
                }
            }
        }

        self.is_playing = false;
        self.source_frame_pos = 0.0;
        self.last_beat_index = None;
        self.click_delay.clear();
        self.percussion_bridge_beat = None;
        self.percussion_bridge_continuous = false;
        self.percussion.reset_transport();
        self.reset_intro_click_if_needed();
        self.armed_section_idx = None;
        self.active_section_idx = if self.song.manifest.sections.is_empty() {
            None
        } else {
            Some(0)
        };
    }

    fn apply_song_with_overrides(
        &mut self,
        song: LoadedSong,
        dir: Option<String>,
        key_shift_semitones: i32,
        bpm_override: Option<f64>,
        click_mode_override: Option<ClickMode>,
        autoplay: bool,
        auto_pad_key: Option<Option<&str>>,
    ) {
        self.active_song_dir = dir;
        self.is_playing = autoplay;
        self.current_bpm = bpm_override.unwrap_or(song.manifest.bpm).clamp(30.0, 260.0);
        self.key_shift_semitones = key_shift_semitones.clamp(-12, 12);
        self.pitched_stretcher
            .set_semitones(self.key_shift_semitones as f32);
        let keep_continuous_clock = autoplay && self.percussion_bridge_beat.is_some();
        self.source_frame_pos = 0.0;
        if !keep_continuous_clock {
            self.last_beat_index = None;
            self.click_delay.clear();
        }
        self.armed_section_idx = None;
        self.active_section_idx = if song.manifest.sections.is_empty() {
            None
        } else {
            Some(0)
        };
        let effective_click_mode = click_mode_override.unwrap_or(song.manifest.click_mode);
        self.song = song;
        if keep_continuous_clock && (self.is_stretch_bypassed() || self.song.stems.is_empty()) {
            self.click_delay.clear();
        }
        self.active_click_mode = effective_click_mode;
        self.apply_click_mode_state();

        if let Some(pad_key_opt) = auto_pad_key {
            if let Some(override_key) = pad_key_opt {
                self.pad.follow_song_key = false;
                self.pad.set_key(override_key);
            } else if self.pad.follow_song_key {
                self.update_pad_key_from_song();
            }
        } else {
            self.update_pad_key_from_song();
        }

        if autoplay {
            self.prime_stretchers();
        }

        if let Some(ov) = self.pending_session_override.take() {
            self.is_playing = false;
            self.pad.enabled = false;
            self.click_muted = ov.click_muted;
            if !ov.pad_follow_song_key {
                self.pad.follow_song_key = false;
                self.pad.set_key(&ov.pad_key);
            }
            if let Some(shift) = ov.key_shift_semitones {
                self.key_shift_semitones = shift.clamp(-12, 12);
                self.pitched_stretcher
                    .set_semitones(self.key_shift_semitones as f32);
            }
            if let Some(bpm) = ov.bpm.filter(|v| v.is_finite()) {
                self.current_bpm = bpm.clamp(30.0, 260.0);
            }
        }
        self.sync_audition_tempo_from_song();
    }

    fn arm_or_jump_section(&mut self, idx: usize) {
        if self.is_playing {
            self.armed_section_idx = Some(idx);
        } else {
            let start_sec = self
                .song
                .manifest
                .section_start_seconds(&self.song.manifest.sections[idx]);
            self.source_frame_pos = start_sec * self.sample_rate as f64;
            self.active_section_idx = Some(idx);
            self.armed_section_idx = None;
        }
    }

    fn is_stretch_bypassed(&self) -> bool {
        self.key_shift_semitones == 0 && (self.current_bpm - self.song.manifest.bpm).abs() < 0.001
    }

    fn tempo_ratio(&self) -> f64 {
        (self.current_bpm / self.song.manifest.bpm).clamp(0.5, 2.0)
    }

    fn stretched_output_latency_frames(&self, rate: f64) -> usize {
        let input_lat = self.pitched_stretcher.input_latency_frames() as f64;
        let output_lat = self.pitched_stretcher.output_latency_frames() as f64;
        ((input_lat / rate.max(0.1)).round() as usize).saturating_add(output_lat as usize)
    }

    fn stretched_source_latency_frames(&self, rate: f64) -> f64 {
        let input_lat = self.pitched_stretcher.input_latency_frames() as f64;
        let output_lat = self.pitched_stretcher.output_latency_frames() as f64;
        input_lat + output_lat * rate
    }

    fn sync_stretch_transition(&mut self, was_bypassed: bool, prev_rate: f64) {
        let is_now_bypassed = self.is_stretch_bypassed();
        if !self.is_playing || was_bypassed == is_now_bypassed {
            return;
        }
        if is_now_bypassed {
            // Rewind source_frame_pos from the stretched lookahead cursor back to audible playhead.
            let prev_src_lat = self.stretched_source_latency_frames(prev_rate);
            self.source_frame_pos = (self.source_frame_pos - prev_src_lat).max(0.0);
        } else {
            // Entering real-time stretch from bypassed playback: prime stretcher history.
            self.prime_stretchers();
        }
        self.last_beat_index = None;
        self.click_delay.clear();
    }

    fn prime_stretchers(&mut self) {
        let rate = self.tempo_ratio();
        let preroll_frames = self.pitched_stretcher.input_latency_frames().min(4096);
        let cur = self.source_frame_pos.floor() as usize;
        let start = cur.saturating_sub(preroll_frames);
        let len = cur.saturating_sub(start);

        if len == 0 {
            self.pitched_stretcher.reset_with_preroll(&[], rate);
            self.drums_stretcher.reset_with_preroll(&[], rate);
            self.cues_stretcher.reset_with_preroll(&[], rate);
            return;
        }

        self.fill_stem_group_slice(start, len);
        self.pitched_stretcher
            .reset_with_preroll(&self.pitched_in, rate);
        self.drums_stretcher
            .reset_with_preroll(&self.drums_in, rate);
        self.cues_stretcher.reset_with_preroll(&self.cues_in, rate);
    }

    fn fill_stem_group_slice(&mut self, start_frame: usize, frames: usize) {
        self.pitched_in.clear();
        self.drums_in.clear();
        self.cues_in.clear();
        self.pitched_in.resize(frames * 2, 0.0);
        self.drums_in.resize(frames * 2, 0.0);
        self.cues_in.resize(frames * 2, 0.0);

        for stem in &self.song.stems {
            if stem.muted || stem.gain <= 0.0001 {
                continue;
            }
            if !stem.is_cues && self.percussion.mode == PercussionMode::PercussionOnly {
                continue;
            }
            let target = if stem.is_cues {
                &mut self.cues_in
            } else if stem.pitched {
                &mut self.pitched_in
            } else {
                &mut self.drums_in
            };

            let avail = stem.frame_count().saturating_sub(start_frame).min(frames);
            for i in 0..avail {
                let s_idx = (start_frame + i) * 2;
                let d_idx = i * 2;
                target[d_idx] += stem.samples[s_idx] * stem.gain;
                target[d_idx + 1] += stem.samples[s_idx + 1] * stem.gain;
            }
        }
    }

    fn render_block(&mut self, out_frames: usize) -> Vec<BusFrame> {
        if self.is_playing {
            self.maybe_trigger_auto_advance_crossfade();
        }

        let bpb = self.song.manifest.beats_per_bar() as i64;
        let first_downbeat_sec = self.song.manifest.first_downbeat_sec;
        let intro_end_sec = self.song.manifest.intro_end_seconds();
        let inv_sr = 1.0 / self.sample_rate.max(1) as f64;
        let no_stems = self.song.stems.is_empty();
        let bypassed = self.is_stretch_bypassed() || no_stems;
        let rate = self.tempo_ratio();

        let mut block = Vec::with_capacity(out_frames);

        if !self.is_playing {
            let perc_active = self.percussion.is_active() || self.percussion_audition_playing;
            let perc_iem = self.percussion.iem_send;
            let beat_inc = (self.percussion_audition_bpm.clamp(40.0, 240.0) / 60.0) * inv_sr;
            let aud_bpb = self.percussion_audition_bpb.max(1) as u32;
            for _ in 0..out_frames {
                let mut click_sample = 0.0_f32;
                if self.percussion_audition_playing {
                    let cur_beat = self.percussion_audition_beat;
                    self.percussion
                        .advance_and_trigger_step_forced(cur_beat, aud_bpb, true);
                    if self.percussion_audition_with_click && !self.click_muted {
                        let beat_idx = cur_beat.floor() as i64;
                        if self.percussion_audition_last_beat != Some(beat_idx) {
                            self.percussion_audition_last_beat = Some(beat_idx);
                            let is_downbeat = (beat_idx % aud_bpb as i64) == 0;
                            self.click_synth.trigger(is_downbeat);
                        }
                        click_sample = self.click_synth.next_sample() * self.click_gain;
                    }
                    self.percussion_audition_beat += beat_inc;
                }
                let (pad_l, pad_r) = self.pad.next_stereo_sample();
                let (perc_l, perc_r) = if perc_active {
                    self.percussion.next_stereo_sample(0)
                } else {
                    (0.0, 0.0)
                };
                block.push(BusFrame {
                    music_l: pad_l + perc_l,
                    music_r: pad_r + perc_r,
                    iem_l: click_sample + perc_l * perc_iem,
                    iem_r: click_sample + perc_r * perc_iem,
                });
            }
            return block;
        }

        let song_xfade_info = self.song_crossfade.render_block(out_frames);
        let perc_iem = self.percussion.iem_send;
        let backing_gain = self.backing_gain;

        if bypassed {
            let step_rate = if no_stems { rate } else { 1.0 };
            let start_frame = self.source_frame_pos.round() as usize;
            let src_frames = ((out_frames as f64) * step_rate).round().max(1.0) as usize;
            self.check_quantized_transitions(start_frame, src_frames);

            let cur_start_f = self.source_frame_pos;
            let cur_start = cur_start_f.round() as usize;
            self.fill_stem_group_slice(cur_start, out_frames);

            for i in 0..out_frames {
                let frame_src_pos = cur_start_f + (i as f64) * step_rate;
                let frame_src_sec = frame_src_pos * inv_sr;
                if self.active_click_mode == ClickMode::IntroOnly
                    && !self.intro_click_auto_muted
                    && frame_src_sec >= intro_end_sec
                {
                    self.intro_click_auto_muted = true;
                    self.click_muted = true;
                }
                self.process_beat_and_percussion_frame(frame_src_sec, first_downbeat_sec, bpb);

                let click_sample = if self.click_muted {
                    0.0
                } else {
                    self.click_synth.next_sample() * self.click_gain
                };

                let mut frame = BusFrame {
                    music_l: (self.pitched_in[i * 2] + self.drums_in[i * 2]) * backing_gain,
                    music_r: (self.pitched_in[i * 2 + 1] + self.drums_in[i * 2 + 1]) * backing_gain,
                    iem_l: click_sample + self.cues_in[i * 2],
                    iem_r: click_sample + self.cues_in[i * 2 + 1],
                };

                if self.crossfade_remaining > 0 {
                    let tail_idx = self.crossfade_total - self.crossfade_remaining;
                    if let Some(old) = self.prev_tail.get(tail_idx) {
                        let alpha = tail_idx as f32 / self.crossfade_total as f32;
                        frame.music_l = old.music_l * (1.0 - alpha) + frame.music_l * alpha;
                        frame.music_r = old.music_r * (1.0 - alpha) + frame.music_r * alpha;
                    }
                    self.crossfade_remaining -= 1;
                }

                if let Some((xf_start, xf_total)) = song_xfade_info {
                    if i < self.song_crossfade.out_buf.len() {
                        let p = ((xf_start + i) as f32 / xf_total.max(1) as f32).clamp(0.0, 1.0);
                        let g_out = (p * FRAC_PI_2).cos();
                        let g_in = (p * FRAC_PI_2).sin();
                        let out_lr = self.song_crossfade.out_buf[i];
                        frame.music_l = out_lr[0] * backing_gain * g_out + frame.music_l * g_in;
                        frame.music_r = out_lr[1] * backing_gain * g_out + frame.music_r * g_in;
                    }
                }

                if self.percussion.is_active() {
                    let (perc_l, perc_r) = self.percussion.next_stereo_sample(0);
                    frame.music_l += perc_l;
                    frame.music_r += perc_r;
                    frame.iem_l += perc_l * perc_iem;
                    frame.iem_r += perc_r * perc_iem;
                }

                if self.pad.is_active() {
                    let (pad_l, pad_r) = self.pad.next_stereo_sample();
                    frame.music_l += pad_l;
                    frame.music_r += pad_r;
                }

                block.push(frame);
            }

            self.source_frame_pos = if no_stems {
                cur_start_f + (out_frames as f64) * step_rate
            } else {
                (cur_start + out_frames) as f64
            };
        } else {
            let cur_start_f = self.source_frame_pos;
            let next_pos_f = cur_start_f + (out_frames as f64) * rate;
            let src_start = cur_start_f.round() as usize;
            let src_end = next_pos_f.round() as usize;
            let src_frames = src_end.saturating_sub(src_start).max(1);

            self.check_quantized_transitions(src_start, src_frames);

            let cur_start = self.source_frame_pos.round() as usize;
            let cur_end = (self.source_frame_pos + (out_frames as f64) * rate).round() as usize;
            let actual_src_frames = cur_end.saturating_sub(cur_start).max(1);
            self.fill_stem_group_slice(cur_start, actual_src_frames);

            self.pitched_out.resize(out_frames * 2, 0.0);
            self.drums_out.resize(out_frames * 2, 0.0);
            self.cues_out.resize(out_frames * 2, 0.0);

            self.pitched_stretcher
                .process(&self.pitched_in, &mut self.pitched_out);
            self.drums_stretcher
                .process(&self.drums_in, &mut self.drums_out);
            self.cues_stretcher
                .process(&self.cues_in, &mut self.cues_out);

            let latency = self.stretched_output_latency_frames(rate);
            let perc_delay = latency;

            for i in 0..out_frames {
                let frame_src_pos = self.source_frame_pos + (i as f64) * rate;
                let frame_src_sec = frame_src_pos * inv_sr;
                if self.active_click_mode == ClickMode::IntroOnly
                    && !self.intro_click_auto_muted
                    && frame_src_sec >= intro_end_sec
                {
                    self.intro_click_auto_muted = true;
                    self.click_muted = true;
                }
                self.process_beat_and_percussion_frame(frame_src_sec, first_downbeat_sec, bpb);

                let raw_click = if self.click_muted {
                    0.0
                } else {
                    self.click_synth.next_sample() * self.click_gain
                };

                let click_sample = self.click_delay.process_sample(raw_click, latency);

                let mut frame = BusFrame {
                    music_l: (self.pitched_out[i * 2] + self.drums_out[i * 2]) * backing_gain,
                    music_r: (self.pitched_out[i * 2 + 1] + self.drums_out[i * 2 + 1])
                        * backing_gain,
                    iem_l: click_sample + self.cues_out[i * 2],
                    iem_r: click_sample + self.cues_out[i * 2 + 1],
                };

                if self.crossfade_remaining > 0 {
                    let tail_idx = self.crossfade_total - self.crossfade_remaining;
                    if let Some(old) = self.prev_tail.get(tail_idx) {
                        let alpha = tail_idx as f32 / self.crossfade_total as f32;
                        frame.music_l = old.music_l * (1.0 - alpha) + frame.music_l * alpha;
                        frame.music_r = old.music_r * (1.0 - alpha) + frame.music_r * alpha;
                    }
                    self.crossfade_remaining -= 1;
                }

                if let Some((xf_start, xf_total)) = song_xfade_info {
                    if i < self.song_crossfade.out_buf.len() {
                        let p = ((xf_start + i) as f32 / xf_total.max(1) as f32).clamp(0.0, 1.0);
                        let g_out = (p * FRAC_PI_2).cos();
                        let g_in = (p * FRAC_PI_2).sin();
                        let out_lr = self.song_crossfade.out_buf[i];
                        frame.music_l = out_lr[0] * backing_gain * g_out + frame.music_l * g_in;
                        frame.music_r = out_lr[1] * backing_gain * g_out + frame.music_r * g_in;
                    }
                }

                if self.percussion.is_active() {
                    let (perc_l, perc_r) = self.percussion.next_stereo_sample(perc_delay);
                    frame.music_l += perc_l;
                    frame.music_r += perc_r;
                    frame.iem_l += perc_l * perc_iem;
                    frame.iem_r += perc_r * perc_iem;
                }

                if self.pad.is_active() {
                    let (pad_l, pad_r) = self.pad.next_stereo_sample();
                    frame.music_l += pad_l;
                    frame.music_r += pad_r;
                }

                block.push(frame);
            }

            self.source_frame_pos += (out_frames as f64) * rate;
        }

        if self.source_frame_pos as usize >= self.song.total_frames {
            self.handle_song_end();
        }

        block
    }

    fn check_quantized_transitions(&mut self, start_frame: usize, src_frames: usize) {
        let cur_sec = start_frame as f64 / self.sample_rate as f64;
        let next_sec = (start_frame + src_frames) as f64 / self.sample_rate as f64;
        let (bar_now, _beat_now, _) = self.song.manifest.seconds_to_bar_beat(cur_sec);
        let (bar_next, _beat_next, _) = self.song.manifest.seconds_to_bar_beat(next_sec);

        if let Some(idx) = self
            .song
            .manifest
            .sections
            .iter()
            .position(|s| self.song.manifest.section_contains_at(s, cur_sec, bar_now))
        {
            if self.active_section_idx != Some(idx) {
                self.active_section_idx = Some(idx);
                if self.percussion.follow_sections {
                    self.percussion.intensity =
                        section_intensity_from_name(&self.song.manifest.sections[idx].name);
                }
            }
        }

        let bar_crossed = bar_next > bar_now;
        if let Some(target_idx) = self.armed_section_idx {
            let should_jump = match self.quantize_mode {
                QuantizeMode::NextBar => bar_crossed,
                QuantizeMode::EndOfSection => self
                    .active_section_idx
                    .and_then(|cur| self.song.manifest.sections.get(cur))
                    .map(|s| {
                        if s.end_sec.is_some() {
                            self.song.manifest.section_ended_at(s, next_sec, bar_next)
                        } else {
                            bar_crossed && self.song.manifest.section_ended_at(s, next_sec, bar_next)
                        }
                    })
                    .unwrap_or(bar_crossed),
            };
            if should_jump {
                self.jump_to_section_now(target_idx);
                self.armed_section_idx = None;
                return;
            }
        }

        if self.loop_enabled {
            if let Some(cur_idx) = self.active_section_idx {
                if let Some(sec) = self.song.manifest.sections.get(cur_idx) {
                    let reached_end = if sec.end_sec.is_some() {
                        self.song.manifest.section_ended_at(sec, next_sec, bar_next)
                    } else {
                        bar_crossed && self.song.manifest.section_ended_at(sec, next_sec, bar_next)
                    };
                    if reached_end {
                        self.jump_to_section_now(cur_idx);
                    }
                }
            }
        }
    }

    fn jump_to_section_now(&mut self, section_idx: usize) {
        let Some(target_sec) = self
            .song
            .manifest
            .sections
            .get(section_idx)
            .map(|sec| self.song.manifest.section_start_seconds(sec))
        else {
            return;
        };

        self.prev_tail.clear();
        let cur_pos = self.source_frame_pos.floor() as usize;
        let tail_len = self.crossfade_total;
        self.fill_stem_group_slice(cur_pos, tail_len);
        for i in 0..tail_len {
            self.prev_tail.push(BusFrame {
                music_l: self.pitched_in[i * 2] + self.drums_in[i * 2],
                music_r: self.pitched_in[i * 2 + 1] + self.drums_in[i * 2 + 1],
                iem_l: 0.0,
                iem_r: 0.0,
            });
        }
        self.crossfade_remaining = tail_len;

        self.source_frame_pos = target_sec * self.sample_rate as f64;
        self.active_section_idx = Some(section_idx);
        if self.percussion.follow_sections {
            if let Some(sec) = self.song.manifest.sections.get(section_idx) {
                self.percussion.intensity = section_intensity_from_name(&sec.name);
            }
        }
        self.last_beat_index = None;
        self.click_delay.clear();
        self.percussion_bridge_beat = None;
        self.percussion_bridge_continuous = false;
        self.percussion.reset_step_cursor();
        self.prime_stretchers();
    }

    fn publish_snapshot(
        &self,
        snapshot: &Arc<RwLock<EngineSnapshot>>,
        shared_routing: &Arc<RwLock<SharedRouting>>,
    ) {
        let cur_sec = if self.is_stretch_bypassed() {
            self.source_frame_pos / self.sample_rate as f64
        } else {
            let rate = self.tempo_ratio();
            let src_latency = self.stretched_source_latency_frames(rate);
            ((self.source_frame_pos - src_latency).max(0.0)) / self.sample_rate as f64
        };
        let bpb = self.song.manifest.beats_per_bar();
        let (bar, beat) = if let Some(bridge_beat) = self.percussion_bridge_beat {
            let latency_beats = if self.is_stretch_bypassed() || self.song.stems.is_empty() {
                0.0
            } else {
                let rate = self.tempo_ratio();
                let out_lat = self.stretched_output_latency_frames(rate) as f64;
                (out_lat / self.sample_rate.max(1) as f64) * (self.current_bpm / 60.0)
            };
            let audible_beat = (bridge_beat - latency_beats).max(0.0);
            let total_beats = audible_beat.floor() as u32;
            let bpb_u = bpb.max(1);
            (total_beats / bpb_u + 1, total_beats % bpb_u + 1)
        } else {
            let (b, bt, _) = self.song.manifest.seconds_to_bar_beat(cur_sec);
            (b, bt)
        };
        let (routing_mode, master_gain) = shared_routing
            .read()
            .map(|r| (r.mode, r.master_gain))
            .unwrap_or((OutputRoutingMode::SplitLeftRight, 1.0));

        if let Ok(mut snap) = snapshot.write() {
            snap.song_id = Some(self.song.manifest.id.clone());
            snap.active_song_dir = self.active_song_dir.clone();
            snap.song_title = self.song.manifest.title.clone();
            snap.original_key = self.song.manifest.original_key.clone();
            snap.current_key = transpose_key_name(
                &self.song.manifest.original_key,
                self.key_shift_semitones,
            );
            snap.is_playing = self.is_playing;
            snap.current_bar = bar;
            snap.current_beat = beat;
            snap.beats_per_bar = self.song.manifest.beats_per_bar();
            snap.current_position_sec = cur_sec;
            snap.total_duration_sec = self.song.total_frames as f64 / self.sample_rate as f64;
            snap.active_section_id = self
                .active_section_idx
                .and_then(|i| self.song.manifest.sections.get(i))
                .map(|s| s.id.clone());
            snap.armed_section_id = self
                .armed_section_idx
                .and_then(|i| self.song.manifest.sections.get(i))
                .map(|s| s.id.clone());
            snap.loop_enabled = self.loop_enabled;
            snap.continue_until_stopped = self.continue_until_stopped;
            snap.quantize_mode = self.quantize_mode;
            snap.key_shift_semitones = self.key_shift_semitones;
            snap.base_bpm = self.song.manifest.bpm;
            snap.current_bpm = self.current_bpm;
            snap.master_gain = master_gain;
            snap.backing_gain = self.backing_gain;
            snap.click_gain = self.click_gain;
            snap.click_muted = self.click_muted;
            snap.click_mode = self.active_click_mode;
            snap.click_intro_end_bar = self.song.manifest.intro_end_bar();
            snap.pad_enabled = self.pad.enabled;
            snap.pad_muted = self.pad.muted;
            snap.pad_gain = self.pad.gain;
            snap.pad_key = self.pad.current_key().to_string();
            snap.pad_is_minor = self.pad.is_minor();
            snap.pad_follow_song_key = self.pad.follow_song_key;
            snap.pad_preset_id = self.pad.preset_id().map(|s| s.to_string());
            snap.pad_preset_name = self.pad.preset_name().to_string();
            snap.pad_layers = self.pad.layers().to_vec();
            snap.pad_brightness = self.pad.brightness;
            snap.pad_shimmer = self.pad.shimmer;
            snap.pad_pan = self.pad.pan;
            snap.pad_crossfade_sec = self.pad.crossfade_sec;
            snap.setlist_name = self.setlist_name.clone();
            snap.setlist_sections = self.setlist_sections.clone();
            snap.active_setlist_slot = self.active_setlist_slot;
            snap.song_crossfade_progress = if self.song_crossfade.active {
                Some(
                    (self.song_crossfade.fade_frame_idx as f32
                        / self.song_crossfade.fade_total_frames.max(1) as f32)
                        .clamp(0.0, 1.0),
                )
            } else {
                None
            };
            snap.crossfading_from_title = if self.song_crossfade.active {
                Some(self.song_crossfade.from_title.clone())
            } else {
                None
            };
            snap.percussion_mode = self.percussion.mode;
            snap.percussion_style = self.percussion.style;
            snap.percussion_intensity = self.percussion.intensity;
            snap.percussion_follow_sections = self.percussion.follow_sections;
            snap.percussion_gain = self.percussion.gain;
            snap.percussion_iem_send = self.percussion.iem_send;
            snap.percussion_room_mix = self.percussion.room_mix;
            snap.percussion_kit_name = self.percussion.active_kit_name.clone();
            snap.percussion_kit_dir = self.percussion.active_kit_dir.clone();
            snap.percussion_custom_pattern = self.percussion.custom_pattern.clone();
            snap.percussion_current_step = self.percussion.current_step_in_cycle;
            snap.percussion_audition_playing = self.percussion_audition_playing;
            snap.percussion_audition_bpm = self.percussion_audition_bpm;
            snap.percussion_channels = self.percussion.channels.clone();
            snap.setlist_items = self
                .setlist_items
                .iter()
                .enumerate()
                .map(|(slot, item)| {
                    let original_key = item
                        .original_key
                        .clone()
                        .unwrap_or_else(|| "C".to_string());
                    let base_bpm = item.base_bpm.unwrap_or(120.0);
                    let (preload_state, estimated_duration_sec) =
                        if item.slot_kind != SetlistSlotKind::SongBundle {
                            (PreloadState::Ready, 0.0)
                        } else {
                            match item
                                .song_dir
                                .as_deref()
                                .and_then(|d| self.song_cache.get(d))
                            {
                                Some(CacheEntry::Ready(s)) => (
                                    PreloadState::Ready,
                                    s.total_frames as f64 / s.sample_rate.max(1) as f64,
                                ),
                                Some(CacheEntry::Loading) => (PreloadState::Loading, 0.0),
                                Some(CacheEntry::Error(e)) => (PreloadState::Error(e.clone()), 0.0),
                                None => (PreloadState::Unloaded, 0.0),
                            }
                        };
                    SetlistItemSnapshot {
                        slot,
                        song_id: item.song_id.clone(),
                        song_dir: item.song_dir.clone(),
                        title: item
                            .title
                            .clone()
                            .unwrap_or_else(|| item.song_id.clone()),
                        effective_key: transpose_key_name(&original_key, item.key_shift_semitones),
                        original_key,
                        base_bpm,
                        effective_bpm: item.bpm_override.unwrap_or(base_bpm),
                        key_shift_semitones: item.key_shift_semitones,
                        bpm_override: item.bpm_override,
                        transition_mode: item.transition_mode,
                        crossfade_sec: item.crossfade_sec,
                        ambient_pad_enabled: item.ambient_pad_enabled,
                        pad_key_override: item.pad_key_override.clone(),
                        pad_lock: item.pad_lock.clone(),
                        click_mode: item.click_mode,
                        effective_click_mode: item
                            .click_mode
                            .or(item.base_click_mode)
                            .unwrap_or(ClickMode::AlwaysOn),
                        section_id: item.section_id.clone(),
                        slot_kind: item.slot_kind,
                        time_signature: item.time_signature,
                        percussion_lock: item.percussion_lock.clone(),
                        stem_mix: item.stem_mix.clone(),
                        estimated_duration_sec,
                        preload_state,
                    }
                })
                .collect();
            snap.routing_mode = routing_mode;
            snap.device_name = self.device_name.clone();
            snap.device_sample_rate = self.sample_rate;
            snap.device_channels = self.device_channels;
            snap.sections = self
                .song
                .manifest
                .sections
                .iter()
                .map(|s| SectionSnapshot {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    start_bar: s.start_bar,
                    end_bar: s.end_bar,
                    start_sec: s.start_sec,
                    end_sec: s.end_sec,
                    color: s.color.clone(),
                })
                .collect();
            snap.stems = self
                .song
                .stems
                .iter()
                .map(|s| StemSnapshot {
                    name: s.name.clone(),
                    pitched: s.pitched,
                    is_cues: s.is_cues,
                    gain: s.gain,
                    muted: s.muted,
                })
                .collect();
        }
    }
}

fn build_snapshot(
    song: &LoadedSong,
    device_name: &str,
    sample_rate: u32,
    channels: u16,
    routing_mode: OutputRoutingMode,
) -> EngineSnapshot {
    EngineSnapshot {
        song_id: Some(song.manifest.id.clone()),
        active_song_dir: None,
        song_title: song.manifest.title.clone(),
        original_key: song.manifest.original_key.clone(),
        current_key: song.manifest.original_key.clone(),
        is_playing: false,
        current_bar: 1,
        current_beat: 1,
        beats_per_bar: song.manifest.beats_per_bar(),
        current_position_sec: 0.0,
        total_duration_sec: song.total_frames as f64 / sample_rate as f64,
        active_section_id: song.manifest.sections.first().map(|s| s.id.clone()),
        armed_section_id: None,
        loop_enabled: false,
        continue_until_stopped: false,
        quantize_mode: QuantizeMode::NextBar,
        key_shift_semitones: 0,
        base_bpm: song.manifest.bpm,
        current_bpm: song.manifest.bpm,
        master_gain: 1.0,
        backing_gain: 1.0,
        click_gain: 0.85,
        click_muted: song.manifest.click_mode == ClickMode::Off,
        click_mode: song.manifest.click_mode,
        click_intro_end_bar: song.manifest.intro_end_bar(),
        pad_enabled: false,
        pad_muted: false,
        pad_gain: 0.45,
        pad_key: transpose_key_name(&song.manifest.original_key, 0),
        pad_is_minor: matches!(
            crate::pad::parse_pad_key(&song.manifest.original_key),
            Some((_, crate::pad::PadKeyQuality::Minor))
        ),
        pad_follow_song_key: true,
        pad_preset_id: Some("classic-foundation".into()),
        pad_preset_name: "Classic Foundation".into(),
        pad_layers: default_foundation_layers().to_vec(),
        pad_brightness: 0.55,
        pad_shimmer: 0.20,
        pad_pan: 0.0,
        pad_crossfade_sec: 4.0,
        setlist_name: "Sunday Morning Set".into(),
        setlist_sections: Vec::new(),
        setlist_items: Vec::new(),
        active_setlist_slot: None,
        song_crossfade_progress: None,
        crossfading_from_title: None,
        percussion_mode: PercussionMode::Off,
        percussion_style: PercussionStyle::Worship44,
        percussion_intensity: GrooveIntensity::Medium,
        percussion_follow_sections: true,
        percussion_gain: 0.9,
        percussion_iem_send: 0.0,
        percussion_room_mix: DEFAULT_PERCUSSION_ROOM_MIX,
        percussion_kit_name: None,
        percussion_kit_dir: None,
        percussion_custom_pattern: None,
        percussion_current_step: None,
        percussion_audition_playing: false,
        percussion_audition_bpm: song.manifest.bpm,
        percussion_channels: crate::percussion::default_percussion_channels(),
        routing_mode,
        device_name: device_name.to_string(),
        device_sample_rate: sample_rate,
        device_channels: channels,
        sections: song
            .manifest
            .sections
            .iter()
            .map(|s| SectionSnapshot {
                id: s.id.clone(),
                name: s.name.clone(),
                start_bar: s.start_bar,
                end_bar: s.end_bar,
                start_sec: s.start_sec,
                end_sec: s.end_sec,
                color: s.color.clone(),
            })
            .collect(),
        stems: song
            .stems
            .iter()
            .map(|s| StemSnapshot {
                name: s.name.clone(),
                pitched: s.pitched,
                is_cues: s.is_cues,
                gain: s.gain,
                muted: s.muted,
            })
            .collect(),
    }
}

pub fn transpose_key_name(base: &str, semitones: i32) -> String {
    const KEYS: [&str; 12] = [
        "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
    ];
    let trimmed = base.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("unknown") {
        let label = if trimmed.is_empty() { "Unknown" } else { trimmed };
        return if semitones == 0 {
            label.to_string()
        } else {
            format!("{label} ({semitones:+})")
        };
    }
    let is_minor = trimmed.ends_with('m') && !trimmed.to_ascii_lowercase().ends_with("maj");
    let root_raw = if is_minor {
        &trimmed[..trimmed.len() - 1]
    } else {
        trimmed
    };
    let normalized = match root_raw.trim() {
        "Db" => "C#",
        "D#" => "Eb",
        "Gb" => "F#",
        "G#" => "Ab",
        "A#" => "Bb",
        other => other,
    };
    let Some(pos) = KEYS.iter().position(|&k| k.eq_ignore_ascii_case(normalized)) else {
        return if semitones == 0 {
            trimmed.to_string()
        } else {
            format!("{trimmed} ({semitones:+})")
        };
    };
    let idx = ((pos as i32 + semitones).rem_euclid(12)) as usize;
    if is_minor {
        format!("{}m", KEYS[idx])
    } else {
        KEYS[idx].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_bypass_playback() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);

        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.is_playing = true;

        // When starting from frame 0:
        // Downbeat 1 triggers immediately on frame 0.
        // Music also starts on frame 0.
        let block = state.render_block(512);

        // Click synth envelope is triggered on frame 0
        assert!(state.click_synth.is_active(), "Click should be triggered on frame 0");
        // Audio is produced immediately in block 0
        assert_ne!(block[1].iem_l, 0.0, "Click output should begin on sample 0/1");
        assert_ne!(block[1].music_l, 0.0, "Music output should begin immediately without stretcher delay");
    }

    #[test]
    fn test_sync_between_music_and_click_with_tempo_change() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);

        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.is_playing = true;
        state.current_bpm = 80.0; // Song is 72 BPM, playing at 80 BPM

        state.prime_stretchers();

        let block_size = 512;
        let total_frames = 48000 * 10;
        let mut total_rendered = 0;

        let mut trigger_starts = Vec::new();

        while total_rendered < total_frames {
            let block = state.render_block(block_size);
            for (idx, frame) in block.iter().enumerate() {
                let abs_f = total_rendered + idx;
                let is_audible = frame.iem_l.abs() > 0.01;
                if is_audible {
                    if trigger_starts.is_empty() || abs_f - trigger_starts.last().unwrap() > 20000 {
                        trigger_starts.push(abs_f);
                    }
                }
            }
            total_rendered += block_size;
        }

        println!("Clicks at 80 BPM (trigger starts): {:?}", trigger_starts);
        let latency = state.stretched_output_latency_frames(state.tempo_ratio());
        let diff = (trigger_starts[0] as i64 - latency as i64).abs();
        assert!(diff <= 2, "First click should emerge at stretcher latency (got {}, expected {})", trigger_starts[0], latency);

        for i in 1..trigger_starts.len() {
            let interval = trigger_starts[i] - trigger_starts[i - 1];
            println!("Click interval #{}: {} frames (expected: 36000)", i, interval);
            assert!(
                (interval as i64 - 36000).abs() <= 1,
                "Click interval {} must be 36000 ± 1 frame (got {})!",
                i,
                interval
            );
        }
    }

    #[test]
    fn test_music_and_click_impulse_alignment_across_rates() {
        use crate::loader::DecodedStem;
        let sample_rate = 48000u32;
        let base_bpm = 120.0_f64;
        let spb = (60.0 / base_bpm) * sample_rate as f64; // 24000 frames
        let total_frames = sample_rate as usize * 6;
        let mut drums = vec![0.0_f32; total_frames * 2];
        // Place sharp 10ms bursts on beats 0..11
        for beat in 0..12 {
            let start = (beat as f64 * spb).round() as usize;
            for j in 0..480 {
                if start + j < total_frames {
                    let s = (1.0 - j as f32 / 480.0) * 0.9;
                    drums[(start + j) * 2] = s;
                    drums[(start + j) * 2 + 1] = s;
                }
            }
        }
        let mut stems_map = HashMap::new();
        stems_map.insert("drums".into(), crate::manifest::StemDef::File("drums.wav".into()));
        let song = LoadedSong {
            manifest: SongManifest {
                id: "impulse".into(),
                title: "Impulse".into(),
                artist: "".into(),
                bpm: base_bpm,
                time_signature: [4, 4],
                first_downbeat_sec: 0.0,
                original_key: "C".into(),
                provenance: None,
                click_mode: ClickMode::AlwaysOn,
                sections: vec![crate::manifest::SectionDef {
                    id: "s1".into(),
                    name: "Main".into(),
                    start_bar: 1,
                    end_bar: 5,
                    start_sec: None,
                    end_sec: None,
                    color: None,
                }],
                stems: stems_map,
            },
            sample_rate,
            stems: vec![DecodedStem {
                name: "drums".into(),
                pitched: false,
                is_cues: false,
                gain: 1.0,
                muted: false,
                samples: Arc::new(drums),
            }],
            total_frames,
        };

        for &(target_bpm, key_shift) in &[(120.0, 2), (150.0, 0), (90.0, 0), (180.0, 0)] {
            let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
            let mut state = MixerState::new_for_test(song.clone(), sample_rate, producer);
            state.is_playing = true;
            state.current_bpm = target_bpm;
            state.key_shift_semitones = key_shift;
            state.pitched_stretcher.set_semitones(key_shift as f32);
            state.prime_stretchers();

            let mut music_peaks = Vec::new();
            let mut click_starts = Vec::new();
            let mut rendered = 0usize;
            while rendered < sample_rate as usize * 3 {
                let block = state.render_block(512);
                for (i, f) in block.iter().enumerate() {
                    let abs_f = rendered + i;
                    if f.music_l.abs() > 0.25 {
                        if music_peaks.last().map_or(true, |&last| abs_f - last > 8000) {
                            music_peaks.push(abs_f);
                        }
                    }
                    if f.iem_l.abs() > 0.05 {
                        if click_starts.last().map_or(true, |&last| abs_f - last > 8000) {
                            click_starts.push(abs_f);
                        }
                    }
                }
                rendered += 512;
            }
            let rate = state.tempo_ratio();
            let eff_latency = state.stretched_output_latency_frames(rate);
            assert!(
                music_peaks.len() >= 4 && click_starts.len() >= 4,
                "Expected >= 4 impulses and clicks for bpm={target_bpm} key={key_shift}, got music={} click={}",
                music_peaks.len(),
                click_starts.len()
            );
            for beat_i in 0..4 {
                let diff = (music_peaks[beat_i] as isize - click_starts[beat_i] as isize).abs();
                assert!(
                    diff <= 96,
                    "Beat {beat_i} misaligned at bpm={target_bpm} key={key_shift}: music={} click={} (eff_latency={eff_latency}, diff={diff})",
                    music_peaks[beat_i],
                    click_starts[beat_i]
                );
            }
        }
    }

    #[test]
    fn test_pending_song_timing_applied_after_async_preload_and_click_toggle() {
        let sample_rate = 48000u32;
        let base_song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared_routing = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(base_song.clone(), sample_rate, producer);

        let song_dir = "/tmp/test_song_bundle".to_string();
        state.handle_command(EngineCommand::LoadSongDir(song_dir.clone()), &shared_routing);
        state.handle_command(
            EngineCommand::SetSongTiming {
                bpm: 138.0,
                first_downbeat_sec: 0.42,
                beats_per_bar: 3,
                click_mode: None,
                sections: None,
            },
            &shared_routing,
        );

        let mut loaded_song = base_song;
        loaded_song.manifest.bpm = 120.0;
        loaded_song.manifest.first_downbeat_sec = 0.0;
        state.handle_preload_result(PreloadResult {
            dir: song_dir,
            sample_rate,
            result: Ok(loaded_song),
        });

        assert!((state.song.manifest.bpm - 138.0).abs() < 1e-6);
        assert!((state.current_bpm - 138.0).abs() < 1e-6);
        assert!((state.song.manifest.first_downbeat_sec - 0.42).abs() < 1e-6);
        assert_eq!(state.song.manifest.time_signature[0], 3);

        assert!(!state.click_muted);
        state.handle_command(EngineCommand::ToggleClickMuted, &shared_routing);
        assert!(state.click_muted);
        state.handle_command(EngineCommand::ToggleClickMuted, &shared_routing);
        assert!(!state.click_muted);
    }

    #[test]
    fn test_transpose_key_name_handles_minor_and_unknown() {
        assert_eq!(transpose_key_name("G", 2), "A");
        assert_eq!(transpose_key_name("Em", 2), "F#m");
        assert_eq!(transpose_key_name("Am", -2), "Gm");
        assert_eq!(transpose_key_name("Unknown", 0), "Unknown");
        assert_eq!(transpose_key_name("Unknown", 2), "Unknown (+2)");
    }

    #[test]
    fn test_quantize_mode_serde() {
        let modes = [QuantizeMode::NextBar, QuantizeMode::EndOfSection];
        for mode in modes {
            let s = serde_json::to_string(&mode).unwrap();
            let parsed: QuantizeMode = serde_json::from_str(&s).unwrap();
            assert_eq!(mode, parsed);
        }
    }

    #[test]
    fn test_engine_command_serde() {
        let cmds = vec![
            EngineCommand::Play,
            EngineCommand::Stop,
            EngineCommand::PlayStop,
            EngineCommand::QueueSectionById("chorus".into()),
            EngineCommand::QueueSectionBySlot(2),
            EngineCommand::NextSection,
            EngineCommand::PrevSection,
            EngineCommand::ToggleLoop,
            EngineCommand::SetLoop(true),
            EngineCommand::SetQuantizeMode(QuantizeMode::EndOfSection),
            EngineCommand::SetKeyShift(2),
            EngineCommand::AdjustKeyShift(-1),
            EngineCommand::SetBpm(84.0),
            EngineCommand::AdjustBpm(2.0),
            EngineCommand::SetMasterGain(0.8),
            EngineCommand::SetClickGain(0.5),
            EngineCommand::SetClickMuted(true),
            EngineCommand::SetStemGain { name: "drums".into(), gain: 0.7 },
            EngineCommand::SetStemMuted { name: "drums".into(), muted: true },
            EngineCommand::SetPadEnabled(true),
            EngineCommand::SetPadKey("G".into()),
            EngineCommand::SetPadGain(0.6),
            EngineCommand::AddSetlistSong { dir: "/songs/1".into(), insert_at: None },
            EngineCommand::NextSong,
            EngineCommand::PrevSong,
        ];
        for cmd in cmds {
            let json = serde_json::to_string(&cmd).expect("serialize EngineCommand");
            let _parsed: EngineCommand = serde_json::from_str(&json).expect("deserialize EngineCommand");
        }
    }

    #[test]
    fn test_stem_mute_silences_stem() {
        let sample_rate = 48000;
        let mut song = generate_demo_song(sample_rate);
        for stem in song.stems.iter_mut() {
            if stem.name != "music" {
                stem.gain = 0.0;
            } else {
                stem.gain = 1.0;
                stem.muted = false;
            }
        }
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.is_playing = true;
        state.click_gain = 0.0;
        state.click_muted = true;

        let block_unmuted = state.render_block(256);
        let energy_unmuted: f32 = block_unmuted.iter().map(|f| f.music_l.abs()).sum();
        assert!(energy_unmuted > 0.01, "Unmuted stem should produce energy");

        if let Some(music) = state.song.stems.iter_mut().find(|s| s.name == "music") {
            music.muted = true;
        }
        let block_muted = state.render_block(256);
        let energy_muted: f32 = block_muted.iter().map(|f| f.music_l.abs()).sum();
        assert_eq!(energy_muted, 0.0, "Muted stem should produce zero energy");
    }

    #[test]
    fn test_pad_sustains_when_transport_stopped() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.is_playing = false;
        state.pad.enabled = true;
        state.pad.gain = 0.6;

        // Render several blocks to ramp up pad envelope while transport is stopped.
        let mut music_energy = 0.0_f32;
        let mut iem_energy = 0.0_f32;
        for _ in 0..40 {
            let block = state.render_block(512);
            for frame in block {
                music_energy += frame.music_l.abs() + frame.music_r.abs();
                iem_energy += frame.iem_l.abs() + frame.iem_r.abs();
            }
        }
        assert!(music_energy > 1.0, "Pad must sustain in music bus while stopped");
        assert_eq!(iem_energy, 0.0, "Pad must never leak into click/cues bus");
    }

    #[test]
    fn test_setlist_flow_auto_pause_and_auto_advance() {
        let sample_rate = 48000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "song-a".into();
        song_a.manifest.title = "Song A".into();
        song_a.total_frames = 1024;

        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "song-b".into();
        song_b.manifest.title = "Song B".into();
        song_b.manifest.original_key = "C".into();
        song_b.total_frames = 1024;

        let mut song_c = generate_demo_song(sample_rate);
        song_c.manifest.id = "song-c".into();
        song_c.manifest.title = "Song C".into();
        song_c.manifest.original_key = "D".into();
        song_c.total_frames = 1024;

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/a".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/b".into(), CacheEntry::Ready(song_b));
        state.song_cache.insert("/songs/c".into(), CacheEntry::Ready(song_c));

        state.setlist_items = vec![
            SetlistItem {
                song_id: "song-a".into(),
                song_dir: Some("/songs/a".into()),
                title: Some("Song A".into()),
                original_key: Some("G".into()),
                base_bpm: Some(72.0),
                base_click_mode: Some(ClickMode::AlwaysOn),
                key_shift_semitones: 0,
                bpm_override: None,
                click_mode: None,
                transition_mode: TransitionMode::AutoPause,
                crossfade_sec: 2.0,
                ambient_pad_enabled: true,
                pad_key_override: None,
                pad_lock: None,
                ..Default::default()
            },
            SetlistItem {
                song_id: "song-b".into(),
                song_dir: Some("/songs/b".into()),
                title: Some("Song B".into()),
                original_key: Some("C".into()),
                base_bpm: Some(72.0),
                base_click_mode: Some(ClickMode::AlwaysOn),
                key_shift_semitones: 2, // C -> D
                bpm_override: Some(78.0),
                click_mode: None,
                transition_mode: TransitionMode::AutoAdvance,
                crossfade_sec: 2.0,
                ambient_pad_enabled: true,
                pad_key_override: None,
                pad_lock: None,
                ..Default::default()
            },
            SetlistItem {
                song_id: "song-c".into(),
                song_dir: Some("/songs/c".into()),
                title: Some("Song C".into()),
                original_key: Some("D".into()),
                base_bpm: Some(72.0),
                base_click_mode: Some(ClickMode::AlwaysOn),
                key_shift_semitones: 0,
                bpm_override: None,
                click_mode: None,
                transition_mode: TransitionMode::Manual,
                crossfade_sec: 2.0,
                ambient_pad_enabled: false,
                pad_key_override: None,
                pad_lock: None,
                ..Default::default()
            },
        ];

        // Start Song A playing near end -> AutoPause loads Song B, engages pad in D, pauses
        state.activate_cached_slot(0, true, false);
        state.source_frame_pos = 900.0;
        state.render_block(256);

        assert_eq!(state.active_setlist_slot, Some(1));
        assert_eq!(state.song.manifest.id, "song-b");
        assert!(!state.is_playing, "AutoPause should pause on Song B ready");
        assert_eq!(state.key_shift_semitones, 2);
        assert_eq!(state.current_bpm, 78.0);
        assert!(state.pad.enabled, "AutoPause should engage ambient pad");
        assert_eq!(state.pad.current_key(), "D");

        // Advance Song B near end -> AutoAdvance switches to Song C and keeps playing
        state.is_playing = true;
        state.source_frame_pos = 900.0;
        state.render_block(256);

        assert_eq!(state.active_setlist_slot, Some(2));
        assert_eq!(state.song.manifest.id, "song-c");
        assert!(state.is_playing, "AutoAdvance should keep playing Song C");
    }

    #[test]
    fn test_setlist_reorder_preserves_active_song() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song.clone(), sample_rate, producer);
        for d in ["/s1", "/s2", "/s3"] {
            state.song_cache.insert(d.into(), CacheEntry::Ready(song.clone()));
            state.handle_command(
                EngineCommand::AddSetlistSong { dir: d.into(), insert_at: None },
                &shared,
            );
        }
        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        assert_eq!(state.active_setlist_slot, Some(0));

        // Move slot 0 -> slot 2
        state.handle_command(EngineCommand::MoveSetlistSlot { from: 0, to: 2 }, &shared);
        assert_eq!(state.active_setlist_slot, Some(2));
        assert_eq!(state.setlist_items[2].song_dir.as_deref(), Some("/s1"));
    }

    #[test]
    fn test_engine_seek_sec_updates_position_and_section() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song.clone(), sample_rate, producer);
        // Demo song is 72 BPM (3.333 sec/bar); Verse starts at bar 5 (~13.33s)
        let target_sec = song.manifest.bar_to_seconds(5) + 0.5;
        state.handle_command(EngineCommand::SeekSec(target_sec), &shared);
        assert!((state.source_frame_pos / sample_rate as f64 - target_sec).abs() < 0.01);
        assert_eq!(state.active_section_idx, Some(1));

        // Clamping beyond total_frames
        state.handle_command(EngineCommand::SeekSec(9999.0), &shared);
        assert_eq!(state.source_frame_pos.round() as usize, song.total_frames);
    }

    #[test]
    fn test_engine_set_song_timing_syncs_click_and_bar_grid() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.handle_command(
            EngineCommand::SetSongTiming {
                bpm: 96.0,
                first_downbeat_sec: 1.25,
                beats_per_bar: 3,
                click_mode: None,
                sections: Some(vec![
                    SectionDef {
                        id: "intro".into(),
                        name: "Intro".into(),
                        start_bar: 1,
                        end_bar: 5,
                        start_sec: None,
                        end_sec: None,
                        color: Some("slate".into()),
                    },
                    SectionDef {
                        id: "chorus-dynamic".into(),
                        name: "Chorus".into(),
                        start_bar: 5,
                        end_bar: 13,
                        start_sec: None,
                        end_sec: None,
                        color: Some("emerald".into()),
                    },
                ]),
            },
            &shared,
        );
        assert_eq!(state.current_bpm, 96.0);
        assert_eq!(state.song.manifest.bpm, 96.0);
        assert!((state.song.manifest.first_downbeat_sec - 1.25).abs() < 1e-6);
        assert_eq!(state.song.manifest.time_signature[0], 3);
        assert_eq!(state.song.manifest.sections.len(), 2);
        assert_eq!(state.song.manifest.sections[1].name, "Chorus");
        assert!(state.is_stretch_bypassed());
    }

    #[test]
    fn test_route_bus_frame_split_and_four_channel_stereo() {
        let frame = BusFrame {
            music_l: 0.8,
            music_r: 0.2,
            iem_l: -0.6,
            iem_r: -0.2,
        };
        let master = 0.5;
        // Expected mono music = (0.8 + 0.2) * 0.5 * 0.5 = 0.25
        // Expected stereo music = (0.4, 0.1)
        // Expected mono iem = (-0.6 + -0.2) * 0.5 = -0.4
        // Expected stereo iem = (-0.6, -0.2)

        let mut out2 = [0.0_f32; 2];
        route_bus_frame(frame, OutputRoutingMode::SplitLeftRight, master, &mut out2);
        assert!((out2[0] - 0.25).abs() < 1e-6);
        assert!((out2[1] - (-0.4)).abs() < 1e-6);

        route_bus_frame(frame, OutputRoutingMode::SplitClickLeftMusicRight, master, &mut out2);
        assert!((out2[0] - (-0.4)).abs() < 1e-6);
        assert!((out2[1] - 0.25).abs() < 1e-6);

        let mut out4 = [0.0_f32; 4];
        route_bus_frame(frame, OutputRoutingMode::MultiOutStereo, master, &mut out4);
        assert!((out4[0] - 0.4).abs() < 1e-6);
        assert!((out4[1] - 0.1).abs() < 1e-6);
        assert!((out4[2] - (-0.6)).abs() < 1e-6);
        assert!((out4[3] - (-0.2)).abs() < 1e-6);

        route_bus_frame(frame, OutputRoutingMode::MultiOutClickLeftMusicRight, master, &mut out4);
        assert!((out4[0] - (-0.6)).abs() < 1e-6);
        assert!((out4[1] - (-0.2)).abs() < 1e-6);
        assert!((out4[2] - 0.4).abs() < 1e-6);
        assert!((out4[3] - 0.1).abs() < 1e-6);

        // Fallback on 2-channel devices preserves side preference
        route_bus_frame(frame, OutputRoutingMode::MultiOutStereo, master, &mut out2);
        assert!((out2[0] - 0.25).abs() < 1e-6);
        assert!((out2[1] - (-0.4)).abs() < 1e-6);

        route_bus_frame(frame, OutputRoutingMode::MultiOutClickLeftMusicRight, master, &mut out2);
        assert!((out2[0] - (-0.4)).abs() < 1e-6);
        assert!((out2[1] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn test_click_mode_intro_only_auto_mutes_after_intro_and_resets_on_seek_or_stop() {
        let sample_rate = 48000;
        let mut song = generate_demo_song(sample_rate);
        // Set 120 BPM (2.0 s/bar), Intro = bars 1..5 -> intro ends at 8.0s
        song.manifest.bpm = 120.0;
        song.manifest.first_downbeat_sec = 0.0;
        song.manifest.click_mode = ClickMode::IntroOnly;
        for stem in &mut song.stems {
            stem.muted = true;
        }
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.current_bpm = 120.0;

        assert_eq!(state.active_click_mode, ClickMode::IntroOnly);
        assert!(!state.click_muted, "IntroOnly starts with click unmuted");

        // Play near the start of Intro (0.0s) -> click triggers and produces IEM audio
        state.handle_command(EngineCommand::Play, &shared);
        let intro_block = state.render_block(512);
        let intro_iem: f32 = intro_block.iter().map(|f| f.iem_l.abs()).sum();
        assert!(intro_iem > 0.01, "Click should sound during Intro");
        assert!(!state.click_muted);

        // Seek near end of Bar 4 (7.95s, beat 15 already active & finished decaying at 7.98s)
        // Wait: render once at 7.85s (within beat 15) for 0.14s so beat 15 decays,
        // then render across intro_end_sec (8.0s, beat 16 / Verse 1 downbeat).
        state.handle_command(EngineCommand::SeekSec(7.85), &shared);
        for _ in 0..27 {
            let _ = state.render_block(256);
        }
        assert!(!state.click_muted, "Still inside intro before 8.0s");
        let cross_block = state.render_block(1024); // crosses 8.0s (Verse 1 beat 1)
        let cross_iem: f32 = cross_block.iter().map(|f| f.iem_l.abs()).sum();
        assert_eq!(
            cross_iem, 0.0,
            "Verse 1 downbeat at intro_end_sec must not click"
        );
        assert!(
            state.click_muted && state.intro_click_auto_muted,
            "Click must auto-mute once intro finishes"
        );

        // Seeking back into Intro re-arms the click automatically
        state.handle_command(EngineCommand::SeekSec(1.0), &shared);
        assert!(!state.click_muted && !state.intro_click_auto_muted);

        // Crossing into Verse again mutes click, and Stop resets click back ON for Intro
        state.handle_command(EngineCommand::SeekSec(8.5), &shared);
        assert!(state.click_muted && state.intro_click_auto_muted);
        state.handle_command(EngineCommand::Stop, &shared);
        assert!(!state.click_muted && !state.intro_click_auto_muted);
    }

    #[test]
    fn test_setlist_slots_apply_per_song_click_mode_and_bpm() {
        let sample_rate = 48000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "song-a".into();
        song_a.manifest.bpm = 72.0;
        song_a.manifest.click_mode = ClickMode::IntroOnly;

        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "song-b".into();
        song_b.manifest.bpm = 128.0;
        song_b.manifest.click_mode = ClickMode::AlwaysOn;

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/a".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/b".into(), CacheEntry::Ready(song_b));

        state.handle_command(
            EngineCommand::AddSetlistSong {
                dir: "/songs/a".into(),
                insert_at: None,
            },
            &shared,
        );
        state.handle_command(
            EngineCommand::AddSetlistSong {
                dir: "/songs/b".into(),
                insert_at: None,
            },
            &shared,
        );
        state.handle_command(
            EngineCommand::UpdateSetlistSlotConfig {
                slot: 1,
                key_shift_semitones: None,
                bpm_override: Some(Some(132.0)),
                transition_mode: None,
                crossfade_sec: None,
                ambient_pad_enabled: None,
                pad_key_override: None,
                pad_lock: None,
                click_mode: Some(Some(ClickMode::Off)),
                section_id: None,
                percussion_lock: None,
            },
            &shared,
        );

        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        assert_eq!(state.current_bpm, 72.0);
        assert_eq!(state.active_click_mode, ClickMode::IntroOnly);
        assert!(!state.click_muted);

        state.handle_command(EngineCommand::SelectSetlistSlot(1), &shared);
        assert_eq!(state.current_bpm, 132.0);
        assert_eq!(state.active_click_mode, ClickMode::Off);
        assert!(state.click_muted);

        // Live click mode tweak on active slot updates slot override
        state.handle_command(EngineCommand::SetClickMode(ClickMode::IntroOnly), &shared);
        assert_eq!(state.setlist_items[1].click_mode, Some(ClickMode::IntroOnly));
        assert!(!state.click_muted);
    }

    #[test]
    fn test_decimal_timeline_section_tracking_and_jumps() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.handle_command(
            EngineCommand::SetSongTiming {
                bpm: 120.0,
                first_downbeat_sec: 0.0,
                beats_per_bar: 4,
                click_mode: None,
                sections: Some(vec![
                    SectionDef {
                        id: "intro".into(),
                        name: "Intro".into(),
                        start_bar: 1,
                        end_bar: 2,
                        start_sec: Some(0.35),
                        end_sec: Some(1.62),
                        color: Some("slate".into()),
                    },
                    SectionDef {
                        id: "verse".into(),
                        name: "Verse 1".into(),
                        start_bar: 2,
                        end_bar: 4,
                        start_sec: Some(1.62),
                        end_sec: Some(4.85),
                        color: Some("sky".into()),
                    },
                ]),
            },
            &shared,
        );

        // Seeking to 1.50s lands inside Intro even though 120 BPM Bar 2 starts at 2.0s; 1.65s lands in Verse 1.
        state.handle_command(EngineCommand::SeekSec(1.50), &shared);
        assert_eq!(state.active_section_idx, Some(0));
        state.handle_command(EngineCommand::SeekSec(1.65), &shared);
        assert_eq!(state.active_section_idx, Some(1));

        // Stopped jump seeks directly to decimal start_sec (1.62s) instead of snapping to bar grid (2.00s).
        state.handle_command(EngineCommand::QueueSectionById("intro".into()), &shared);
        let pos_sec = state.source_frame_pos / sample_rate as f64;
        assert!((pos_sec - 0.35).abs() < 1e-4);

        // Playing with EndOfSection quantize crosses end_sec (1.62s) cleanly and jumps straight to target start_sec.
        state.handle_command(EngineCommand::SetQuantizeMode(QuantizeMode::EndOfSection), &shared);
        state.handle_command(EngineCommand::SeekSec(1.58), &shared);
        state.handle_command(EngineCommand::Play, &shared);
        state.handle_command(EngineCommand::QueueSectionById("verse".into()), &shared);
        for _ in 0..10 {
            state.render_block(256);
        }
        assert_eq!(state.active_section_idx, Some(1));
        assert!(state.armed_section_idx.is_none());
    }

    #[test]
    fn test_setlist_preset_lock_and_passthrough() {
        use crate::pad::{PadRegister, PadTonality, PadVoiceArchetype};

        let sample_rate = 48000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "song-a".into();
        song_a.manifest.original_key = "Am".into();

        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "song-b".into();
        song_b.manifest.original_key = "G".into();

        let mut song_c = generate_demo_song(sample_rate);
        song_c.manifest.id = "song-c".into();
        song_c.manifest.original_key = "D".into();

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/a".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/b".into(), CacheEntry::Ready(song_b));
        state.song_cache.insert("/songs/c".into(), CacheEntry::Ready(song_c));

        let locked_patch = PadLockConfig {
            enabled: true,
            gain: 0.72,
            preset_id: Some("reverse-piano".into()),
            preset_name: Some("Reverse Piano".into()),
            layers: vec![PadLayerConfig {
                enabled: true,
                archetype: PadVoiceArchetype::ReversePiano,
                register: PadRegister::High,
                tonality: PadTonality::Auto,
                gain: 0.88,
            }],
            brightness: 0.78,
            shimmer: 0.61,
            pan: -0.15,
        };

        state.setlist_items = vec![
            SetlistItem {
                song_id: "song-a".into(),
                song_dir: Some("/songs/a".into()),
                title: Some("Song A".into()),
                original_key: Some("Am".into()),
                base_bpm: Some(72.0),
                base_click_mode: None,
                key_shift_semitones: 0,
                bpm_override: None,
                click_mode: None,
                transition_mode: TransitionMode::AutoPause,
                crossfade_sec: 2.0,
                ambient_pad_enabled: true,
                pad_key_override: None,
                pad_lock: Some(locked_patch),
                ..Default::default()
            },
            SetlistItem {
                song_id: "song-b".into(),
                song_dir: Some("/songs/b".into()),
                title: Some("Song B (Pass-Through)".into()),
                original_key: Some("G".into()),
                base_bpm: Some(72.0),
                base_click_mode: None,
                key_shift_semitones: 0,
                bpm_override: None,
                click_mode: None,
                transition_mode: TransitionMode::AutoPause,
                crossfade_sec: 2.0,
                ambient_pad_enabled: true,
                pad_key_override: None,
                pad_lock: None,
                ..Default::default()
            },
            SetlistItem {
                song_id: "song-c".into(),
                song_dir: Some("/songs/c".into()),
                title: Some("Song C".into()),
                original_key: Some("D".into()),
                base_bpm: Some(72.0),
                base_click_mode: None,
                key_shift_semitones: 0,
                bpm_override: None,
                click_mode: None,
                transition_mode: TransitionMode::Manual,
                crossfade_sec: 2.0,
                ambient_pad_enabled: true,
                pad_key_override: None,
                pad_lock: None,
                ..Default::default()
            },
        ];

        // Slot 0 has PadLockConfig: recalls Reverse Piano, brightness 0.78, shimmer 0.61, while following song key Am
        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        assert!(state.pad.enabled);
        assert_eq!(state.pad.current_key(), "Am");
        assert!(state.pad.is_minor());
        assert_eq!(state.pad.preset_name(), "Reverse Piano");
        assert!((state.pad.brightness - 0.78).abs() < 1e-4);
        assert!((state.pad.shimmer - 0.61).abs() < 1e-4);

        // Live tweak on slot 0 (lower gain & shimmer) before moving to unlocked slot 1 (Pass-Through)
        state.handle_command(EngineCommand::SetPadGain(0.55), &shared);
        state.handle_command(EngineCommand::SetPadShimmer(0.35), &shared);

        // Slot 1 is Pass-Through (pad_lock: None): carries live tweaks forward, only shifting key to G
        state.handle_command(EngineCommand::SelectSetlistSlot(1), &shared);
        assert_eq!(state.pad.current_key(), "G");
        assert!(!state.pad.is_minor());
        assert_eq!(state.pad.preset_name(), "Reverse Piano");
        assert!((state.pad.gain - 0.55).abs() < 1e-4);
        assert!((state.pad.shimmer - 0.35).abs() < 1e-4);

        // Lock current live pad state to slot 2, then clear and verify
        state.handle_command(EngineCommand::LockCurrentPadToSlot(2), &shared);
        assert!(state.setlist_items[2].pad_lock.is_some());
        state.handle_command(EngineCommand::ClearSlotPadLock(2), &shared);
        assert!(state.setlist_items[2].pad_lock.is_none());
    }

    #[test]
    fn test_batch_update_sections_and_standalone_slot_loop() {
        let sample_rate = 48000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "song-a".into();
        song_a.manifest.original_key = "C".into();

        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "song-b".into();
        song_b.manifest.original_key = "G".into();

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/a".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/b".into(), CacheEntry::Ready(song_b));

        state.setlist_items = vec![
            SetlistItem {
                song_id: "song-a".into(),
                song_dir: Some("/songs/a".into()),
                title: Some("Song A".into()),
                original_key: Some("C".into()),
                base_bpm: Some(72.0),
                ..Default::default()
            },
            SetlistItem {
                song_id: "song-b".into(),
                song_dir: Some("/songs/b".into()),
                title: Some("Song B".into()),
                original_key: Some("G".into()),
                base_bpm: Some(120.0),
                ..Default::default()
            },
        ];

        // Add Worship and Sermon sections
        state.handle_command(
            EngineCommand::UpsertSetlistSection(crate::manifest::SetlistSection {
                id: "sec-worship".into(),
                name: "Worship".into(),
                kind: crate::manifest::ServiceSectionKind::Worship,
                color: Some("#8b5cf6".into()),
                collapsed: false,
                note: None,
            }),
            &shared,
        );
        state.handle_command(
            EngineCommand::UpsertSetlistSection(crate::manifest::SetlistSection {
                id: "sec-sermon".into(),
                name: "Sermon".into(),
                kind: crate::manifest::ServiceSectionKind::Sermon,
                color: Some("#38bdf8".into()),
                collapsed: false,
                note: None,
            }),
            &shared,
        );

        // Batch edit both slots: key -> D, click -> Off, pad -> ON, section -> Worship, percussion -> Layer + HighlifePraise
        state.handle_command(
            EngineCommand::BatchUpdateSetlistSlots(SetlistBatchPatch {
                slots: vec![0, 1],
                target_key: Some("D".into()),
                key_shift_semitones: None,
                bpm_override: Some(Some(84.0)),
                click_mode: Some(Some(ClickMode::Off)),
                transition_mode: Some(TransitionMode::AutoAdvance),
                crossfade_sec: Some(3.0),
                ambient_pad_enabled: Some(true),
                pad_lock: None,
                section_id: Some(Some("sec-worship".into())),
                percussion_mode: Some(crate::percussion_patterns::PercussionMode::Layer),
                percussion_style: Some(crate::percussion_patterns::PercussionStyle::HighlifePraise),
                percussion_intensity: Some(crate::percussion_patterns::GrooveIntensity::High),
                percussion_custom_pattern: None,
            }),
            &shared,
        );
        assert_eq!(state.setlist_items[0].key_shift_semitones, 2); // C -> D
        assert_eq!(state.setlist_items[1].key_shift_semitones, -5); // G -> D
        assert_eq!(state.setlist_items[0].click_mode, Some(ClickMode::Off));
        assert_eq!(state.setlist_items[0].bpm_override, Some(84.0));
        assert_eq!(state.setlist_items[0].crossfade_sec, 3.0);
        assert_eq!(state.setlist_items[0].section_id.as_deref(), Some("sec-worship"));
        assert_eq!(
            state.setlist_items[1].percussion_lock.as_ref().unwrap().style,
            crate::percussion_patterns::PercussionStyle::HighlifePraise
        );

        // Verify JSON deserialization of triple-state Option<Option<ClickMode>> in BatchUpdateSetlistSlots
        let cmd_clear_click: EngineCommand = serde_json::from_str(
            r#"{"type":"BatchUpdateSetlistSlots","payload":{"slots":[0,1],"click_mode":null}}"#,
        )
        .expect("deserialize explicit null click_mode");
        state.handle_command(cmd_clear_click, &shared);
        assert_eq!(state.setlist_items[0].click_mode, None);
        assert_eq!(state.setlist_items[1].click_mode, None);

        let cmd_off_click: EngineCommand = serde_json::from_str(
            r#"{"type":"BatchUpdateSetlistSlots","payload":{"slots":[0,1],"click_mode":"Off"}}"#,
        )
        .expect("deserialize ClickMode::Off");
        state.handle_command(cmd_off_click, &shared);
        assert_eq!(state.setlist_items[0].click_mode, Some(ClickMode::Off));
        assert_eq!(state.setlist_items[1].click_mode, Some(ClickMode::Off));

        // Removing section clears item section_id
        state.handle_command(EngineCommand::RemoveSetlistSection("sec-worship".into()), &shared);
        assert!(state.setlist_items[0].section_id.is_none());

        // Add standalone PadOnly sermon slot and verify continuous looping
        state.handle_command(
            EngineCommand::AddStandaloneSetlistSlot {
                title: "Sermon Pad".into(),
                slot_kind: crate::manifest::SetlistSlotKind::PadOnly,
                key: "Eb".into(),
                bpm: 68.0,
                time_signature: [4, 4],
                section_id: Some("sec-sermon".into()),
                insert_at: None,
            },
            &shared,
        );
        assert_eq!(state.setlist_items.len(), 3);
        state.handle_command(EngineCommand::SelectSetlistSlot(2), &shared);
        assert!(state.pad.enabled);
        assert_eq!(state.pad.current_key(), "Eb");
        state.is_playing = true;

        // Render past the 8-bar window (~28s at 68 BPM); should wrap and keep playing cleanly
        let frames_to_render = (sample_rate as usize) * 32;
        let mut rendered = 0usize;
        let mut pad_energy = 0.0_f32;
        while rendered < frames_to_render {
            let block = state.render_block(1024);
            for f in block {
                pad_energy += f.music_l.abs();
            }
            rendered += 1024;
        }
        assert!(state.is_playing, "Standalone PadOnly slot must loop continuously without auto-stopping");
        assert!(pad_energy > 10.0, "Standalone PadOnly slot must output pad audio");
    }

    #[test]
    fn test_song_crossfade_overlap_and_iem_isolation() {
        let sample_rate = 48000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "song-a".into();
        song_a.manifest.title = "Song A".into();
        song_a.manifest.bpm = 120.0;
        song_a.manifest.click_mode = ClickMode::AlwaysOn;

        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "song-b".into();
        song_b.manifest.title = "Song B".into();
        song_b.manifest.bpm = 90.0;
        song_b.manifest.click_mode = ClickMode::Off; // B has click OFF to verify A's click stops cleanly during crossfade

        // Silence all stems in B so any music output during crossfade proves A is fading out cleanly,
        // or inspect outgoing_title/progress + zero IEM click during fade.
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/a".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/b".into(), CacheEntry::Ready(song_b));
        state.setlist_items = vec![
            SetlistItem {
                song_id: "song-a".into(),
                song_dir: Some("/songs/a".into()),
                title: Some("Song A".into()),
                original_key: Some("C".into()),
                base_bpm: Some(120.0),
                transition_mode: TransitionMode::AutoAdvance,
                crossfade_sec: 1.0,
                ..Default::default()
            },
            SetlistItem {
                song_id: "song-b".into(),
                song_dir: Some("/songs/b".into()),
                title: Some("Song B".into()),
                original_key: Some("C".into()),
                base_bpm: Some(90.0),
                click_mode: Some(ClickMode::Off),
                transition_mode: TransitionMode::Manual,
                crossfade_sec: 1.0,
                ..Default::default()
            },
        ];

        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        state.is_playing = true;
        // Render 1 block so A is audible, then trigger NextSong while playing
        let _ = state.render_block(512);
        state.handle_command(EngineCommand::NextSong, &shared);

        assert_eq!(state.active_setlist_slot, Some(1));
        assert!(state.song_crossfade.active, "Manual NextSong while playing with crossfade_sec > 0 must start overlap crossfade");
        assert!(state.is_playing);

        // During crossfade, B has ClickMode::Off -> IEM click from Song A must NOT leak into IEM
        let mut iem_energy_during_fade = 0.0_f32;
        let mut music_energy_during_fade = 0.0_f32;
        for _ in 0..25 {
            let block = state.render_block(1024);
            for f in block {
                iem_energy_during_fade += f.iem_l.abs();
                music_energy_during_fade += f.music_l.abs();
            }
        }
        assert!(music_energy_during_fade > 1.0, "Crossfade must produce smooth music output");
        assert!(iem_energy_during_fade < 1e-4, "Outgoing click from A must not bleed into IEM while B has click Off");

        // Render remainder of 1.0s fade so crossfade deck clears automatically
        for _ in 0..30 {
            let _ = state.render_block(1024);
        }
        assert!(!state.song_crossfade.active, "Crossfade deck must complete and clear after crossfade_sec");

        // Now test zero-crossfade hard cut
        state.setlist_items[1].crossfade_sec = 0.0;
        state.handle_command(EngineCommand::PrevSong, &shared);
        assert!(!state.song_crossfade.active);
    }

    #[test]
    fn test_percussion_only_mutes_backing_stems_and_follows_section_intensity() {
        let sample_rate = 48000;
        let mut song = generate_demo_song(sample_rate);
        song.manifest.sections = vec![
            crate::manifest::SectionDef {
                id: "v1".into(),
                name: "Verse 1".into(),
                start_bar: 1,
                end_bar: 2,
                start_sec: None,
                end_sec: None,
                color: None,
            },
            crate::manifest::SectionDef {
                id: "ch1".into(),
                name: "Chorus".into(),
                start_bar: 2,
                end_bar: 6,
                start_sec: None,
                end_sec: None,
                color: None,
            },
        ];

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        state.pad.enabled = false;
        state.click_muted = true;
        state.active_click_mode = ClickMode::Off;
        state.handle_command(
            EngineCommand::SetPercussionStyle(crate::percussion_patterns::PercussionStyle::Afrobeat),
            &shared,
        );
        state.handle_command(
            EngineCommand::SetPercussionMode(crate::percussion_patterns::PercussionMode::PercussionOnly),
            &shared,
        );
        state.handle_command(EngineCommand::SetPercussionFollowSections(true), &shared);
        state.is_playing = true;

        // Render bar 1 (Verse 1 -> Low intensity)
        let _ = state.render_block(1024);
        assert_eq!(
            state.percussion.intensity,
            crate::percussion_patterns::GrooveIntensity::Low
        );

        // Seek into Chorus (bar 2+) -> Medium intensity
        state.handle_command(EngineCommand::SeekSec(4.0), &shared);
        let _ = state.render_block(1024);
        assert_eq!(
            state.percussion.intensity,
            crate::percussion_patterns::GrooveIntensity::Medium
        );

        // Verify PercussionOnly silences music stems when percussion master gain is 0.0
        state.handle_command(EngineCommand::SetPercussionGain(0.0), &shared);
        let silent_block = state.render_block(1024);
        let max_music = silent_block.iter().map(|f| f.music_l.abs().max(f.music_r.abs())).fold(0.0_f32, f32::max);
        assert!(
            max_music < 1e-5,
            "PercussionOnly must mute non-cue backing stems cleanly (got max_music={})",
            max_music
        );
    }

    #[test]
    fn test_restore_session_preserves_setlist_and_stage_configs() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);

        let saved = crate::session::StageSessionState {
            routing_mode: OutputRoutingMode::SplitClickLeftMusicRight,
            setlist: SetlistManifest {
                id: "restored".into(),
                name: "Evening Worship".into(),
                sections: vec![SetlistSection {
                    id: "sec-ministry".into(),
                    name: "Ministry".into(),
                    kind: crate::manifest::ServiceSectionKind::Ministry,
                    color: None,
                    collapsed: false,
                    note: None,
                }],
                items: vec![
                    SetlistItem {
                        song_id: "pad-1".into(),
                        song_dir: None,
                        title: Some("Atmosphere Pad".into()),
                        original_key: Some("D".into()),
                        base_bpm: Some(68.0),
                        base_click_mode: Some(ClickMode::Off),
                        slot_kind: SetlistSlotKind::PadOnly,
                        ambient_pad_enabled: true,
                        section_id: Some("sec-ministry".into()),
                        ..SetlistItem::default()
                    },
                    SetlistItem {
                        song_id: "perc-2".into(),
                        song_dir: None,
                        title: Some("Offering Groove".into()),
                        original_key: Some("G".into()),
                        base_bpm: Some(104.0),
                        base_click_mode: Some(ClickMode::AlwaysOn),
                        slot_kind: SetlistSlotKind::PercussionGroove,
                        section_id: Some("sec-ministry".into()),
                        ..SetlistItem::default()
                    },
                ],
                mixer: None,
            },
            active_setlist_slot: Some(1),
            master_gain: 0.78,
            click_gain: 0.52,
            click_muted: true,
            loop_enabled: true,
            quantize_mode: QuantizeMode::EndOfSection,
            pad_gain: 0.64,
            pad_key: "Eb".into(),
            pad_is_minor: true,
            pad_follow_song_key: false,
            pad_brightness: 0.72,
            pad_shimmer: 0.48,
            pad_pan: -0.25,
            percussion_style: crate::percussion_patterns::PercussionStyle::Afrobeat,
            percussion_gain: 0.88,
            percussion_iem_send: 0.42,
            percussion_channels: vec![PercussionChannelConfig {
                instrument: crate::percussion_patterns::PercussionInstrument::HandDrums,
                enabled: false,
                gain: 0.91,
                pan: 0.4,
            }],
            ..crate::session::StageSessionState::default()
        };

        state.handle_command(EngineCommand::RestoreSession(saved), &shared);

        assert!(!state.is_playing);
        assert!(!state.pad.enabled, "cold launch should never auto-blast tonic pad");
        assert_eq!(state.setlist_name, "Evening Worship");
        assert_eq!(state.setlist_sections.len(), 1);
        assert_eq!(state.setlist_items.len(), 2);
        assert_eq!(state.active_setlist_slot, Some(1));
        assert_eq!(state.song.manifest.title, "Offering Groove");
        assert!((state.current_bpm - 104.0).abs() < 0.01);
        assert!(state.click_muted);
        assert!(state.loop_enabled);
        assert_eq!(state.quantize_mode, QuantizeMode::EndOfSection);
        assert!((shared.read().unwrap().master_gain - 0.78).abs() < 0.001);
        assert_eq!(
            shared.read().unwrap().mode,
            OutputRoutingMode::SplitClickLeftMusicRight
        );
        assert!((state.click_gain - 0.52).abs() < 0.001);
        assert_eq!(state.pad.current_key(), "Ebm");
        assert!(!state.pad.follow_song_key);
        assert!((state.pad.gain - 0.64).abs() < 0.001);
        assert!((state.pad.brightness - 0.72).abs() < 0.001);
        assert!((state.pad.pan + 0.25).abs() < 0.001);
        assert_eq!(
            state.percussion.style,
            crate::percussion_patterns::PercussionStyle::Afrobeat
        );
        assert_eq!(
            state.percussion.mode,
            crate::percussion_patterns::PercussionMode::PercussionOnly
        );
        assert!((state.percussion.iem_send - 0.42).abs() < 0.001);
        let hand_drums = state
            .percussion
            .channels
            .iter()
            .find(|c| c.instrument == crate::percussion_patterns::PercussionInstrument::HandDrums)
            .expect("hand drums channel present");
        assert!(!hand_drums.enabled);
        assert!((hand_drums.gain - 0.91).abs() < 0.001);
        assert!((hand_drums.pan - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_percussion_audition_plays_when_transport_stopped_and_mode_off() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);
        assert_eq!(state.percussion.mode, crate::percussion_patterns::PercussionMode::Off);
        assert!(!state.is_playing);

        state.handle_command(
            EngineCommand::StartPercussionAudition {
                bpm: 120.0,
                beats_per_bar: 4,
                with_click: true,
            },
            &shared,
        );
        assert!(state.percussion_audition_playing);
        assert!(state.has_active_audio());

        let mut music_peak = 0.0_f32;
        let mut iem_peak = 0.0_f32;
        for _ in 0..30 {
            for f in state.render_block(1024) {
                music_peak = music_peak.max(f.music_l.abs()).max(f.music_r.abs());
                iem_peak = iem_peak.max(f.iem_l.abs()).max(f.iem_r.abs());
                assert!(f.music_l.is_finite() && f.music_r.is_finite());
            }
        }
        assert!(music_peak > 0.05 && music_peak <= 1.2, "audition peak={}", music_peak);
        assert!(iem_peak > 0.02, "with_click should sound on IEM, peak={}", iem_peak);

        state.handle_command(EngineCommand::SetPercussionAuditionBpm(136.0), &shared);
        assert!((state.percussion_audition_bpm - 136.0).abs() < 0.01);

        // Starting normal stage playback cleanly stops independent loop audition
        state.handle_command(EngineCommand::Play, &shared);
        assert!(!state.percussion_audition_playing);
        assert!(state.is_playing);

        state.handle_command(EngineCommand::Stop, &shared);
        state.handle_command(
            EngineCommand::StartPercussionAudition {
                bpm: 112.0,
                beats_per_bar: 4,
                with_click: false,
            },
            &shared,
        );
        assert!(state.percussion_audition_playing);
        state.handle_command(EngineCommand::StopPercussionAudition, &shared);
        assert!(!state.percussion_audition_playing);
    }

    #[test]
    fn test_percussion_audition_follows_setlist_slot_and_bpm_changes() {
        let sample_rate = 48000;
        let song_a = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a, sample_rate, producer);
        state.setlist_items = vec![
            SetlistItem {
                song_id: "slot-0".into(),
                title: Some("Slow 6/8 Groove".into()),
                base_bpm: Some(72.0),
                slot_kind: SetlistSlotKind::PercussionGroove,
                time_signature: Some([6, 8]),
                ..Default::default()
            },
            SetlistItem {
                song_id: "slot-1".into(),
                title: Some("Fast Makossa".into()),
                base_bpm: Some(134.0),
                slot_kind: SetlistSlotKind::PercussionGroove,
                time_signature: Some([4, 4]),
                ..Default::default()
            },
        ];

        // Selecting setlist slot 0 updates audition BPM & meter to match setlist
        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        assert!((state.current_bpm - 72.0).abs() < 0.01);
        assert!((state.percussion_audition_bpm - 72.0).abs() < 0.01);
        assert_eq!(state.percussion_audition_bpb, 6);

        // Selecting a custom loop keeps it aligned to setlist BPM (72 -> 134 on NextSong)
        let custom = crate::percussion_patterns::CustomPercussionPattern {
            id: "my-loop".into(),
            name: "My Loop".into(),
            steps_per_bar: 16,
            bars_in_cycle: 2,
            swing: 0.0,
            is_compound: false,
            hits: vec![crate::percussion_patterns::CustomPatternHit {
                step: 0,
                instrument: crate::percussion_patterns::PercussionInstrument::Kick,
                velocity: 0.9,
                variant: crate::percussion_patterns::HitVariant::Standard,
                min_intensity: crate::percussion_patterns::GrooveIntensity::Low,
            }],
        };
        state.handle_command(EngineCommand::SetCustomPercussionPattern(Some(custom)), &shared);
        state.handle_command(
            EngineCommand::StartPercussionAudition {
                bpm: state.current_bpm,
                beats_per_bar: 6,
                with_click: false,
            },
            &shared,
        );
        assert!(state.percussion_audition_playing);

        // Adjusting BPM on live stage while auditioning dynamically tracks setlist BPM
        state.handle_command(EngineCommand::AdjustBpm(8.0), &shared);
        assert!((state.current_bpm - 80.0).abs() < 0.01);
        assert!((state.percussion_audition_bpm - 80.0).abs() < 0.01);

        // Advancing to slot 1 updates both stage BPM and audition BPM to 134 / 4/4
        state.handle_command(EngineCommand::NextSong, &shared);
        assert!((state.current_bpm - 134.0).abs() < 0.01);
        assert!((state.percussion_audition_bpm - 134.0).abs() < 0.01);
        assert_eq!(state.percussion_audition_bpb, 4);
    }

    #[test]
    fn test_percussion_only_aligns_with_metronome_click_when_tempo_or_key_shifted() {
        let sample_rate = 48000;
        let mut song = generate_demo_song(sample_rate);
        song.manifest.bpm = 120.0;
        song.manifest.first_downbeat_sec = 0.0;
        song.manifest.click_mode = ClickMode::AlwaysOn;

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);

        // Engage PercussionOnly + custom kick on step 0, and apply a key/tempo shift so `!bypassed`
        state.handle_command(
            EngineCommand::SetPercussionMode(crate::percussion_patterns::PercussionMode::PercussionOnly),
            &shared,
        );
        state.handle_command(EngineCommand::SetPercussionIemSend(0.0), &shared);
        state.handle_command(EngineCommand::SetKeyShift(2), &shared);
        state.handle_command(EngineCommand::SetBpm(130.0), &shared);
        state.handle_command(EngineCommand::Play, &shared);

        let mut click_onset = None;
        let mut perc_onset = None;
        for b in 0..25 {
            let block = state.render_block(512);
            for (i, f) in block.iter().enumerate() {
                let frame_idx = b * 512 + i;
                if click_onset.is_none() && f.iem_l.abs() > 0.01 {
                    click_onset = Some(frame_idx);
                }
                if perc_onset.is_none() && f.music_l.abs() > 0.01 {
                    perc_onset = Some(frame_idx);
                }
            }
        }
        let c = click_onset.expect("click onset observed");
        let p = perc_onset.expect("percussion onset observed");
        assert!(
            (c as isize - p as isize).abs() <= 4,
            "Click ({c}) and PercussionOnly beat 1 ({p}) must emerge within 4 samples even when stretched"
        );
    }

    #[test]
    fn test_batch_update_slots_applies_and_clears_custom_percussion_pattern() {
        let sample_rate = 48_000;
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let mut state = MixerState::new_for_test(generate_demo_song(sample_rate), sample_rate, producer);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));

        state.handle_command(
            EngineCommand::AddStandaloneSetlistSlot {
                title: "Praise 1".into(),
                slot_kind: crate::manifest::SetlistSlotKind::PercussionGroove,
                key: "G".into(),
                bpm: 112.0,
                time_signature: [4, 4],
                section_id: None,
                insert_at: None,
            },
            &shared,
        );
        state.handle_command(
            EngineCommand::AddStandaloneSetlistSlot {
                title: "Praise 2".into(),
                slot_kind: crate::manifest::SetlistSlotKind::PercussionGroove,
                key: "A".into(),
                bpm: 118.0,
                time_signature: [4, 4],
                section_id: None,
                insert_at: None,
            },
            &shared,
        );
        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);

        let alujo = CustomPercussionPattern {
            id: "alujo".into(),
            name: "Alujo".into(),
            steps_per_bar: 16,
            bars_in_cycle: 2,
            swing: 0.04,
            is_compound: false,
            hits: vec![crate::percussion_patterns::CustomPatternHit {
                step: 0,
                instrument: crate::percussion_patterns::PercussionInstrument::Kick,
                velocity: 0.9,
                variant: crate::percussion_patterns::HitVariant::Standard,
                min_intensity: crate::percussion_patterns::GrooveIntensity::Low,
            }],
        };

        state.handle_command(
            EngineCommand::BatchUpdateSetlistSlots(SetlistBatchPatch {
                slots: vec![0, 1],
                percussion_custom_pattern: Some(Some(alujo.clone())),
                ..Default::default()
            }),
            &shared,
        );

        assert_eq!(
            state.setlist_items[0]
                .percussion_lock
                .as_ref()
                .and_then(|l| l.custom_pattern.as_ref())
                .map(|p| p.id.as_str()),
            Some("alujo")
        );
        assert_eq!(
            state.setlist_items[1]
                .percussion_lock
                .as_ref()
                .and_then(|l| l.custom_pattern.as_ref())
                .map(|p| p.id.as_str()),
            Some("alujo")
        );
        assert_eq!(
            state.percussion.custom_pattern.as_ref().map(|p| p.id.as_str()),
            Some("alujo")
        );

        // Switching back to a built-in style with explicit null custom pattern clears it cleanly
        state.handle_command(
            EngineCommand::BatchUpdateSetlistSlots(SetlistBatchPatch {
                slots: vec![0, 1],
                percussion_style: Some(crate::percussion_patterns::PercussionStyle::Woro),
                percussion_custom_pattern: Some(None),
                ..Default::default()
            }),
            &shared,
        );
        assert_eq!(
            state.setlist_items[1].percussion_lock.as_ref().unwrap().style,
            crate::percussion_patterns::PercussionStyle::Woro
        );
        assert!(
            state.setlist_items[1]
                .percussion_lock
                .as_ref()
                .unwrap()
                .custom_pattern
                .is_none()
        );
        assert!(state.percussion.custom_pattern.is_none());
    }

    #[test]
    fn test_same_section_crossfade_keeps_beat_continuous_and_stops_at_section_end() {
        let sample_rate = 48_000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "praise-1".into();
        song_a.manifest.title = "Praise One".into();
        song_a.manifest.bpm = 120.0;
        song_a.manifest.first_downbeat_sec = 0.0;

        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "praise-2".into();
        song_b.manifest.title = "Praise Two".into();
        song_b.manifest.bpm = 120.0;
        // Simulate an imported song with a 0.5s intro pickup before Bar 1 Beat 1
        song_b.manifest.first_downbeat_sec = 0.5;

        let mut song_c = generate_demo_song(sample_rate);
        song_c.manifest.id = "worship-1".into();
        song_c.manifest.title = "Worship One".into();
        song_c.manifest.bpm = 72.0;

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/p1".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/p2".into(), CacheEntry::Ready(song_b));
        state.song_cache.insert("/songs/w1".into(), CacheEntry::Ready(song_c));

        state.setlist_items = vec![
            SetlistItem {
                song_id: "praise-1".into(),
                song_dir: Some("/songs/p1".into()),
                title: Some("Praise One".into()),
                original_key: Some("G".into()),
                base_bpm: Some(120.0),
                transition_mode: TransitionMode::AutoPause,
                crossfade_sec: 1.0,
                ..Default::default()
            },
            SetlistItem {
                song_id: "praise-2".into(),
                song_dir: Some("/songs/p2".into()),
                title: Some("Praise Two".into()),
                original_key: Some("G".into()),
                base_bpm: Some(120.0),
                transition_mode: TransitionMode::AutoPause,
                crossfade_sec: 1.0,
                ..Default::default()
            },
            SetlistItem {
                song_id: "worship-1".into(),
                song_dir: Some("/songs/w1".into()),
                title: Some("Worship One".into()),
                original_key: Some("D".into()),
                base_bpm: Some(72.0),
                transition_mode: TransitionMode::AutoPause,
                crossfade_sec: 1.0,
                ..Default::default()
            },
        ];

        // Assign Praise slots [0, 1] to `sec-praise` and Worship slot [2] to `sec-worship`.
        // Non-last song in `sec-praise` defaults to AutoAdvance; end-of-section stays AutoPause.
        state.handle_command(
            EngineCommand::BatchUpdateSetlistSlots(SetlistBatchPatch {
                slots: vec![0, 1],
                section_id: Some(Some("sec-praise".into())),
                percussion_mode: Some(crate::percussion_patterns::PercussionMode::PercussionOnly),
                percussion_style: Some(crate::percussion_patterns::PercussionStyle::Afrobeat),
                ..Default::default()
            }),
            &shared,
        );
        state.handle_command(
            EngineCommand::UpdateSetlistSlotConfig {
                slot: 2,
                key_shift_semitones: None,
                bpm_override: None,
                transition_mode: None,
                crossfade_sec: None,
                ambient_pad_enabled: None,
                pad_key_override: None,
                pad_lock: None,
                click_mode: None,
                section_id: Some(Some("sec-worship".into())),
                percussion_lock: None,
            },
            &shared,
        );
        assert_eq!(state.setlist_items[0].transition_mode, TransitionMode::AutoAdvance);
        assert_eq!(state.setlist_items[1].transition_mode, TransitionMode::AutoPause);

        // Start Praise One playing and advance 1 block so percussion hits are active
        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        state.handle_command(EngineCommand::Play, &shared);
        let _ = state.render_block(2048);

        // Crossfade/Next within `sec-praise` into Praise Two (which has a 0.5s pre-downbeat pickup)
        state.handle_command(EngineCommand::NextSong, &shared);
        assert_eq!(state.active_setlist_slot, Some(1));
        assert!(state.is_playing);
        assert_eq!(
            state.percussion.mode,
            crate::percussion_patterns::PercussionMode::PercussionOnly
        );
        assert!(
            state.percussion_bridge_beat.is_some(),
            "Same-section crossfade must maintain continuous beat clock across song handoff"
        );

        // Render blocks during Song B's 0.5s pre-downbeat: beat steps & percussion audio must NOT stop!
        let mut perc_audio_energy = 0.0_f32;
        for _ in 0..20 {
            let block = state.render_block(1024);
            for f in block {
                perc_audio_energy += f.music_l.abs();
            }
        }
        assert!(
            perc_audio_energy > 1.0,
            "Percussion groove must stay audible without stopping during same-section transition"
        );

        // Crossing out of `sec-praise` (end of section) into unlocked `sec-worship` stops the style by default
        state.handle_command(EngineCommand::NextSong, &shared);
        assert_eq!(state.active_setlist_slot, Some(2));
        assert_eq!(
            state.percussion.mode,
            crate::percussion_patterns::PercussionMode::Off,
            "Leaving a section into an unlocked slot must stop the section's percussion style by default"
        );
    }

    #[test]
    fn test_same_section_crossfade_maintains_continuous_metronome_count() {
        let sample_rate = 48_000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "m-praise-1".into();
        song_a.manifest.bpm = 120.0; // 24,000 frames per beat
        song_a.manifest.time_signature = [4, 4];
        song_a.manifest.first_downbeat_sec = 0.0;
        song_a.manifest.click_mode = ClickMode::AlwaysOn;

        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "m-praise-2".into();
        song_b.manifest.bpm = 120.0;
        song_b.manifest.time_signature = [4, 4];
        song_b.manifest.first_downbeat_sec = 0.5;
        song_b.manifest.click_mode = ClickMode::AlwaysOn;

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let snap = Arc::new(RwLock::new(build_snapshot(
            &song_a,
            "test",
            sample_rate,
            2,
            OutputRoutingMode::SplitLeftRight,
        )));
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/mp1".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/mp2".into(), CacheEntry::Ready(song_b));

        state.setlist_items = vec![
            SetlistItem {
                song_id: "m-praise-1".into(),
                song_dir: Some("/songs/mp1".into()),
                title: Some("Praise 1".into()),
                base_bpm: Some(120.0),
                transition_mode: TransitionMode::AutoAdvance,
                crossfade_sec: 1.0,
                section_id: Some("sec-praise".into()),
                ..Default::default()
            },
            SetlistItem {
                song_id: "m-praise-2".into(),
                song_dir: Some("/songs/mp2".into()),
                title: Some("Praise 2".into()),
                base_bpm: Some(120.0),
                transition_mode: TransitionMode::AutoPause,
                crossfade_sec: 1.0,
                section_id: Some("sec-praise".into()),
                ..Default::default()
            },
        ];

        // Also test PercussionOnly (continuous loop across same section)
        state.handle_command(
            EngineCommand::BatchUpdateSetlistSlots(SetlistBatchPatch {
                slots: vec![0, 1],
                percussion_mode: Some(crate::percussion_patterns::PercussionMode::PercussionOnly),
                percussion_style: Some(crate::percussion_patterns::PercussionStyle::Afrobeat),
                ..Default::default()
            }),
            &shared,
        );
        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        state.handle_command(EngineCommand::Play, &shared);

        // Advance 1.5 beats = 36,000 frames -> Beat 2 of Bar 1 (0-based beat index 1)
        let _ = state.render_block(24_000);
        let _ = state.render_block(12_000);
        assert_eq!(state.last_beat_index, Some(1));
        state.publish_snapshot(&snap, &shared);
        {
            let s = snap.read().unwrap();
            assert_eq!((s.current_bar, s.current_beat), (1, 2));
        }

        // Crossfade mid-beat into Song B (0.5s pickup): count MUST stay at Bar 1 Beat 2
        // without resetting to Beat 1 or firing a spurious beat-0 click
        state.handle_command(EngineCommand::NextSong, &shared);
        assert_eq!(state.active_setlist_slot, Some(1));
        assert_eq!(state.last_beat_index, Some(1));
        state.publish_snapshot(&snap, &shared);
        {
            let s = snap.read().unwrap();
            assert_eq!(
                (s.current_bar, s.current_beat),
                (1, 2),
                "Bar/Beat display must stay on 1:2 across same-section crossfade"
            );
        }

        // First 6,000 frames (0.25 beats -> beat 1.75): still on beat 1, click must NOT retrigger early
        let _ = state.render_block(6_000);
        assert_eq!(state.last_beat_index, Some(1));
        assert!(
            !state.click_synth.is_active(),
            "Metronome click must not double-trigger mid-beat during transition"
        );

        // Next 6,016 frames steps onto beat 2.0 -> triggers Beat 3 cleanly on time
        let _ = state.render_block(6_016);
        assert_eq!(state.last_beat_index, Some(2));
        assert!(
            state.click_synth.is_active(),
            "Metronome click must trigger cleanly when beat 3 arrives"
        );
        state.publish_snapshot(&snap, &shared);
        {
            let s = snap.read().unwrap();
            assert_eq!((s.current_bar, s.current_beat), (1, 3));
        }

        // Advance 2 more beats (48,000 frames -> beat 4.0) -> Beat 4 then Bar 2 Beat 1
        let _ = state.render_block(24_000);
        assert_eq!(state.last_beat_index, Some(3));
        state.publish_snapshot(&snap, &shared);
        {
            let s = snap.read().unwrap();
            assert_eq!((s.current_bar, s.current_beat), (1, 4));
        }

        let _ = state.render_block(24_000);
        assert_eq!(state.last_beat_index, Some(4));
        state.publish_snapshot(&snap, &shared);
        {
            let s = snap.read().unwrap();
            assert_eq!((s.current_bar, s.current_beat), (2, 1));
        }
    }

    #[test]
    fn test_stop_playback_stops_drone_pad() {
        let sample_rate = 48_000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, sample_rate, producer);

        state.handle_command(EngineCommand::SetPadEnabled(true), &shared);
        state.handle_command(EngineCommand::Play, &shared);
        let _ = state.render_block(512);
        assert!(state.is_playing);
        assert!(state.pad.enabled);

        state.handle_command(EngineCommand::Stop, &shared);
        assert!(!state.is_playing);
        assert!(
            !state.pad.enabled,
            "Stopping playback must also turn off the ambient drone pad"
        );

        // Also test PlayStop toggle when playing
        state.handle_command(EngineCommand::SetPadEnabled(true), &shared);
        state.handle_command(EngineCommand::Play, &shared);
        assert!(state.pad.enabled);
        state.handle_command(EngineCommand::PlayStop, &shared);
        assert!(!state.is_playing);
        assert!(!state.pad.enabled);
    }

    #[test]
    fn test_setlist_load_and_slot_switch_preserve_stem_and_iem_mixer() {
        let sample_rate = 48_000;
        let mut song_a = generate_demo_song(sample_rate);
        song_a.manifest.id = "song-a".into();
        let mut song_b = generate_demo_song(sample_rate);
        song_b.manifest.id = "song-b".into();

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a.clone(), sample_rate, producer);
        state.song_cache.insert("/songs/a".into(), CacheEntry::Ready(song_a));
        state.song_cache.insert("/songs/b".into(), CacheEntry::Ready(song_b));

        let setlist = SetlistManifest {
            id: "mixer-set".into(),
            name: "Mixer Test Set".into(),
            sections: vec![],
            items: vec![
                SetlistItem {
                    song_id: "song-a".into(),
                    song_dir: Some("/songs/a".into()),
                    base_click_mode: Some(ClickMode::AlwaysOn),
                    stem_mix: vec![
                        StemMixOverride {
                            name: "drums".into(),
                            gain: 0.42,
                            muted: true,
                        },
                        StemMixOverride {
                            name: "music".into(),
                            gain: 1.15,
                            muted: false,
                        },
                    ],
                    ..Default::default()
                },
                SetlistItem {
                    song_id: "song-b".into(),
                    song_dir: Some("/songs/b".into()),
                    base_click_mode: Some(ClickMode::AlwaysOn),
                    ..Default::default()
                },
            ],
            mixer: Some(crate::manifest::SetlistMixerConfig {
                master_gain: 0.72,
                backing_gain: 0.65,
                click_gain: 0.58,
                click_muted: Some(true),
            }),
        };

        state.handle_command(EngineCommand::LoadSetlist(setlist), &shared);
        assert!((shared.read().unwrap().master_gain - 0.72).abs() < 1e-4);
        assert!((state.backing_gain - 0.65).abs() < 1e-4);
        assert!((state.click_gain - 0.58).abs() < 1e-4);
        assert!(state.click_muted);

        let drums = state.song.stems.iter().find(|s| s.name == "drums").unwrap();
        assert!((drums.gain - 0.42).abs() < 1e-4);
        assert!(drums.muted);
        let music = state.song.stems.iter().find(|s| s.name == "music").unwrap();
        assert!((music.gain - 1.15).abs() < 1e-4);
        assert!(!music.muted);

        // Live edits to slot 1 stems must persist across slot switches
        state.handle_command(EngineCommand::SelectSetlistSlot(1), &shared);
        state.handle_command(
            EngineCommand::SetStemGain {
                name: "bass".into(),
                gain: 0.65,
            },
            &shared,
        );
        state.handle_command(
            EngineCommand::SetStemMuted {
                name: "music".into(),
                muted: true,
            },
            &shared,
        );
        assert!(
            state.setlist_items[1]
                .stem_mix
                .iter()
                .any(|s| s.name == "bass" && (s.gain - 0.65).abs() < 1e-4)
        );
        assert!(
            state.setlist_items[1]
                .stem_mix
                .iter()
                .any(|s| s.name == "music" && s.muted)
        );

        // Switch back to slot 0 and then return to slot 1 -> slot 1 mix remains intact
        state.handle_command(EngineCommand::SelectSetlistSlot(0), &shared);
        assert!(state.song.stems.iter().find(|s| s.name == "drums").unwrap().muted);
        state.handle_command(EngineCommand::SelectSetlistSlot(1), &shared);
        let bass_b = state.song.stems.iter().find(|s| s.name == "bass").unwrap();
        assert!((bass_b.gain - 0.65).abs() < 1e-4);
        assert!(state.song.stems.iter().find(|s| s.name == "music").unwrap().muted);
    }

    #[test]
    fn test_continue_until_stopped_slot_percussion_keeps_playing_past_song_end() {
        let mut song_a = generate_demo_song(44_100);
        song_a.manifest.id = "song-a".into();
        song_a.manifest.title = "Song A Groove".into();
        // Shorten to 1 bar at 120 BPM (88,200 frames) so we cross EOF rapidly in the test.
        song_a.manifest.bpm = 120.0;
        song_a.total_frames = 88_200;
        for stem in &mut song_a.stems {
            Arc::make_mut(&mut stem.samples).truncate(88_200 * 2);
        }

        let mut song_b = generate_demo_song(44_100);
        song_b.manifest.id = "song-b".into();
        song_b.manifest.title = "Song B".into();

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song_a.clone(), 44_100, producer);
        state
            .song_cache
            .insert("/songs/a".into(), CacheEntry::Ready(song_a));
        state
            .song_cache
            .insert("/songs/b".into(), CacheEntry::Ready(song_b));

        let setlist = SetlistManifest {
            id: "continue-perc-test".into(),
            name: "Continue Percussion Test".into(),
            sections: vec![crate::manifest::SetlistSection {
                id: "sec-praise".into(),
                name: "Praise".into(),
                kind: crate::manifest::ServiceSectionKind::Praise,
                color: None,
                collapsed: false,
                note: None,
            }],
            items: vec![
                SetlistItem {
                    song_id: "song-a".into(),
                    song_dir: Some("/songs/a".into()),
                    section_id: Some("sec-praise".into()),
                    transition_mode: TransitionMode::ContinueUntilStopped,
                    percussion_lock: Some(PercussionLockConfig {
                        mode: crate::percussion_patterns::PercussionMode::PercussionOnly,
                        style: crate::percussion_patterns::PercussionStyle::default(),
                        intensity: crate::percussion_patterns::GrooveIntensity::Medium,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                SetlistItem {
                    song_id: "song-b".into(),
                    song_dir: Some("/songs/b".into()),
                    section_id: Some("sec-praise".into()),
                    transition_mode: TransitionMode::AutoPause,
                    ..Default::default()
                },
            ],
            mixer: None,
        };

        state.handle_command(EngineCommand::LoadSetlist(setlist), &shared);
        assert_eq!(
            state.setlist_items[0].transition_mode,
            TransitionMode::ContinueUntilStopped
        );
        state.handle_command(EngineCommand::Play, &shared);

        // Render past 2.5 seconds (> 2.0s song length) -> stays on slot 0, still playing, continuous clock!
        let mut max_after_end = 0.0_f32;
        for step in 0..115 {
            let block = state.render_block(1024);
            if step >= 88 {
                for f in &block {
                    max_after_end = max_after_end.max(f.music_l.abs().max(f.music_r.abs()));
                }
            }
        }
        assert!(state.is_playing, "Slot 0 must keep playing past song end");
        assert_eq!(state.active_setlist_slot, Some(0), "Must stay on Song 1 until stopped/advanced");
        assert!(state.percussion_bridge_continuous);
        assert!(
            max_after_end > 0.01,
            "Percussion groove must keep producing audio after song end (got {max_after_end})"
        );

        state.handle_command(EngineCommand::Stop, &shared);
        assert!(!state.is_playing);
        assert!(!state.percussion_bridge_continuous);
    }

    #[test]
    fn test_toggle_continue_track_keeps_backing_stems_playing_until_stop() {
        let mut song = generate_demo_song(44_100);
        song.manifest.bpm = 120.0;
        song.total_frames = 44_100;
        for stem in &mut song.stems {
            Arc::make_mut(&mut stem.samples).truncate(44_100 * 2);
        }

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, 44_100, producer);
        state.current_bpm = 120.0;
        state.handle_command(EngineCommand::ToggleContinueTrack, &shared);
        assert!(state.continue_until_stopped);
        state.handle_command(EngineCommand::Play, &shared);

        for _ in 0..60 {
            let _ = state.render_block(1024);
        }
        assert!(state.is_playing, "Backing track must keep looping while Continue Track is active");
        assert!(state.source_frame_pos < 44_100.0);

        state.handle_command(EngineCommand::Stop, &shared);
        assert!(!state.is_playing);
    }

    #[test]
    fn test_independent_backing_gain_and_click_gain() {
        let mut song = generate_demo_song(48_000);
        song.manifest.bpm = 120.0;
        song.manifest.click_mode = ClickMode::AlwaysOn;

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, 48_000, producer);
        state.handle_command(EngineCommand::Play, &shared);

        // Baseline render with backing=1.0, click=1.0
        state.handle_command(EngineCommand::SetBackingGain(1.0), &shared);
        state.handle_command(EngineCommand::SetClickGain(1.0), &shared);
        let base_block = state.render_block(512);
        let base_music_peak = base_block
            .iter()
            .map(|f| f.music_l.abs().max(f.music_r.abs()))
            .fold(0.0_f32, f32::max);
        let base_iem_peak = base_block
            .iter()
            .map(|f| f.iem_l.abs().max(f.iem_r.abs()))
            .fold(0.0_f32, f32::max);
        assert!(base_music_peak > 0.01);
        assert!(base_iem_peak > 0.01);

        // Zeroing backing_gain silences backing stems while leaving click in IEM untouched
        state.handle_command(EngineCommand::SeekSec(0.0), &shared);
        state.handle_command(EngineCommand::SetBackingGain(0.0), &shared);
        state.handle_command(EngineCommand::SetClickGain(1.0), &shared);
        let zero_backing_block = state.render_block(512);
        let zero_music_peak = zero_backing_block
            .iter()
            .map(|f| f.music_l.abs().max(f.music_r.abs()))
            .fold(0.0_f32, f32::max);
        let click_still_peak = zero_backing_block
            .iter()
            .map(|f| f.iem_l.abs().max(f.iem_r.abs()))
            .fold(0.0_f32, f32::max);
        assert!(zero_music_peak < 1e-5, "Backing gain 0.0 must silence backing stems");
        assert!(click_still_peak > 0.01, "Click in IEM must remain audible when backing gain is 0.0");

        // Zeroing click_gain silences click while backing track & pad remain untouched
        state.handle_command(EngineCommand::SeekSec(0.0), &shared);
        state.handle_command(EngineCommand::SetBackingGain(0.5), &shared);
        state.handle_command(EngineCommand::SetClickGain(0.0), &shared);
        state.handle_command(EngineCommand::SetPadEnabled(true), &shared);
        state.handle_command(EngineCommand::SetPadGain(0.6), &shared);
        let half_backing_block = state.render_block(512);
        let half_music_peak = half_backing_block
            .iter()
            .map(|f| f.music_l.abs().max(f.music_r.abs()))
            .fold(0.0_f32, f32::max);
        let click_zero_peak = half_backing_block
            .iter()
            .map(|f| f.iem_l.abs().max(f.iem_r.abs()))
            .fold(0.0_f32, f32::max);
        assert!(half_music_peak > 0.01, "Pad + 50% backing must remain audible when click is 0.0");
        assert!(click_zero_peak < 1e-5, "Click gain 0.0 must silence click without altering backing/pad");
    }

    #[test]
    fn test_cues_stem_routes_only_to_iem_not_foh() {
        let mut song = generate_demo_song(48_000);
        // Zero out all backing stems and add a non-silent is_cues stem
        for stem in &mut song.stems {
            stem.gain = 0.0;
            stem.muted = true;
        }
        let samples = vec![0.45_f32; 48_000 * 2];
        song.stems.push(crate::loader::DecodedStem {
            name: "cues".into(),
            pitched: false,
            is_cues: true,
            gain: 0.9,
            muted: false,
            samples: Arc::new(samples),
        });

        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);
        let shared = Arc::new(RwLock::new(SharedRouting {
            mode: OutputRoutingMode::SplitLeftRight,
            master_gain: 1.0,
        }));
        let mut state = MixerState::new_for_test(song, 48_000, producer);
        state.handle_command(EngineCommand::SetClickMuted(true), &shared);
        state.handle_command(EngineCommand::Play, &shared);

        let block = state.render_block(512);
        let foh_peak = block
            .iter()
            .map(|f| f.music_l.abs().max(f.music_r.abs()))
            .fold(0.0_f32, f32::max);
        let iem_peak = block
            .iter()
            .map(|f| f.iem_l.abs().max(f.iem_r.abs()))
            .fold(0.0_f32, f32::max);

        assert!(foh_peak < 1e-6, "IEM cues stem must never bleed into FOH (peak={foh_peak})");
        assert!(iem_peak > 0.25, "IEM cues stem must be audible in IEM bus (peak={iem_peak})");
    }
}
