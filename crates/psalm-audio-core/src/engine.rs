use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, RwLock};
use std::thread;
use std::time::Duration;

use cpal::traits::{DeviceTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use serde::{Deserialize, Serialize};

use crate::click::{ClickDelayBuffer, ClickSynth};
use crate::loader::{generate_demo_song, load_song_from_dir, LoadedSong};
use crate::routing::{find_output_device, OutputRoutingMode};
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
    pub color: Option<String>,
}

/// Lightweight snapshot emitted to the UI at ~30fps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSnapshot {
    pub song_id: Option<String>,
    pub song_title: String,
    pub original_key: String,
    pub current_key: String,
    pub is_playing: bool,
    pub current_bar: u32,
    pub current_beat: u32,
    pub beats_per_bar: u32,
    pub active_section_id: Option<String>,
    pub armed_section_id: Option<String>,
    pub loop_enabled: bool,
    pub quantize_mode: QuantizeMode,
    pub key_shift_semitones: i32,
    pub base_bpm: f64,
    pub current_bpm: f64,
    pub master_gain: f32,
    pub click_gain: f32,
    pub click_muted: bool,
    pub routing_mode: OutputRoutingMode,
    pub device_name: String,
    pub device_sample_rate: u32,
    pub device_channels: u16,
    pub sections: Vec<SectionSnapshot>,
    pub stems: Vec<StemSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum EngineCommand {
    LoadDemoSong,
    LoadSongDir(String),
    Play,
    Stop,
    PlayStop,
    QueueSectionById(String),
    QueueSectionBySlot(usize),
    QueueSectionByName(String),
    NextSection,
    PrevSection,
    ToggleLoop,
    SetLoop(bool),
    SetQuantizeMode(QuantizeMode),
    SetKeyShift(i32),
    AdjustKeyShift(i32),
    SetBpm(f64),
    AdjustBpm(f64),
    SetMasterGain(f32),
    SetClickGain(f32),
    SetClickMuted(bool),
    SetStemGain { name: String, gain: f32 },
    SetStemMuted { name: String, muted: bool },
    SetRoutingMode(OutputRoutingMode),
    SelectOutputDevice(String),
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

                let mut state = MixerState::new(
                    initial_song,
                    dev_name,
                    sample_rate,
                    channels,
                    stream,
                    producer,
                );

                while !shutdown_worker.load(Ordering::Relaxed) {
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        state.handle_command(cmd, &shared_routing_worker);
                    }

                    let mut pushed = false;
                    if state.is_playing {
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

fn open_audio_stream(
    preferred_device: Option<&str>,
    shared_routing: Arc<RwLock<SharedRouting>>,
) -> Result<(String, u32, u16, cpal::Stream, Producer<BusFrame>)> {
    let device = find_output_device(preferred_device)?;
    let device_name = device.name().unwrap_or_else(|_| "Unknown Device".into());
    let config = device
        .default_output_config()
        .map_err(|e| AudioCoreError::Device(e.to_string()))?;

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
                    let music_mono = (frame.music_l + frame.music_r) * 0.5 * master;
                    let iem_mono = (frame.iem_l + frame.iem_r) * 0.5;

                    match mode {
                        OutputRoutingMode::SplitLeftRight => {
                            if out_channels >= 2 {
                                out_frame[0] = music_mono.clamp(-1.0, 1.0);
                                out_frame[1] = iem_mono.clamp(-1.0, 1.0);
                            } else if out_channels == 1 {
                                out_frame[0] = music_mono.clamp(-1.0, 1.0);
                            }
                            for s in out_frame.iter_mut().skip(2) {
                                *s = 0.0;
                            }
                        }
                        OutputRoutingMode::SplitClickLeftMusicRight => {
                            if out_channels >= 2 {
                                out_frame[0] = iem_mono.clamp(-1.0, 1.0);
                                out_frame[1] = music_mono.clamp(-1.0, 1.0);
                            } else if out_channels == 1 {
                                out_frame[0] = music_mono.clamp(-1.0, 1.0);
                            }
                            for s in out_frame.iter_mut().skip(2) {
                                *s = 0.0;
                            }
                        }
                        OutputRoutingMode::MultiOutStereo => {
                            if out_channels >= 4 {
                                out_frame[0] = (frame.music_l * master).clamp(-1.0, 1.0);
                                out_frame[1] = (frame.music_r * master).clamp(-1.0, 1.0);
                                out_frame[2] = frame.iem_l.clamp(-1.0, 1.0);
                                out_frame[3] = frame.iem_r.clamp(-1.0, 1.0);
                                for s in out_frame.iter_mut().skip(4) {
                                    *s = 0.0;
                                }
                            } else if out_channels >= 2 {
                                out_frame[0] = music_mono.clamp(-1.0, 1.0);
                                out_frame[1] = iem_mono.clamp(-1.0, 1.0);
                            }
                        }
                    }
                }
            },
            |err| log::error!("cpal output stream error: {err}"),
            None,
        )
        .map_err(|e| AudioCoreError::Stream(e.to_string()))?;

    stream
        .play()
        .map_err(|e| AudioCoreError::Stream(e.to_string()))?;

    Ok((device_name, sample_rate, out_channels as u16, stream, producer))
}

struct MixerState {
    song: LoadedSong,
    sample_rate: u32,
    is_playing: bool,
    source_frame_pos: f64,
    last_beat_index: Option<i64>,
    active_section_idx: Option<usize>,
    armed_section_idx: Option<usize>,
    loop_enabled: bool,
    quantize_mode: QuantizeMode,
    key_shift_semitones: i32,
    current_bpm: f64,
    click_gain: f32,
    click_muted: bool,
    device_name: String,
    device_channels: u16,

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
}

impl MixerState {
    fn new(
        song: LoadedSong,
        device_name: String,
        sample_rate: u32,
        device_channels: u16,
        stream: cpal::Stream,
        producer: Producer<BusFrame>,
    ) -> Self {
        let base_bpm = song.manifest.bpm;
        Self {
            song,
            sample_rate,
            is_playing: false,
            source_frame_pos: 0.0,
            last_beat_index: None,
            active_section_idx: Some(0),
            armed_section_idx: None,
            loop_enabled: false,
            quantize_mode: QuantizeMode::NextBar,
            key_shift_semitones: 0,
            current_bpm: base_bpm,
            click_gain: 0.85,
            click_muted: false,
            device_name,
            device_channels,
            _stream: Some(stream),
            producer,
            click_synth: ClickSynth::new(sample_rate),
            click_delay: ClickDelayBuffer::new(16384),
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
        }
    }

    fn handle_command(&mut self, cmd: EngineCommand, shared_routing: &Arc<RwLock<SharedRouting>>) {
        match cmd {
            EngineCommand::LoadDemoSong => {
                self.load_song(generate_demo_song(self.sample_rate));
            }
            EngineCommand::LoadSongDir(path) => {
                match load_song_from_dir(std::path::Path::new(&path), self.sample_rate) {
                    Ok(song) => self.load_song(song),
                    Err(e) => log::error!("Failed to load song at {path}: {e}"),
                }
            }
            EngineCommand::Play => {
                if !self.is_playing {
                    self.is_playing = true;
                    self.last_beat_index = None;
                    self.click_delay.clear();
                    self.prime_stretchers();
                }
            }
            EngineCommand::Stop => {
                self.is_playing = false;
                self.source_frame_pos = 0.0;
                self.last_beat_index = None;
                self.click_delay.clear();
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
            EngineCommand::SetQuantizeMode(q) => {
                self.quantize_mode = q;
            }
            EngineCommand::SetKeyShift(semi) => {
                self.key_shift_semitones = semi.clamp(-12, 12);
                self.pitched_stretcher
                    .set_semitones(self.key_shift_semitones as f32);
            }
            EngineCommand::AdjustKeyShift(delta) => {
                let next = (self.key_shift_semitones + delta).clamp(-12, 12);
                self.key_shift_semitones = next;
                self.pitched_stretcher.set_semitones(next as f32);
            }
            EngineCommand::SetBpm(bpm) => {
                self.current_bpm = bpm.clamp(30.0, 260.0);
            }
            EngineCommand::AdjustBpm(delta) => {
                self.current_bpm = (self.current_bpm + delta).clamp(30.0, 260.0);
            }
            EngineCommand::SetMasterGain(gain) => {
                if let Ok(mut r) = shared_routing.write() {
                    r.master_gain = gain.clamp(0.0, 1.5);
                }
            }
            EngineCommand::SetClickGain(gain) => {
                self.click_gain = gain.clamp(0.0, 1.5);
            }
            EngineCommand::SetClickMuted(muted) => {
                self.click_muted = muted;
            }
            EngineCommand::SetStemGain { name, gain } => {
                if let Some(stem) = self.song.stems.iter_mut().find(|s| s.name == name) {
                    stem.gain = gain.clamp(0.0, 1.5);
                }
            }
            EngineCommand::SetStemMuted { name, muted } => {
                if let Some(stem) = self.song.stems.iter_mut().find(|s| s.name == name) {
                    stem.muted = muted;
                }
            }
            EngineCommand::SetRoutingMode(mode) => {
                if let Ok(mut r) = shared_routing.write() {
                    r.mode = mode;
                }
            }
            EngineCommand::SelectOutputDevice(name) => {
                if let Ok((dev_name, sr, ch, stream, prod)) =
                    open_audio_stream(Some(&name), Arc::clone(shared_routing))
                {
                    self._stream = Some(stream);
                    self.producer = prod;
                    self.device_name = dev_name;
                    self.device_channels = ch;
                    if sr != self.sample_rate {
                        self.sample_rate = sr;
                        self.crossfade_total = (sr as usize / 80).max(64);
                        self.click_synth.set_sample_rate(sr);
                        self.pitched_stretcher.set_sample_rate(sr);
                        self.drums_stretcher.set_sample_rate(sr);
                        self.cues_stretcher.set_sample_rate(sr);
                    }
                }
            }
        }
    }

    fn load_song(&mut self, song: LoadedSong) {
        self.is_playing = false;
        self.current_bpm = song.manifest.bpm;
        self.key_shift_semitones = 0;
        self.pitched_stretcher.set_semitones(0.0);
        self.source_frame_pos = 0.0;
        self.last_beat_index = None;
        self.click_delay.clear();
        self.armed_section_idx = None;
        self.active_section_idx = if song.manifest.sections.is_empty() {
            None
        } else {
            Some(0)
        };
        self.song = song;
    }

    fn arm_or_jump_section(&mut self, idx: usize) {
        if self.is_playing {
            self.armed_section_idx = Some(idx);
        } else {
            let start_sec = self
                .song
                .manifest
                .bar_to_seconds(self.song.manifest.sections[idx].start_bar);
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
        let bpb = self.song.manifest.beats_per_bar() as i64;
        let spb_src = self.song.manifest.seconds_per_beat() * self.sample_rate as f64;
        let downbeat_src = self.song.manifest.first_downbeat_sec * self.sample_rate as f64;
        let bypassed = self.is_stretch_bypassed();

        let mut block = Vec::with_capacity(out_frames);

        if bypassed {
            let start_frame = self.source_frame_pos.round() as usize;
            self.check_quantized_transitions(start_frame, out_frames);

            let cur_start = self.source_frame_pos.round() as usize;
            self.fill_stem_group_slice(cur_start, out_frames);

            for i in 0..out_frames {
                let frame_src_pos = (cur_start + i) as f64;
                if frame_src_pos >= downbeat_src {
                    let beat_idx = ((frame_src_pos - downbeat_src) / spb_src).floor() as i64;
                    if self.last_beat_index != Some(beat_idx) {
                        self.last_beat_index = Some(beat_idx);
                        let is_downbeat = (beat_idx % bpb) == 0;
                        self.click_synth.trigger(is_downbeat);
                    }
                }

                let click_sample = if self.click_muted {
                    0.0
                } else {
                    self.click_synth.next_sample() * self.click_gain
                };

                let mut frame = BusFrame {
                    music_l: self.pitched_in[i * 2] + self.drums_in[i * 2],
                    music_r: self.pitched_in[i * 2 + 1] + self.drums_in[i * 2 + 1],
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

                block.push(frame);
            }

            self.source_frame_pos = (cur_start + out_frames) as f64;
        } else {
            let rate = self.tempo_ratio();
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

            let latency = self.pitched_stretcher.total_latency_frames();

            for i in 0..out_frames {
                let frame_src_pos = self.source_frame_pos + (i as f64) * rate;
                if frame_src_pos >= downbeat_src {
                    let beat_idx = ((frame_src_pos - downbeat_src) / spb_src).floor() as i64;
                    if self.last_beat_index != Some(beat_idx) {
                        self.last_beat_index = Some(beat_idx);
                        let is_downbeat = (beat_idx % bpb) == 0;
                        self.click_synth.trigger(is_downbeat);
                    }
                }

                let raw_click = if self.click_muted {
                    0.0
                } else {
                    self.click_synth.next_sample() * self.click_gain
                };

                let click_sample = self.click_delay.process_sample(raw_click, latency);

                let mut frame = BusFrame {
                    music_l: self.pitched_out[i * 2] + self.drums_out[i * 2],
                    music_r: self.pitched_out[i * 2 + 1] + self.drums_out[i * 2 + 1],
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

                block.push(frame);
            }

            self.source_frame_pos += (out_frames as f64) * rate;
        }

        if self.source_frame_pos as usize >= self.song.total_frames {
            if self.loop_enabled {
                self.source_frame_pos = 0.0;
                self.last_beat_index = None;
                self.click_delay.clear();
                self.prime_stretchers();
            } else {
                self.is_playing = false;
                self.source_frame_pos = 0.0;
                self.click_delay.clear();
            }
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
            .position(|s| bar_now >= s.start_bar && bar_now < s.end_bar)
        {
            self.active_section_idx = Some(idx);
        }

        if bar_next > bar_now {
            if let Some(target_idx) = self.armed_section_idx {
                let should_jump = match self.quantize_mode {
                    QuantizeMode::NextBar => true,
                    QuantizeMode::EndOfSection => self
                        .active_section_idx
                        .and_then(|cur| self.song.manifest.sections.get(cur))
                        .map(|s| bar_next >= s.end_bar)
                        .unwrap_or(true),
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
                        if bar_next >= sec.end_bar {
                            self.jump_to_section_now(cur_idx);
                        }
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
            .map(|sec| self.song.manifest.bar_to_seconds(sec.start_bar))
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
        self.last_beat_index = None;
        self.click_delay.clear();
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
            let latency = self.pitched_stretcher.total_latency_frames() as f64;
            let rate = self.tempo_ratio();
            ((self.source_frame_pos - latency * rate).max(0.0)) / self.sample_rate as f64
        };
        let (bar, beat, _) = self.song.manifest.seconds_to_bar_beat(cur_sec);
        let (routing_mode, master_gain) = shared_routing
            .read()
            .map(|r| (r.mode, r.master_gain))
            .unwrap_or((OutputRoutingMode::SplitLeftRight, 1.0));

        if let Ok(mut snap) = snapshot.write() {
            snap.song_id = Some(self.song.manifest.id.clone());
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
            snap.active_section_id = self
                .active_section_idx
                .and_then(|i| self.song.manifest.sections.get(i))
                .map(|s| s.id.clone());
            snap.armed_section_id = self
                .armed_section_idx
                .and_then(|i| self.song.manifest.sections.get(i))
                .map(|s| s.id.clone());
            snap.loop_enabled = self.loop_enabled;
            snap.quantize_mode = self.quantize_mode;
            snap.key_shift_semitones = self.key_shift_semitones;
            snap.base_bpm = self.song.manifest.bpm;
            snap.current_bpm = self.current_bpm;
            snap.master_gain = master_gain;
            snap.click_gain = self.click_gain;
            snap.click_muted = self.click_muted;
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
        song_title: song.manifest.title.clone(),
        original_key: song.manifest.original_key.clone(),
        current_key: song.manifest.original_key.clone(),
        is_playing: false,
        current_bar: 1,
        current_beat: 1,
        beats_per_bar: song.manifest.beats_per_bar(),
        active_section_id: song.manifest.sections.first().map(|s| s.id.clone()),
        armed_section_id: None,
        loop_enabled: false,
        quantize_mode: QuantizeMode::NextBar,
        key_shift_semitones: 0,
        base_bpm: song.manifest.bpm,
        current_bpm: song.manifest.bpm,
        master_gain: 1.0,
        click_gain: 0.85,
        click_muted: false,
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
    let normalized = match base.trim() {
        "Db" => "C#",
        "D#" => "Eb",
        "Gb" => "F#",
        "G#" => "Ab",
        "A#" => "Bb",
        other => other,
    };
    let Some(pos) = KEYS.iter().position(|&k| k.eq_ignore_ascii_case(normalized)) else {
        return if semitones == 0 {
            base.to_string()
        } else {
            format!("{base} ({semitones:+})")
        };
    };
    let idx = ((pos as i32 + semitones).rem_euclid(12)) as usize;
    KEYS[idx].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_bypass_playback() {
        let sample_rate = 48000;
        let song = generate_demo_song(sample_rate);
        let (producer, _consumer) = RingBuffer::<BusFrame>::new(4096);

        let mut state = MixerState {
            song,
            sample_rate,
            is_playing: true,
            source_frame_pos: 0.0,
            last_beat_index: None,
            active_section_idx: Some(0),
            armed_section_idx: None,
            loop_enabled: false,
            quantize_mode: QuantizeMode::NextBar,
            key_shift_semitones: 0,
            current_bpm: 72.0,
            click_gain: 0.85,
            click_muted: false,
            device_name: "test".into(),
            device_channels: 2,
            _stream: None,
            producer,
            click_synth: ClickSynth::new(sample_rate),
            click_delay: ClickDelayBuffer::new(16384),
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
        };

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

        let mut state = MixerState {
            song,
            sample_rate,
            is_playing: true,
            source_frame_pos: 0.0,
            last_beat_index: None,
            active_section_idx: Some(0),
            armed_section_idx: None,
            loop_enabled: false,
            quantize_mode: QuantizeMode::NextBar,
            key_shift_semitones: 0,
            current_bpm: 80.0, // Song is 72 BPM, playing at 80 BPM
            click_gain: 0.85,
            click_muted: false,
            device_name: "test".into(),
            device_channels: 2,
            _stream: None,
            producer,
            click_synth: ClickSynth::new(sample_rate),
            click_delay: ClickDelayBuffer::new(16384),
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
        };

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
        let latency = state.pitched_stretcher.total_latency_frames();
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
}
