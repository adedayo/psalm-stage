use std::f32::consts::{FRAC_PI_4, TAU};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::percussion_patterns::{
    intensity_allows, CustomPercussionPattern, GrooveIntensity, HitVariant, PercussionInstrument,
    PercussionMode, PercussionStyle, ALL_PERCUSSION_INSTRUMENTS,
};

pub const DEFAULT_PERCUSSION_ROOM_MIX: f32 = 0.24;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PercussionChannelConfig {
    pub instrument: PercussionInstrument,
    pub enabled: bool,
    /// Channel mix level: 0.0..=1.5
    pub gain: f32,
    /// Stereo pan: -1.0..=1.0
    pub pan: f32,
}

impl PercussionChannelConfig {
    pub fn default_for(instrument: PercussionInstrument) -> Self {
        Self {
            instrument,
            enabled: true,
            gain: instrument.default_gain(),
            pan: instrument.default_pan(),
        }
    }

    pub fn clamped(mut self) -> Self {
        self.gain = if self.gain.is_finite() {
            self.gain.clamp(0.0, 1.5)
        } else {
            self.instrument.default_gain()
        };
        self.pan = if self.pan.is_finite() {
            self.pan.clamp(-1.0, 1.0)
        } else {
            self.instrument.default_pan()
        };
        self
    }
}

pub fn default_percussion_channels() -> Vec<PercussionChannelConfig> {
    ALL_PERCUSSION_INSTRUMENTS
        .iter()
        .copied()
        .map(PercussionChannelConfig::default_for)
        .collect()
}

/// Decoded stereo percussion one-shot stored in pre-loaded sample kit memory.
#[derive(Debug, Clone)]
pub struct LoadedPercSample {
    pub sample_rate: u32,
    pub left: Vec<f32>,
    pub right: Vec<f32>,
}

/// Fixed 36-slot (9 instruments x 4 variants) sample bank swapped atomically on command.
#[derive(Debug, Clone)]
pub struct PercussionSampleKitData {
    pub kit_name: String,
    /// Indexed by `instrument.index() * 4 + variant.index()`.
    pub slots: Vec<Option<Arc<LoadedPercSample>>>,
}

impl PercussionSampleKitData {
    pub fn empty(kit_name: impl Into<String>) -> Self {
        Self {
            kit_name: kit_name.into(),
            slots: vec![None; ALL_PERCUSSION_INSTRUMENTS.len() * HitVariant::ALL.len()],
        }
    }

    pub fn set_slot(
        &mut self,
        instrument: PercussionInstrument,
        variant: HitVariant,
        sample: Arc<LoadedPercSample>,
    ) {
        let idx = instrument.index() * 4 + variant.index();
        if idx < self.slots.len() {
            self.slots[idx] = Some(sample);
        }
    }

    pub fn resolve(
        &self,
        instrument: PercussionInstrument,
        variant: HitVariant,
    ) -> Option<&Arc<LoadedPercSample>> {
        let base = instrument.index() * 4;
        let exact = base + variant.index();
        if let Some(Some(s)) = self.slots.get(exact) {
            return Some(s);
        }
        // Fallback to Standard sample if specific variant is absent
        if let Some(Some(s)) = self.slots.get(base) {
            return Some(s);
        }
        None
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PercussionLockConfig {
    #[serde(default)]
    pub mode: PercussionMode,
    #[serde(default)]
    pub style: PercussionStyle,
    #[serde(default)]
    pub intensity: GrooveIntensity,
    #[serde(default = "default_one")]
    pub gain: f32,
    #[serde(default)]
    pub iem_send: f32,
    #[serde(default = "default_true")]
    pub follow_sections: bool,
    #[serde(default = "default_room_mix")]
    pub room_mix: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kit_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kit_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_pattern: Option<CustomPercussionPattern>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<PercussionChannelConfig>,
}

fn default_one() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_room_mix() -> f32 {
    DEFAULT_PERCUSSION_ROOM_MIX
}

impl Default for PercussionLockConfig {
    fn default() -> Self {
        Self {
            mode: PercussionMode::Off,
            style: PercussionStyle::Worship44,
            intensity: GrooveIntensity::Medium,
            gain: 0.9,
            iem_send: 0.0,
            follow_sections: true,
            room_mix: DEFAULT_PERCUSSION_ROOM_MIX,
            kit_name: None,
            kit_dir: None,
            custom_pattern: None,
            channels: default_percussion_channels(),
        }
    }
}

/// Maps a song section name (e.g., "Verse 1", "Chorus", "Bridge", "Praise Break")
/// to an appropriate groove intensity.
pub fn section_intensity_from_name(name: &str) -> GrooveIntensity {
    let lower = name.to_ascii_lowercase();
    if lower.contains("bridge")
        || lower.contains("vamp")
        || lower.contains("praise")
        || lower.contains("shout")
        || lower.contains("solo")
        || lower.contains("build")
        || lower.contains("climax")
    {
        GrooveIntensity::High
    } else if lower.contains("chorus")
        || lower.contains("pre-chorus")
        || lower.contains("prechorus")
        || lower.contains("turnaround")
        || lower.contains("hook")
        || lower.contains("refrain")
    {
        GrooveIntensity::Medium
    } else {
        // Intro, Verse, Tag, Prayer, Outro, Interlude, Sermon
        GrooveIntensity::Low
    }
}

/// Pre-allocated stereo delay buffer matching stretcher group delay when layering percussion on stems.
#[derive(Debug, Clone)]
pub struct PercussionDelayBuffer {
    buffer_l: Vec<f32>,
    buffer_r: Vec<f32>,
    write_pos: usize,
}

impl PercussionDelayBuffer {
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(64);
        Self {
            buffer_l: vec![0.0; cap],
            buffer_r: vec![0.0; cap],
            write_pos: 0,
        }
    }

    pub fn clear(&mut self) {
        self.buffer_l.fill(0.0);
        self.buffer_r.fill(0.0);
        self.write_pos = 0;
    }

    #[inline]
    pub fn process_stereo(&mut self, l: f32, r: f32, delay_samples: usize) -> (f32, f32) {
        if delay_samples == 0 {
            return (l, r);
        }
        let cap = self.buffer_l.len();
        let delay = delay_samples.min(cap - 1);
        let read_pos = (self.write_pos + cap - delay) % cap;
        let out_l = self.buffer_l[read_pos];
        let out_r = self.buffer_r[read_pos];
        self.buffer_l[self.write_pos] = l;
        self.buffer_r[self.write_pos] = r;
        self.write_pos = (self.write_pos + 1) % cap;
        (out_l, out_r)
    }
}

/// Lightweight zero-alloc stereo studio room ambience for percussion glue.
#[derive(Debug, Clone)]
struct PercRoomReverb {
    comb_l1: Vec<f32>,
    comb_l2: Vec<f32>,
    comb_r1: Vec<f32>,
    comb_r2: Vec<f32>,
    idx_l1: usize,
    idx_l2: usize,
    idx_r1: usize,
    idx_r2: usize,
    damp_l: f32,
    damp_r: f32,
    hp_l: f32,
    hp_r: f32,
}

impl PercRoomReverb {
    fn new(sample_rate: u32) -> Self {
        let scale = (sample_rate.max(8000) as f32 / 44_100.0).max(0.25);
        let len = |base: usize| ((base as f32 * scale) as usize).max(32);
        Self {
            comb_l1: vec![0.0; len(1117)],
            comb_l2: vec![0.0; len(1499)],
            comb_r1: vec![0.0; len(1181)],
            comb_r2: vec![0.0; len(1553)],
            idx_l1: 0,
            idx_l2: 0,
            idx_r1: 0,
            idx_r2: 0,
            damp_l: 0.0,
            damp_r: 0.0,
            hp_l: 0.0,
            hp_r: 0.0,
        }
    }

    fn clear(&mut self) {
        self.comb_l1.fill(0.0);
        self.comb_l2.fill(0.0);
        self.comb_r1.fill(0.0);
        self.comb_r2.fill(0.0);
        self.idx_l1 = 0;
        self.idx_l2 = 0;
        self.idx_r1 = 0;
        self.idx_r2 = 0;
        self.damp_l = 0.0;
        self.damp_r = 0.0;
        self.hp_l = 0.0;
        self.hp_r = 0.0;
    }

    #[inline]
    fn process_stereo(&mut self, in_l: f32, in_r: f32) -> (f32, f32) {
        // High-pass input to room so sub-kick energy never muddies the stereo tail
        let clean_l = in_l - self.hp_l;
        self.hp_l = self.hp_l * 0.96 + in_l * 0.04;
        let clean_r = in_r - self.hp_r;
        self.hp_r = self.hp_r * 0.96 + in_r * 0.04;

        let fb = 0.56_f32;
        let dl1 = self.comb_l1[self.idx_l1];
        let dl2 = self.comb_l2[self.idx_l2];
        let dr1 = self.comb_r1[self.idx_r1];
        let dr2 = self.comb_r2[self.idx_r2];

        self.damp_l = self.damp_l * 0.42 + (dl1 + dl2) * 0.29;
        self.damp_r = self.damp_r * 0.42 + (dr1 + dr2) * 0.29;

        // Slight cross-channel injection for natural studio stereo bloom
        self.comb_l1[self.idx_l1] = clean_l + (dl1 + dr2 * 0.14) * fb;
        self.comb_l2[self.idx_l2] = clean_l * 0.85 + (self.damp_l) * fb;
        self.comb_r1[self.idx_r1] = clean_r + (dr1 + dl2 * 0.14) * fb;
        self.comb_r2[self.idx_r2] = clean_r * 0.85 + (self.damp_r) * fb;

        self.idx_l1 = (self.idx_l1 + 1) % self.comb_l1.len();
        self.idx_l2 = (self.idx_l2 + 1) % self.comb_l2.len();
        self.idx_r1 = (self.idx_r1 + 1) % self.comb_r1.len();
        self.idx_r2 = (self.idx_r2 + 1) % self.comb_r2.len();

        ((dl1 + dl2) * 0.42, (dr1 + dr2) * 0.42)
    }
}

#[inline]
fn instrument_room_send(inst: PercussionInstrument) -> f32 {
    match inst {
        PercussionInstrument::Kick => 0.10,
        PercussionInstrument::Snare => 0.44,
        PercussionInstrument::HiHat => 0.22,
        PercussionInstrument::HandDrums => 0.36,
        PercussionInstrument::Tambourine => 0.48,
        PercussionInstrument::Shaker => 0.24,
        PercussionInstrument::Claps => 0.56,
        PercussionInstrument::Clave => 0.28,
        PercussionInstrument::Bell => 0.34,
    }
}

/// Fixed-capacity voice slot for hybrid sample + multi-layer acoustic DSP synthesis.
#[derive(Debug, Clone)]
struct PercVoice {
    active: bool,
    instrument: PercussionInstrument,
    variant: HitVariant,
    style: PercussionStyle,
    age_sec: f32,
    duration_sec: f32,
    velocity: f32,
    pitch_mul: f32,
    phase_a: f32,
    phase_b: f32,
    phase_c: f32,
    phase_d: f32,
    phase_e: f32,
    phase_f: f32,
    bp_z1: f32,
    bp_z2: f32,
    hp_z1: f32,
    hp_z2: f32,
    rng_state: u32,
    sample_ref: Option<Arc<LoadedPercSample>>,
    sample_pos: f64,
    sample_step: f64,
}

impl Default for PercVoice {
    fn default() -> Self {
        Self {
            active: false,
            instrument: PercussionInstrument::Kick,
            variant: HitVariant::Standard,
            style: PercussionStyle::Worship44,
            age_sec: 0.0,
            duration_sec: 0.1,
            velocity: 0.0,
            pitch_mul: 1.0,
            phase_a: 0.0,
            phase_b: 0.0,
            phase_c: 0.0,
            phase_d: 0.0,
            phase_e: 0.0,
            phase_f: 0.0,
            bp_z1: 0.0,
            bp_z2: 0.0,
            hp_z1: 0.0,
            hp_z2: 0.0,
            rng_state: 0x9E37_79B9,
            sample_ref: None,
            sample_pos: 0.0,
            sample_step: 1.0,
        }
    }
}

impl PercVoice {
    #[inline]
    fn next_noise(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng_state = x.max(1);
        ((x as i32) as f32) / (i32::MAX as f32)
    }

    fn trigger(
        &mut self,
        instrument: PercussionInstrument,
        variant: HitVariant,
        style: PercussionStyle,
        velocity: f32,
        seed: u32,
        engine_sample_rate: u32,
        sample_opt: Option<Arc<LoadedPercSample>>,
    ) {
        self.active = true;
        self.instrument = instrument;
        self.variant = variant;
        self.style = style;
        self.age_sec = 0.0;
        self.velocity = velocity.clamp(0.05, 1.0);
        // Subtle organic round-robin pitch variation (±0.8%)
        let pitch_jitter = (((seed >> 16) & 0xFF) as f32 / 255.0 - 0.5) * 0.016;
        self.pitch_mul = 1.0 + pitch_jitter;
        self.phase_a = 0.0;
        self.phase_b = 0.0;
        self.phase_c = 0.0;
        self.phase_d = 0.0;
        self.phase_e = 0.0;
        self.phase_f = 0.0;
        self.bp_z1 = 0.0;
        self.bp_z2 = 0.0;
        self.hp_z1 = 0.0;
        self.hp_z2 = 0.0;
        self.rng_state = seed.max(1);

        if let Some(samp) = sample_opt {
            self.sample_step =
                (samp.sample_rate.max(8000) as f64 / engine_sample_rate.max(8000) as f64)
                    * self.pitch_mul as f64;
            self.sample_pos = 0.0;
            self.duration_sec =
                (samp.left.len() as f32 / samp.sample_rate.max(8000) as f32).max(0.02);
            self.sample_ref = Some(samp);
            return;
        }

        self.sample_ref = None;
        self.sample_pos = 0.0;
        self.sample_step = 1.0;
        self.duration_sec = match instrument {
            PercussionInstrument::Kick => 0.28,
            PercussionInstrument::Snare => match (style, variant) {
                (PercussionStyle::AcousticCajon, _) => 0.18,
                (_, HitVariant::Soft) => 0.10,
                _ => 0.22,
            },
            PercussionInstrument::HiHat => match variant {
                HitVariant::Accent => 0.26,
                _ => 0.065,
            },
            PercussionInstrument::HandDrums => match (style, variant) {
                (PercussionStyle::Amapiano, HitVariant::Special) => 0.36,
                (_, HitVariant::Special) => 0.32,
                _ => 0.26,
            },
            PercussionInstrument::Tambourine => match variant {
                HitVariant::Accent => 0.22,
                _ => 0.11,
            },
            PercussionInstrument::Shaker => match variant {
                HitVariant::Accent => 0.085,
                _ => 0.058,
            },
            PercussionInstrument::Claps => 0.20,
            PercussionInstrument::Clave => 0.065,
            PercussionInstrument::Bell => 0.21,
        };
    }

    #[inline]
    fn next_sample_stereo(&mut self, inv_sr: f32) -> (f32, f32) {
        if !self.active {
            return (0.0, 0.0);
        }

        if let Some(ref samp) = self.sample_ref {
            let idx = self.sample_pos as usize;
            if idx >= samp.left.len() {
                self.active = false;
                return (0.0, 0.0);
            }
            let frac = (self.sample_pos - idx as f64) as f32;
            let next_idx = (idx + 1).min(samp.left.len().saturating_sub(1));
            let l = samp.left[idx] * (1.0 - frac) + samp.left[next_idx] * frac;
            let r = samp.right[idx] * (1.0 - frac) + samp.right[next_idx] * frac;
            self.sample_pos += self.sample_step;
            return (l * self.velocity, r * self.velocity);
        }

        let t = self.age_sec;
        self.age_sec += inv_sr;
        if t >= self.duration_sec {
            self.active = false;
            return (0.0, 0.0);
        }

        let pm = self.pitch_mul;
        let (raw_l, raw_r) = match self.instrument {
            PercussionInstrument::Kick => {
                // 3-layer acoustic kick: sub sweep + resonant shell thump + beater click
                let pitch_env = (-t * 44.0).exp();
                let start_hz = if self.variant == HitVariant::Soft {
                    118.0
                } else {
                    165.0
                } * pm;
                let end_hz = if self.style == PercussionStyle::Amapiano {
                    44.0
                } else {
                    48.5
                } * pm;
                let freq = end_hz + (start_hz - end_hz) * pitch_env;
                self.phase_a = (self.phase_a + freq * inv_sr).fract();
                self.phase_b = (self.phase_b + (112.0 * pm) * inv_sr).fract();
                self.phase_c = (self.phase_c + (1650.0 * pm) * inv_sr).fract();

                let sub = (self.phase_a * TAU).sin() * (-t * 12.5).exp();
                let chest = (self.phase_b * TAU).sin() * (-t * 28.0).exp() * 0.34;
                let n = self.next_noise();
                self.bp_z1 = self.bp_z1 * 0.65 + n * 0.35;
                let beater =
                    ((self.phase_c * TAU).sin() * 0.65 + self.bp_z1 * 0.45) * (-t * 240.0).exp();
                let click_amt = if self.variant == HitVariant::Soft {
                    0.18
                } else {
                    0.34
                };
                let mono = ((sub + chest) * 1.12 + beater * click_amt).tanh() * 0.94;
                (mono, mono)
            }
            PercussionInstrument::Snare => {
                let is_cajon = self.style == PercussionStyle::AcousticCajon;
                let is_rim = self.variant == HitVariant::Soft && !is_cajon;
                if is_rim {
                    // Warm wooden cross-stick
                    self.phase_a = (self.phase_a + 885.0 * pm * inv_sr).fract();
                    self.phase_b = (self.phase_b + 1620.0 * pm * inv_sr).fract();
                    let env = (-t * 64.0).exp();
                    let mono = ((self.phase_a * TAU).sin() * 0.66
                        + (self.phase_b * TAU).sin() * 0.34)
                        * env
                        * 0.80;
                    (mono, mono * 0.96)
                } else if is_cajon {
                    // Acoustic wooden cajon bass/slap
                    let f1 = (152.0 + 98.0 * (-t * 52.0).exp()) * pm;
                    self.phase_a = (self.phase_a + f1 * inv_sr).fract();
                    let body = (self.phase_a * TAU).sin() * (-t * 22.0).exp() * 0.72;
                    let n = self.next_noise();
                    self.bp_z1 = self.bp_z1 * 0.72 + n * 0.28;
                    let slap = (n - self.bp_z1) * (-t * 34.0).exp() * 0.58;
                    let mono = (body + slap).tanh() * 0.88;
                    (mono, mono * 0.98)
                } else {
                    // Acoustic snare: dual shell modes + dual-band snare wire shimmer + rim accent
                    let f1 = (192.0 + 75.0 * (-t * 78.0).exp()) * pm;
                    let f2 = 326.0 * pm;
                    self.phase_a = (self.phase_a + f1 * inv_sr).fract();
                    self.phase_b = (self.phase_b + f2 * inv_sr).fract();
                    let shell = ((self.phase_a * TAU).sin() * 0.64
                        + (self.phase_b * TAU).sin() * 0.36)
                        * (-t * 26.0).exp();
                    let n = self.next_noise();
                    let hp1 = n - self.hp_z1 * 0.84;
                    self.hp_z1 = n;
                    self.bp_z1 = self.bp_z1 * 0.62 + hp1 * 0.38;
                    let wire_body = hp1 - self.bp_z1 * 0.35;
                    let wire_env = (-t * 19.5).exp();
                    let rim = if self.variant == HitVariant::Accent {
                        self.phase_c = (self.phase_c + 1240.0 * pm * inv_sr).fract();
                        (self.phase_c * TAU).sin() * (-t * 54.0).exp() * 0.32
                    } else {
                        0.0
                    };
                    let l = (shell * 0.56 + rim + wire_body * wire_env * 0.66).tanh() * 0.88;
                    let r = (shell * 0.56 + rim + (wire_body * 0.92 + hp1 * 0.08) * wire_env * 0.66)
                        .tanh()
                        * 0.88;
                    (l, r)
                }
            }
            PercussionInstrument::HiHat => {
                // 6 inharmonic metallic partials + 2nd-order HPF + crisp stick tip transient
                let freqs = [205.0, 304.0, 370.0, 523.0, 540.0, 800.0];
                self.phase_a = (self.phase_a + freqs[0] * pm * inv_sr).fract();
                self.phase_b = (self.phase_b + freqs[1] * pm * inv_sr).fract();
                self.phase_c = (self.phase_c + freqs[2] * pm * inv_sr).fract();
                self.phase_d = (self.phase_d + freqs[3] * pm * inv_sr).fract();
                self.phase_e = (self.phase_e + freqs[4] * pm * inv_sr).fract();
                self.phase_f = (self.phase_f + freqs[5] * pm * inv_sr).fract();
                let sq = |p: f32| if p < 0.5 { 1.0 } else { -1.0 };
                let metal = (sq(self.phase_a)
                    + sq(self.phase_b)
                    + sq(self.phase_c)
                    + sq(self.phase_d)
                    + sq(self.phase_e)
                    + sq(self.phase_f))
                    * 0.14
                    + self.next_noise() * 0.42;
                let hp1 = metal - self.hp_z1 * 0.90;
                self.hp_z1 = metal;
                let hp2 = hp1 - self.hp_z2 * 0.86;
                self.hp_z2 = hp1;
                let decay = if self.variant == HitVariant::Accent {
                    13.5
                } else {
                    58.0
                };
                let tip = (-t * 220.0).exp() * 0.35;
                let mono = (hp2 + hp1 * tip) * (-t * decay).exp() * 0.58;
                (mono, mono * 0.96)
            }
            PercussionInstrument::HandDrums => {
                if self.style == PercussionStyle::Amapiano && self.variant == HitVariant::Special {
                    // Iconic Amapiano Log Drum: woody resonant pitch drop + soft FM growl
                    let f_sweep = (60.0 + 62.0 * (-t * 34.0).exp()) * pm;
                    self.phase_a = (self.phase_a + f_sweep * inv_sr).fract();
                    self.phase_b = (self.phase_b + (f_sweep * 2.01) * inv_sr).fract();
                    let mod_sig = (self.phase_b * TAU).sin() * 0.78 * (-t * 20.0).exp();
                    let carrier = ((self.phase_a * TAU) + mod_sig).sin();
                    let knock = (-t * 145.0).exp() * (self.phase_b * TAU).sin() * 0.28;
                    let mono = (carrier * (-t * 9.0).exp() * 0.94 + knock).tanh() * 0.95;
                    (mono, mono)
                } else if self.variant == HitVariant::Special {
                    // Expressive African Talking Drum (upward scoop + warm overtone + light skin attack)
                    let glide = 1.0 - (-t * 16.5).exp();
                    let freq = (146.0 + 142.0 * glide) * pm;
                    self.phase_a = (self.phase_a + freq * inv_sr).fract();
                    self.phase_b = (self.phase_b + freq * 1.56 * inv_sr).fract();
                    self.phase_c = (self.phase_c + freq * 2.13 * inv_sr).fract();
                    let skin = self.next_noise() * (-t * 110.0).exp() * 0.22;
                    let body = (self.phase_a * TAU).sin() * 0.72
                        + (self.phase_b * TAU).sin() * 0.24
                        + (self.phase_c * TAU).sin() * 0.10;
                    let mono = (body * (-t * 10.5).exp() + skin).tanh() * 0.88;
                    (mono, mono * 0.96)
                } else {
                    // Circular membrane conga / djembe (fundamental + 1.56 + 2.13 modal ratios)
                    let base = if self.variant == HitVariant::Accent {
                        268.0
                    } else if self.variant == HitVariant::Soft {
                        162.0
                    } else {
                        212.0
                    } * pm;
                    let freq = base + 92.0 * (-t * 68.0).exp();
                    self.phase_a = (self.phase_a + freq * inv_sr).fract();
                    self.phase_b = (self.phase_b + freq * 1.56 * inv_sr).fract();
                    self.phase_c = (self.phase_c + freq * 2.13 * inv_sr).fract();
                    let tone = ((self.phase_a * TAU).sin() * 0.72
                        + (self.phase_b * TAU).sin() * 0.22
                        + (self.phase_c * TAU).sin() * 0.12)
                        * (-t * 14.0).exp();
                    let slap_noise = if self.variant == HitVariant::Accent {
                        let n = self.next_noise();
                        let hp = n - self.hp_z1 * 0.74;
                        self.hp_z1 = n;
                        hp * (-t * 58.0).exp() * 0.42
                    } else {
                        0.0
                    };
                    let mono = (tone * 0.82 + slap_noise).tanh() * 0.88;
                    (mono, mono)
                }
            }
            PercussionInstrument::Tambourine => {
                // Multi-zill brass jingle pairs + realistic double-bounce micro-shimmer
                self.phase_a = (self.phase_a + 4920.0 * pm * inv_sr).fract();
                self.phase_b = (self.phase_b + 6280.0 * pm * inv_sr).fract();
                self.phase_c = (self.phase_c + 7840.0 * pm * inv_sr).fract();
                self.phase_d = (self.phase_d + 9320.0 * pm * inv_sr).fract();
                let zills = (self.phase_a * TAU).sin() * 0.30
                    + (self.phase_b * TAU).sin() * 0.30
                    + (self.phase_c * TAU).sin() * 0.24
                    + (self.phase_d * TAU).sin() * 0.18;
                let n = self.next_noise();
                let hp = n - self.hp_z1 * 0.92;
                self.hp_z1 = n;

                let bounce = |dt: f32, rate: f32| -> f32 {
                    if dt < 0.0 {
                        0.0
                    } else {
                        (-dt * rate).exp()
                    }
                };
                let env = if self.variant == HitVariant::Accent {
                    (bounce(t, 28.0) + bounce(t - 0.017, 26.0) * 0.72 + bounce(t - 0.036, 18.0) * 0.58)
                        .min(1.2)
                } else {
                    (bounce(t, 44.0) + bounce(t - 0.015, 38.0) * 0.55).min(1.1)
                };
                let attack = (t / 0.003).min(1.0);
                let l = (zills * 0.46 + hp * 0.54) * attack * env * 0.70;
                let r = ((zills * 0.50 + hp * 0.50) * 0.96) * attack * env * 0.70;
                (l, r)
            }
            PercussionInstrument::Shaker => {
                // Dual-grain cabasa/maraca bead envelope with resonant high-band shaping
                let n = self.next_noise();
                self.bp_z1 = self.bp_z1 * 0.54 + n * 0.46;
                let hp = self.bp_z1 - self.hp_z1 * 0.87;
                self.hp_z1 = self.bp_z1;
                let attack_sec = if self.variant == HitVariant::Accent {
                    0.006
                } else {
                    0.015
                };
                let attack = (t / attack_sec).min(1.0);
                let decay = if self.variant == HitVariant::Accent {
                    34.0
                } else {
                    48.0
                };
                let grain_tail = if t > 0.018 {
                    (-(t - 0.018) * 55.0).exp() * 0.25
                } else {
                    0.0
                };
                let mono = hp * (attack * (-t * decay).exp() + grain_tail) * 0.85;
                (mono, mono * 0.94)
            }
            PercussionInstrument::Claps => {
                // 5 staggered human hands with stereo width + palm/snap dual formant
                let burst_env = |dt: f32, rate: f32| -> f32 {
                    if dt < 0.0 {
                        0.0
                    } else {
                        (-dt * rate).exp()
                    }
                };
                let e1 = burst_env(t, 180.0);
                let e2 = burst_env(t - 0.0085, 175.0) * 0.88;
                let e3 = burst_env(t - 0.0165, 170.0) * 0.82;
                let e4 = burst_env(t - 0.025, 165.0) * 0.76;
                let tail = burst_env(t - 0.034, 21.0) * 0.95;
                let env_l = (e1 + e2 * 0.8 + e3 + e4 * 0.7 + tail).min(1.4);
                let env_r = (e1 * 0.75 + e2 + e3 * 0.8 + e4 + tail * 0.96).min(1.4);

                let n = self.next_noise();
                self.bp_z1 = self.bp_z1 * 0.74 + n * 0.26;
                let bp = self.bp_z1 - self.hp_z1 * 0.79;
                self.hp_z1 = self.bp_z1;
                // Add crisp palm snap ~1.25 kHz
                self.bp_z2 = self.bp_z2 * 0.82 + n * 0.18;
                let palm = self.bp_z2 - self.hp_z2 * 0.72;
                self.hp_z2 = self.bp_z2;

                let sig = bp * 0.72 + palm * 0.42;
                let l = (sig * env_l * 1.42).tanh() * 0.86;
                let r = (sig * env_r * 1.42).tanh() * 0.86;
                (l, r)
            }
            PercussionInstrument::Clave => {
                // Rosewood dual-mode body resonance + wood click
                let pitch_drop = 1.0 + 0.020 * (-t * 150.0).exp();
                let f1 = if self.variant == HitVariant::Soft {
                    2140.0
                } else {
                    2480.0
                } * pm
                    * pitch_drop;
                let f2 = f1 * 1.52;
                self.phase_a = (self.phase_a + f1 * inv_sr).fract();
                self.phase_b = (self.phase_b + f2 * inv_sr).fract();
                let click = self.next_noise() * (-t * 320.0).exp() * 0.25;
                let osc = (self.phase_a * TAU).sin() * 0.76 + (self.phase_b * TAU).sin() * 0.28;
                let mono = (osc * (-t * 82.0).exp() + click) * 0.78;
                (mono, mono)
            }
            PercussionInstrument::Bell => {
                // African High/Low Agogo or Funk/Gospel damped cowbell
                let is_agogo = matches!(
                    self.style,
                    PercussionStyle::Afrobeat
                        | PercussionStyle::HighlifePraise
                        | PercussionStyle::Woro
                        | PercussionStyle::Makossa
                );
                let (f1, f2, f3) = if is_agogo {
                    if self.variant == HitVariant::Soft {
                        (665.0 * pm, 998.0 * pm, 1680.0 * pm)
                    } else {
                        (935.0 * pm, 1408.0 * pm, 2345.0 * pm)
                    }
                } else {
                    (562.0 * pm, 845.0 * pm, 1390.0 * pm)
                };
                self.phase_a = (self.phase_a + f1 * inv_sr).fract();
                self.phase_b = (self.phase_b + f2 * inv_sr).fract();
                self.phase_c = (self.phase_c + f3 * inv_sr).fract();
                let sq = |p: f32| if p < 0.5 { 1.0 } else { -1.0 };
                let raw_bell = sq(self.phase_a) * 0.36
                    + (self.phase_b * TAU).sin() * 0.48
                    + (self.phase_c * TAU).sin() * 0.16 * (-t * 48.0).exp();
                self.bp_z1 = self.bp_z1 * 0.68 + raw_bell * 0.32;
                let mono = self.bp_z1 * (-t * 22.0).exp() * 0.68;
                (mono, mono)
            }
        };

        (raw_l * self.velocity, raw_r * self.velocity)
    }
}

const MAX_PERC_VOICES: usize = 28;

#[derive(Debug, Clone)]
pub struct PercussionEngine {
    pub mode: PercussionMode,
    pub style: PercussionStyle,
    pub intensity: GrooveIntensity,
    pub follow_sections: bool,
    pub gain: f32,
    pub iem_send: f32,
    pub room_mix: f32,
    pub active_kit_name: Option<String>,
    pub active_kit_dir: Option<String>,
    pub custom_pattern: Option<CustomPercussionPattern>,
    pub current_step_in_cycle: Option<u8>,
    pub channels: Vec<PercussionChannelConfig>,
    sample_rate: u32,
    voices: Vec<PercVoice>,
    next_voice_idx: usize,
    last_step_index: Option<i64>,
    delay_buffer: PercussionDelayBuffer,
    room_reverb: PercRoomReverb,
    sample_kit: Option<Arc<PercussionSampleKitData>>,
    step_seed: u32,
}

impl PercussionEngine {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate.max(8000);
        let cap = (sr as usize / 2).max(32768);
        Self {
            mode: PercussionMode::Off,
            style: PercussionStyle::Worship44,
            intensity: GrooveIntensity::Medium,
            follow_sections: true,
            gain: 0.9,
            iem_send: 0.0,
            room_mix: DEFAULT_PERCUSSION_ROOM_MIX,
            active_kit_name: None,
            active_kit_dir: None,
            custom_pattern: None,
            current_step_in_cycle: None,
            channels: default_percussion_channels(),
            sample_rate: sr,
            voices: vec![PercVoice::default(); MAX_PERC_VOICES],
            next_voice_idx: 0,
            last_step_index: None,
            delay_buffer: PercussionDelayBuffer::new(cap),
            room_reverb: PercRoomReverb::new(sr),
            sample_kit: None,
            step_seed: 0x1357_9BDF,
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        let sr = sample_rate.max(8000);
        self.sample_rate = sr;
        let cap = (sr as usize / 2).max(32768);
        self.delay_buffer = PercussionDelayBuffer::new(cap);
        self.room_reverb = PercRoomReverb::new(sr);
        self.reset_transport();
    }

    pub fn set_sample_kit(&mut self, kit: Option<PercussionSampleKitData>, kit_dir: Option<String>) {
        if let Some(k) = kit {
            self.active_kit_name = Some(k.kit_name.clone());
            self.active_kit_dir = kit_dir;
            self.sample_kit = Some(Arc::new(k));
        } else {
            self.active_kit_name = None;
            self.active_kit_dir = None;
            self.sample_kit = None;
        }
    }

    pub fn set_custom_pattern(&mut self, pattern: Option<CustomPercussionPattern>) {
        self.custom_pattern = pattern.map(|p| p.clamped_and_sorted());
        self.last_step_index = None;
        self.current_step_in_cycle = None;
    }

    pub fn reset_transport(&mut self) {
        self.last_step_index = None;
        self.current_step_in_cycle = None;
        self.delay_buffer.clear();
        self.room_reverb.clear();
        for v in &mut self.voices {
            v.active = false;
        }
    }

    pub fn reset_step_cursor(&mut self) {
        self.last_step_index = None;
        self.current_step_in_cycle = None;
    }

    pub fn is_enabled(&self) -> bool {
        self.mode != PercussionMode::Off
    }

    pub fn is_active(&self) -> bool {
        self.is_enabled() || self.voices.iter().any(|v| v.active)
    }

    pub fn reset_mix(&mut self) {
        self.channels = default_percussion_channels();
        self.gain = 0.9;
        self.room_mix = DEFAULT_PERCUSSION_ROOM_MIX;
    }

    pub fn active_steps_per_bar(&self) -> u8 {
        if let Some(ref custom) = self.custom_pattern {
            custom.steps_per_bar
        } else {
            self.style.pattern().steps_per_bar
        }
    }

    pub fn set_channel(
        &mut self,
        instrument: PercussionInstrument,
        enabled: Option<bool>,
        gain: Option<f32>,
        pan: Option<f32>,
    ) {
        if let Some(ch) = self.channels.iter_mut().find(|c| c.instrument == instrument) {
            if let Some(en) = enabled {
                ch.enabled = en;
            }
            if let Some(g) = gain {
                ch.gain = if g.is_finite() { g.clamp(0.0, 1.5) } else { ch.gain };
            }
            if let Some(p) = pan {
                ch.pan = if p.is_finite() { p.clamp(-1.0, 1.0) } else { ch.pan };
            }
        }
    }

    pub fn apply_lock(&mut self, lock: &PercussionLockConfig) {
        self.mode = lock.mode;
        self.style = lock.style;
        self.intensity = lock.intensity;
        self.gain = lock.gain.clamp(0.0, 1.5);
        self.iem_send = lock.iem_send.clamp(0.0, 1.0);
        self.follow_sections = lock.follow_sections;
        self.room_mix = lock.room_mix.clamp(0.0, 1.0);
        self.custom_pattern = lock
            .custom_pattern
            .as_ref()
            .map(|p| p.clone().clamped_and_sorted());
        if lock.kit_name.is_some() {
            self.active_kit_name = lock.kit_name.clone();
        }
        if lock.channels.len() == ALL_PERCUSSION_INSTRUMENTS.len() {
            self.channels = lock.channels.iter().cloned().map(|c| c.clamped()).collect();
        } else {
            self.channels = default_percussion_channels();
            for incoming in &lock.channels {
                self.set_channel(
                    incoming.instrument,
                    Some(incoming.enabled),
                    Some(incoming.gain),
                    Some(incoming.pan),
                );
            }
        }
    }

    /// Auditions an instrument hit immediately (for interactive step painting & pad testing).
    pub fn trigger_preview(
        &mut self,
        instrument: PercussionInstrument,
        variant: HitVariant,
        velocity: f32,
    ) {
        self.trigger_instrument_internal(instrument, variant, velocity.clamp(0.1, 1.0), false);
    }

    /// Evaluates continuous beat position (`beat_float >= 0.0`) and triggers any newly entered step.
    pub fn advance_and_trigger_step(&mut self, continuous_beat: f64, beats_per_bar: u32) {
        self.advance_and_trigger_step_forced(continuous_beat, beats_per_bar, false);
    }

    /// Evaluates continuous beat position and triggers newly entered steps (optionally bypassing `mode == Off` for Loop Studio audition).
    pub fn advance_and_trigger_step_forced(
        &mut self,
        continuous_beat: f64,
        beats_per_bar: u32,
        force_enable: bool,
    ) {
        if (!force_enable && !self.is_enabled()) || !continuous_beat.is_finite() || continuous_beat < 0.0 {
            return;
        }
        let (steps_per_bar, bars_in_cycle, swing) = if let Some(ref custom) = self.custom_pattern {
            (
                custom.steps_per_bar,
                custom.bars_in_cycle,
                custom.swing,
            )
        } else {
            let spec = self.style.pattern();
            (spec.steps_per_bar, spec.bars_in_cycle, spec.swing)
        };

        let bpb = beats_per_bar.max(1) as f64;
        let bar_pos = continuous_beat / bpb;
        let raw_step = bar_pos * (steps_per_bar as f64);

        let step_floor = raw_step.floor();
        let step_frac = raw_step - step_floor;
        let step_int = step_floor as i64;
        let steps_per_beat = (steps_per_bar.max(1) / beats_per_bar.max(1).min(steps_per_bar as u32) as u8).max(1) as i64;
        let step_in_bar = step_int.rem_euclid(steps_per_bar.max(1) as i64);
        let is_on_beat = (step_in_bar % steps_per_beat) == 0;
        let adjusted_step = if swing > 0.0 && !is_on_beat && (step_int & 1) == 1 {
            if step_frac < swing as f64 {
                step_int - 1
            } else {
                step_int
            }
        } else {
            step_int
        };

        if adjusted_step < 0 {
            return;
        }
        if self.last_step_index == Some(adjusted_step) {
            return;
        }
        self.last_step_index = Some(adjusted_step);

        let cycle_len = (steps_per_bar as i64) * (bars_in_cycle as i64);
        let step_in_cycle = adjusted_step.rem_euclid(cycle_len.max(1)) as u8;
        self.current_step_in_cycle = Some(step_in_cycle);

        if let Some(custom) = self.custom_pattern.take() {
            for hit in &custom.hits {
                if hit.step == step_in_cycle
                    && intensity_allows(hit.min_intensity, self.intensity)
                {
                    self.trigger_instrument_internal(
                        hit.instrument,
                        hit.variant,
                        hit.velocity,
                        true,
                    );
                }
            }
            self.custom_pattern = Some(custom);
        } else {
            let spec = self.style.pattern();
            for hit in spec.hits {
                if hit.step == step_in_cycle
                    && intensity_allows(hit.min_intensity, self.intensity)
                {
                    self.trigger_instrument_internal(
                        hit.instrument,
                        hit.variant,
                        hit.velocity,
                        true,
                    );
                }
            }
        }
    }

    fn trigger_instrument_internal(
        &mut self,
        instrument: PercussionInstrument,
        variant: HitVariant,
        velocity: f32,
        respect_mute: bool,
    ) {
        if respect_mute {
            let enabled = self
                .channels
                .iter()
                .find(|c| c.instrument == instrument)
                .map(|c| c.enabled)
                .unwrap_or(true);
            if !enabled {
                return;
            }
        }

        // Deterministic micro-humanization on velocity (±5%)
        self.step_seed = self
            .step_seed
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let jitter = (((self.step_seed >> 24) as f32) / 255.0 - 0.5) * 0.10;
        let vel = (velocity * (1.0 + jitter)).clamp(0.05, 1.0);

        // Choke closed HiHat when a new HiHat triggers
        if instrument == PercussionInstrument::HiHat {
            for v in &mut self.voices {
                if v.active && v.instrument == PercussionInstrument::HiHat {
                    v.active = false;
                }
            }
        }

        let sample_opt = self
            .sample_kit
            .as_ref()
            .and_then(|kit| kit.resolve(instrument, variant).cloned());

        let idx = self.next_voice_idx % MAX_PERC_VOICES;
        self.next_voice_idx = (idx + 1) % MAX_PERC_VOICES;
        self.voices[idx].trigger(
            instrument,
            variant,
            self.style,
            vel,
            self.step_seed,
            self.sample_rate,
            sample_opt,
        );
    }

    /// Renders one stereo sample `(l, r)` with studio room ambience and optional latency delay.
    #[inline]
    pub fn next_stereo_sample(&mut self, latency_delay_samples: usize) -> (f32, f32) {
        let inv_sr = 1.0 / (self.sample_rate as f32);
        let mut dry_l = 0.0_f32;
        let mut dry_r = 0.0_f32;
        let mut wet_in_l = 0.0_f32;
        let mut wet_in_r = 0.0_f32;

        for voice in &mut self.voices {
            if !voice.active {
                continue;
            }
            let (sl, sr) = voice.next_sample_stereo(inv_sr);
            if sl == 0.0 && sr == 0.0 {
                continue;
            }
            let (ch_gain, ch_pan) = self
                .channels
                .iter()
                .find(|c| c.instrument == voice.instrument)
                .map(|c| (if c.enabled { c.gain } else { 0.0 }, c.pan))
                .unwrap_or((1.0, 0.0));
            if ch_gain == 0.0 {
                continue;
            }
            let theta = (ch_pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
            let pan_l = sl * ch_gain * theta.cos();
            let pan_r = sr * ch_gain * theta.sin();
            dry_l += pan_l;
            dry_r += pan_r;
            let room_send = instrument_room_send(voice.instrument);
            wet_in_l += pan_l * room_send;
            wet_in_r += pan_r * room_send;
        }

        let room_amt = self.room_mix.clamp(0.0, 1.0);
        let (sum_l, sum_r) = if room_amt > 0.001 {
            let (rev_l, rev_r) = self.room_reverb.process_stereo(wet_in_l, wet_in_r);
            (dry_l + rev_l * room_amt, dry_r + rev_r * room_amt)
        } else {
            (dry_l, dry_r)
        };

        let master = self.gain.clamp(0.0, 1.5);
        let out_l = (sum_l * master).tanh();
        let out_r = (sum_r * master).tanh();

        self.delay_buffer
            .process_stereo(out_l, out_r, latency_delay_samples)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::percussion_patterns::CustomPatternHit;

    #[test]
    fn test_percussion_all_styles_produce_bounded_audio() {
        for sr in [44_100u32, 48_000, 96_000] {
            for &style in PercussionStyle::all() {
                for intensity in [
                    GrooveIntensity::Low,
                    GrooveIntensity::Medium,
                    GrooveIntensity::High,
                ] {
                    let mut engine = PercussionEngine::new(sr);
                    engine.mode = PercussionMode::PercussionOnly;
                    engine.style = style;
                    engine.intensity = intensity;

                    let bpm = 112.0;
                    let spb = 60.0 / bpm;
                    let bpb = if matches!(
                        style,
                        PercussionStyle::Woro | PercussionStyle::Worship68
                    ) {
                        6
                    } else {
                        4
                    };
                    let total_frames = ((spb * (bpb as f64) * 2.0) * sr as f64) as usize;
                    let mut peak = 0.0_f32;
                    let mut energy = 0.0_f64;

                    for frame in 0..total_frames {
                        let beat = (frame as f64 / sr as f64) / spb;
                        engine.advance_and_trigger_step(beat, bpb);
                        let (l, r) = engine.next_stereo_sample(0);
                        assert!(l.is_finite() && r.is_finite(), "NaN/Inf in {style:?}");
                        peak = peak.max(l.abs()).max(r.abs());
                        energy += (l as f64 * l as f64) + (r as f64 * r as f64);
                    }
                    let rms = (energy / (total_frames as f64 * 2.0)).sqrt();
                    assert!(
                        rms > 0.005,
                        "Expected audible RMS for {style:?} {intensity:?} @ {sr}Hz, got {rms}"
                    );
                    assert!(
                        peak <= 1.0,
                        "Expected bounded peak <= 1.0 for {style:?} {intensity:?}, got {peak}"
                    );
                }
            }
        }
    }

    #[test]
    fn test_percussion_channel_mute_and_solo_isolation() {
        let mut engine = PercussionEngine::new(48_000);
        engine.mode = PercussionMode::PercussionOnly;
        engine.style = PercussionStyle::Afrobeat;
        engine.intensity = GrooveIntensity::High;

        // Mute every instrument except Clave
        for ch in &mut engine.channels {
            ch.enabled = ch.instrument == PercussionInstrument::Clave;
        }
        let mut clave_energy = 0.0_f64;
        for f in 0..24_000 {
            let beat = (f as f64 / 48_000.0) * 2.0;
            engine.advance_and_trigger_step(beat, 4);
            let (l, r) = engine.next_stereo_sample(0);
            clave_energy += (l as f64).powi(2) + (r as f64).powi(2);
        }
        assert!(clave_energy > 0.1, "Clave-only mix should produce energy");

        // Now mute all instruments -> silence (also clear room reverb tail from prior hits)
        engine.reset_transport();
        for ch in &mut engine.channels {
            ch.enabled = false;
        }
        let mut muted_peak = 0.0_f32;
        for f in 0..24_000 {
            let beat = (f as f64 / 48_000.0) * 2.0;
            engine.advance_and_trigger_step(beat, 4);
            let (l, r) = engine.next_stereo_sample(0);
            muted_peak = muted_peak.max(l.abs()).max(r.abs());
        }
        assert_eq!(muted_peak, 0.0, "Muting all channels must produce silence");
    }

    #[test]
    fn test_percussion_delay_buffer_matches_stretcher_latency() {
        let mut engine = PercussionEngine::new(48_000);
        engine.mode = PercussionMode::Layer;
        engine.style = PercussionStyle::Worship44;
        let latency = 5760usize;

        engine.advance_and_trigger_step(0.0, 4);
        let mut first_nonzero_frame = None;
        for f in 0..(latency + 200) {
            let (l, r) = engine.next_stereo_sample(latency);
            if first_nonzero_frame.is_none() && (l.abs() > 1e-6 || r.abs() > 1e-6) {
                first_nonzero_frame = Some(f);
            }
        }
        assert_eq!(
            first_nonzero_frame,
            Some(latency),
            "Delayed percussion onset must match stretcher latency sample-for-sample"
        );
    }

    #[test]
    fn test_custom_pattern_and_sample_kit_playback() {
        let mut engine = PercussionEngine::new(48_000);
        engine.mode = PercussionMode::PercussionOnly;
        engine.room_mix = 0.0;

        let mut kit = PercussionSampleKitData::empty("UnitKit");
        kit.set_slot(
            PercussionInstrument::Kick,
            HitVariant::Standard,
            Arc::new(LoadedPercSample {
                sample_rate: 48_000,
                left: vec![0.6; 400],
                right: vec![0.6; 400],
            }),
        );
        engine.set_sample_kit(Some(kit), None);
        engine.set_custom_pattern(Some(CustomPercussionPattern {
            id: "test-loop".into(),
            name: "Test Loop".into(),
            steps_per_bar: 16,
            bars_in_cycle: 1,
            swing: 0.0,
            is_compound: false,
            hits: vec![CustomPatternHit {
                step: 0,
                instrument: PercussionInstrument::Kick,
                velocity: 1.0,
                variant: HitVariant::Standard,
                min_intensity: GrooveIntensity::Low,
            }],
        }));

        engine.advance_and_trigger_step(0.0, 4);
        let (l, r) = engine.next_stereo_sample(0);
        assert!(
            l > 0.2 && r > 0.2,
            "Sample kit kick should play back sample buffer on step 0, got ({l}, {r})"
        );
        assert_eq!(engine.current_step_in_cycle, Some(0));
    }

    #[test]
    fn test_compound_twelve_step_swing_keeps_beats_aligned_to_click() {
        let mut engine = PercussionEngine::new(48_000);
        engine.mode = PercussionMode::PercussionOnly;
        engine.set_custom_pattern(Some(CustomPercussionPattern {
            id: "swung-12".into(),
            name: "Swung 12/8".into(),
            steps_per_bar: 12,
            bars_in_cycle: 1,
            swing: 0.15,
            is_compound: true,
            hits: vec![
                CustomPatternHit {
                    step: 0,
                    instrument: PercussionInstrument::Kick,
                    velocity: 0.9,
                    variant: HitVariant::Standard,
                    min_intensity: GrooveIntensity::Low,
                },
                CustomPatternHit {
                    step: 3, // Beat 2 of 4/4 dotted-quarter bar (odd step index!)
                    instrument: PercussionInstrument::Snare,
                    velocity: 0.9,
                    variant: HitVariant::Standard,
                    min_intensity: GrooveIntensity::Low,
                },
            ],
        }));

        // Exactly on Beat 2 (continuous_beat = 1.0 of 4 beats): step 3 must fire immediately, not swing!
        engine.advance_and_trigger_step(1.0, 4);
        assert_eq!(
            engine.current_step_in_cycle,
            Some(3),
            "Beat 2 step 3 in a 12-step bar must fire exactly on beat 1.0 even with swing > 0"
        );
    }
}
