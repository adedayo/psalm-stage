use std::f32::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use serde::{Deserialize, Serialize};

pub const CHROMATIC_KEYS: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

pub const MAX_PAD_LAYERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PadKeyQuality {
    #[default]
    Major,
    Minor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PadVoiceArchetype {
    #[default]
    WarmAnalog,
    BrightSynth,
    EvolvingMotion,
    StringsCello,
    OrganPad,
    GuitarSwell,
    ReversePiano,
    AiryChoir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PadRegister {
    Low,
    #[default]
    Mid,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PadTonality {
    #[default]
    Auto,
    OpenFifth,
    Major,
    Minor,
    Melodic,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadLayerConfig {
    pub enabled: bool,
    pub archetype: PadVoiceArchetype,
    pub register: PadRegister,
    pub tonality: PadTonality,
    pub gain: f32,
}

impl Default for PadLayerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            archetype: PadVoiceArchetype::WarmAnalog,
            register: PadRegister::Mid,
            tonality: PadTonality::Auto,
            gain: 0.75,
        }
    }
}

impl PadLayerConfig {
    pub fn clamped(mut self) -> Self {
        self.gain = if self.gain.is_finite() {
            self.gain.clamp(0.0, 1.0)
        } else {
            0.75
        };
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PadPatchSpec {
    #[serde(default)]
    pub preset_id: Option<String>,
    pub preset_name: String,
    pub layers: Vec<PadLayerConfig>,
    pub brightness: f32,
    pub shimmer: f32,
    #[serde(default)]
    pub pan: f32,
    #[serde(default)]
    pub crossfade_sec: Option<f32>,
}

/// Parses a musical key string into chromatic pitch class (`0..12`, `C = 0`) and `PadKeyQuality`.
pub fn parse_pad_key(raw: &str) -> Option<(usize, PadKeyQuality)> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let lower = trimmed.to_ascii_lowercase();
    let (root_only, quality) = if let Some(r) = lower
        .strip_suffix("minor")
        .or_else(|| lower.strip_suffix("min"))
    {
        (r.trim(), PadKeyQuality::Minor)
    } else if let Some(r) = lower
        .strip_suffix("major")
        .or_else(|| lower.strip_suffix("maj"))
    {
        (r.trim(), PadKeyQuality::Major)
    } else if let Some(r) = lower.strip_suffix('m') {
        (r.trim(), PadKeyQuality::Minor)
    } else {
        (lower.as_str(), PadKeyQuality::Major)
    };

    let pc = match root_only {
        "c" | "b#" => Some(0),
        "c#" | "db" => Some(1),
        "d" => Some(2),
        "d#" | "eb" => Some(3),
        "e" | "fb" => Some(4),
        "f" | "e#" => Some(5),
        "f#" | "gb" => Some(6),
        "g" => Some(7),
        "g#" | "ab" => Some(8),
        "a" => Some(9),
        "a#" | "bb" => Some(10),
        "b" | "cb" => Some(11),
        _ => None,
    }?;

    Some((pc, quality))
}

/// Parses any musical key string (including sharps, flats, and minor suffixes like "Am" or "F#m")
/// into a chromatic pitch class `0..12` where `C = 0`.
pub fn key_to_pitch_class(raw: &str) -> Option<usize> {
    parse_pad_key(raw).map(|(pc, _)| pc)
}

pub fn format_pad_key(pitch_class: usize, quality: PadKeyQuality) -> String {
    let root = CHROMATIC_KEYS[pitch_class % 12];
    match quality {
        PadKeyQuality::Major => root.to_string(),
        PadKeyQuality::Minor => format!("{root}m"),
    }
}

pub fn normalize_key_label(raw: &str) -> String {
    match parse_pad_key(raw) {
        Some((pc, quality)) => format_pad_key(pc, quality),
        None => "C".to_string(),
    }
}

pub fn transpose_pad_key(raw: &str, semitones: i32) -> String {
    match parse_pad_key(raw) {
        Some((pc, quality)) => {
            let next_pc = (pc as i32 + semitones).rem_euclid(12) as usize;
            format_pad_key(next_pc, quality)
        }
        None => "C".to_string(),
    }
}

/// Equal-temperament root frequency in the warm C3..B3 register (A4 = 440 Hz).
pub fn pitch_class_root_hz(pitch_class: usize) -> f32 {
    let pc = (pitch_class % 12) as f32;
    440.0 * 2.0_f32.powf((pc - 21.0) / 12.0)
}

#[inline]
fn resolve_effective_tonality(tonality: PadTonality, quality: PadKeyQuality) -> PadTonality {
    match tonality {
        PadTonality::Auto => match quality {
            PadKeyQuality::Minor => PadTonality::Minor,
            PadKeyQuality::Major => PadTonality::OpenFifth,
        },
        other => other,
    }
}

#[inline]
fn voicing_semitones_and_weights(
    tonality: PadTonality,
    register: PadRegister,
) -> ([f32; 4], [f32; 4]) {
    let reg_shift = match register {
        PadRegister::Low => -12.0_f32,
        PadRegister::Mid => 0.0_f32,
        PadRegister::High => 12.0_f32,
    };
    let (intervals, weights) = match tonality {
        PadTonality::OpenFifth | PadTonality::Auto => {
            ([-12.0_f32, 0.0, 7.0, 12.0], [0.22_f32, 0.36, 0.25, 0.17])
        }
        PadTonality::Major => ([0.0_f32, 4.0, 7.0, 12.0], [0.36_f32, 0.17, 0.27, 0.20]),
        PadTonality::Minor => ([0.0_f32, 3.0, 7.0, 12.0], [0.35_f32, 0.21, 0.25, 0.19]),
        PadTonality::Melodic => ([0.0_f32, 7.0, 12.0, 14.0], [0.32_f32, 0.26, 0.22, 0.20]),
    };
    (
        [
            intervals[0] + reg_shift,
            intervals[1] + reg_shift,
            intervals[2] + reg_shift,
            intervals[3] + reg_shift,
        ],
        weights,
    )
}

#[derive(Debug, Clone)]
struct PadLayerVoice {
    phases_l: [f32; 4],
    phases_r: [f32; 4],
    lfo_a: f32,
    lfo_b: f32,
    grain_phase: f32,
    lp_l: f32,
    lp_r: f32,
    smoothed_gain: f32,
}

impl PadLayerVoice {
    fn new(seed: usize, initial_gain: f32) -> Self {
        let offset = (seed as f32) * 0.137;
        Self {
            phases_l: [
                (0.00 + offset).fract(),
                (0.17 + offset).fract(),
                (0.41 + offset).fract(),
                (0.63 + offset).fract(),
            ],
            phases_r: [
                (0.25 + offset).fract(),
                (0.52 + offset).fract(),
                (0.78 + offset).fract(),
                (0.11 + offset).fract(),
            ],
            lfo_a: (seed as f32 * 0.29).fract(),
            lfo_b: (seed as f32 * 0.61).fract(),
            grain_phase: (seed as f32 * 0.23).fract(),
            lp_l: 0.0,
            lp_r: 0.0,
            smoothed_gain: initial_gain,
        }
    }

    #[inline]
    fn synthesize_partial(archetype: PadVoiceArchetype, phase: f32, lfo_b: f32) -> f32 {
        let tau_p = phase * TAU;
        match archetype {
            PadVoiceArchetype::WarmAnalog => {
                // Soft-saturated sine + warm 2nd harmonic + gentle tri-saw
                let tri = 1.0 - 4.0 * (phase - 0.5).abs();
                let base = tau_p.sin() * 0.72 + tri * 0.28 + 0.10 * (tau_p * 2.0).sin();
                (base * 1.15).tanh()
            }
            PadVoiceArchetype::BrightSynth => {
                // Saw-rich choral blend with octave presence
                let saw = 2.0 * phase - 1.0;
                let soft_saw = saw - 0.35 * (tau_p * 3.0).sin();
                (tau_p.sin() * 0.50 + soft_saw * 0.35 + 0.22 * (tau_p * 2.0).sin()).tanh()
            }
            PadVoiceArchetype::EvolvingMotion => {
                // Phase-modulated harmonic drift driven by secondary LFO
                let mod_amt = 0.35 + 0.30 * (lfo_b * TAU).sin();
                let pm = (tau_p + mod_amt * (tau_p * 2.0).sin()).sin();
                pm * 0.78 + 0.22 * (tau_p * 3.0).sin()
            }
            PadVoiceArchetype::StringsCello => {
                // Odd-harmonic bow body + warm 2nd harmonic
                let odd = tau_p.sin() * 0.66
                    + 0.22 * (tau_p * 3.0).sin()
                    + 0.09 * (tau_p * 5.0).sin();
                (odd + 0.14 * (tau_p * 2.0).sin()).tanh()
            }
            PadVoiceArchetype::OrganPad => {
                // Drawbar stack (16'/8'/5th/4')
                (0.46 * tau_p.sin()
                    + 0.30 * (tau_p * 2.0).sin()
                    + 0.16 * (tau_p * 3.0).sin()
                    + 0.10 * (tau_p * 4.0).sin())
                .tanh()
            }
            PadVoiceArchetype::GuitarSwell => {
                // Shimmering harmonic swell with 2nd + 4th sparkle
                (0.62 * tau_p.sin()
                    + 0.24 * (tau_p * 2.0).sin()
                    + 0.12 * (tau_p * 4.0).sin())
                .tanh()
            }
            PadVoiceArchetype::ReversePiano => {
                // Clean piano-bell partials (shaped externally by overlapping reverse grains)
                0.68 * tau_p.sin()
                    + 0.22 * (tau_p * 2.0).sin()
                    + 0.10 * (tau_p * 3.0).sin()
            }
            PadVoiceArchetype::AiryChoir => {
                // Vowel-like formant pair (fundamental + warm 2nd + breathy 5th)
                0.58 * tau_p.sin()
                    + 0.28 * (tau_p * 2.0).sin()
                    + 0.14 * (tau_p * 5.0).sin()
            }
        }
    }

    #[inline]
    fn next_sample(
        &mut self,
        cfg: &PadLayerConfig,
        pitch_class: usize,
        quality: PadKeyQuality,
        sample_rate: f32,
    ) -> ((f32, f32), (f32, f32)) {
        // Smooth per-layer gain (~30 ms)
        let gain_alpha = 1.0 - (-1.0 / (0.03 * sample_rate)).exp();
        let target_g = if cfg.enabled { cfg.gain } else { 0.0 };
        self.smoothed_gain += gain_alpha * (target_g - self.smoothed_gain);

        if self.smoothed_gain <= 1e-5 {
            return ((0.0, 0.0), (0.0, 0.0));
        }

        let (lfo_a_hz, lfo_b_hz, detune_scale, cutoff_hz) = match cfg.archetype {
            PadVoiceArchetype::WarmAnalog => (0.11, 0.05, 1.0, 780.0),
            PadVoiceArchetype::BrightSynth => (0.16, 0.07, 1.45, 2900.0),
            PadVoiceArchetype::EvolvingMotion => (0.19, 0.083, 1.25, 1650.0),
            PadVoiceArchetype::StringsCello => (0.24, 0.13, 1.35, 1350.0),
            PadVoiceArchetype::OrganPad => (0.55, 0.09, 0.70, 1900.0),
            PadVoiceArchetype::GuitarSwell => (0.14, 0.06, 1.15, 2100.0),
            PadVoiceArchetype::ReversePiano => (0.09, 0.04, 0.85, 2600.0),
            PadVoiceArchetype::AiryChoir => (0.15, 0.07, 1.30, 2200.0),
        };

        self.lfo_a = (self.lfo_a + lfo_a_hz / sample_rate).fract();
        self.lfo_b = (self.lfo_b + lfo_b_hz / sample_rate).fract();

        let eff_tonality = resolve_effective_tonality(cfg.tonality, quality);
        let (semis, weights) = voicing_semitones_and_weights(eff_tonality, cfg.register);
        let f0 = pitch_class_root_hz(pitch_class);

        let base_detune_l = [0.9971_f32, 1.0029, 0.9978, 1.0022];
        let base_detune_r = [1.0029_f32, 0.9971, 1.0022, 0.9978];

        // Subtle pitch vibrato for Strings / Organ rotary / Choir
        let vibrato = match cfg.archetype {
            PadVoiceArchetype::StringsCello => 1.0 + 0.0018 * (self.lfo_a * TAU).sin(),
            PadVoiceArchetype::OrganPad => 1.0 + 0.0012 * (self.lfo_a * TAU).sin(),
            _ => 1.0,
        };

        let mut raw_l = 0.0_f32;
        let mut raw_r = 0.0_f32;
        let mut shim_l = 0.0_f32;
        let mut shim_r = 0.0_f32;

        for idx in 0..4 {
            let freq = f0 * 2.0_f32.powf(semis[idx] / 12.0) * vibrato;
            let dl = 1.0 + (base_detune_l[idx] - 1.0) * detune_scale;
            let dr = 1.0 + (base_detune_r[idx] - 1.0) * detune_scale;

            let f_l = (freq * dl).min(sample_rate * 0.40);
            let f_r = (freq * dr).min(sample_rate * 0.40);

            self.phases_l[idx] = (self.phases_l[idx] + f_l / sample_rate).fract();
            self.phases_r[idx] = (self.phases_r[idx] + f_r / sample_rate).fract();

            let s_l = Self::synthesize_partial(cfg.archetype, self.phases_l[idx], self.lfo_b);
            let s_r = Self::synthesize_partial(cfg.archetype, self.phases_r[idx], self.lfo_b);

            raw_l += s_l * weights[idx];
            raw_r += s_r * weights[idx];

            // Octave-up shimmer tap
            let up_l = (self.phases_l[idx] * 2.0 * TAU).sin();
            let up_r = (self.phases_r[idx] * 2.0 * TAU).sin();
            shim_l += up_l * weights[idx];
            shim_r += up_r * weights[idx];
        }

        // Dynamic envelope / motion per archetype
        let env_mod = match cfg.archetype {
            PadVoiceArchetype::ReversePiano => {
                // Two overlapping slow-attack reverse-envelope grains (180 deg apart)
                self.grain_phase = (self.grain_phase + 0.31 / sample_rate).fract();
                let g1 = reverse_grain_env(self.grain_phase);
                let g2 = reverse_grain_env((self.grain_phase + 0.5).fract());
                0.45 + 0.65 * (g1 + g2) * 0.70
            }
            PadVoiceArchetype::GuitarSwell => {
                0.68 + 0.32 * (0.5 + 0.5 * (self.lfo_a * TAU).sin()).powf(1.6)
            }
            PadVoiceArchetype::EvolvingMotion => {
                0.72 + 0.28 * (self.lfo_a * TAU).sin() * (self.lfo_b * TAU).cos()
            }
            _ => 0.85 + 0.15 * (self.lfo_a * TAU).sin(),
        };

        raw_l *= env_mod * 0.55;
        raw_r *= env_mod * 0.55;
        shim_l *= env_mod * 0.35;
        shim_r *= env_mod * 0.35;

        // Per-layer gentle low-pass voicing filter
        let alpha = (TAU * cutoff_hz / (TAU * cutoff_hz + sample_rate)).clamp(0.001, 0.75);
        self.lp_l += alpha * (raw_l - self.lp_l);
        self.lp_r += alpha * (raw_r - self.lp_r);

        let g = self.smoothed_gain;
        ((self.lp_l * g, self.lp_r * g), (shim_l * g, shim_r * g))
    }
}

#[inline]
fn reverse_grain_env(phase: f32) -> f32 {
    // Slow exponential swell over 0.0..0.85, then smooth taper 0.85..1.0
    if phase < 0.85 {
        let t = phase / 0.85;
        t * t
    } else {
        let t = (1.0 - phase) / 0.15;
        t.clamp(0.0, 1.0)
    }
}

#[derive(Debug, Clone)]
struct PadRig {
    pitch_class: usize,
    quality: PadKeyQuality,
    configs: [PadLayerConfig; MAX_PAD_LAYERS],
    voices: [PadLayerVoice; MAX_PAD_LAYERS],
}

impl PadRig {
    fn new(
        pitch_class: usize,
        quality: PadKeyQuality,
        configs: [PadLayerConfig; MAX_PAD_LAYERS],
    ) -> Self {
        let voices = [
            PadLayerVoice::new(0, if configs[0].enabled { configs[0].gain } else { 0.0 }),
            PadLayerVoice::new(1, if configs[1].enabled { configs[1].gain } else { 0.0 }),
            PadLayerVoice::new(2, if configs[2].enabled { configs[2].gain } else { 0.0 }),
            PadLayerVoice::new(3, if configs[3].enabled { configs[3].gain } else { 0.0 }),
        ];
        Self {
            pitch_class: pitch_class % 12,
            quality,
            configs,
            voices,
        }
    }

    #[inline]
    fn next_sample(&mut self, sample_rate: f32) -> ((f32, f32), (f32, f32)) {
        let mut sum_l = 0.0_f32;
        let mut sum_r = 0.0_f32;
        let mut shim_l = 0.0_f32;
        let mut shim_r = 0.0_f32;
        let mut weight_sum = 0.0_f32;

        for idx in 0..MAX_PAD_LAYERS {
            let g = self.voices[idx].smoothed_gain;
            if self.configs[idx].enabled || g > 1e-5 {
                let ((l, r), (sl, sr)) = self.voices[idx].next_sample(
                    &self.configs[idx],
                    self.pitch_class,
                    self.quality,
                    sample_rate,
                );
                sum_l += l;
                sum_r += r;
                shim_l += sl;
                shim_r += sr;
                weight_sum += self.voices[idx].smoothed_gain;
            }
        }

        // Preserve consistent headroom as 1..4 layers are stacked
        let norm = 1.0 / weight_sum.max(1.0).sqrt();
        ((sum_l * norm, sum_r * norm), (shim_l * norm, shim_r * norm))
    }
}

/// Stereo 2-pole Chamberlin State-Variable Filter (2x oversampled for unconditional stability).
#[derive(Debug, Clone, Default)]
struct StereoSvfLowPass {
    low_l: f32,
    band_l: f32,
    low_r: f32,
    band_r: f32,
}

impl StereoSvfLowPass {
    #[inline]
    fn process(&mut self, in_l: f32, in_r: f32, cutoff_hz: f32, sample_rate: f32) -> (f32, f32) {
        let f = (2.0 * (PI * cutoff_hz / (sample_rate * 2.0)).sin()).clamp(0.001, 0.85);
        let damp = 1.4142_f32; // Butterworth Q = 0.707

        for _ in 0..2 {
            let hp_l = in_l - self.low_l - damp * self.band_l;
            self.band_l += f * hp_l;
            self.low_l += f * self.band_l;

            let hp_r = in_r - self.low_r - damp * self.band_r;
            self.band_r += f * hp_r;
            self.low_r += f * self.band_r;
        }
        (self.low_l, self.low_r)
    }
}

/// Pre-allocated 4-tap stereo diffuse shimmer tank (zero allocations during audio rendering).
#[derive(Debug, Clone)]
struct ShimmerTank {
    delays_l: [Vec<f32>; 2],
    delays_r: [Vec<f32>; 2],
    idx_l: [usize; 2],
    idx_r: [usize; 2],
    damp_l: f32,
    damp_r: f32,
}

impl ShimmerTank {
    fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8000.0);
        // Prime-proportioned delay lengths (~47ms, ~71ms, ~53ms, ~83ms)
        let len_l0 = ((0.047 * sr) as usize).max(64);
        let len_l1 = ((0.071 * sr) as usize).max(64);
        let len_r0 = ((0.053 * sr) as usize).max(64);
        let len_r1 = ((0.083 * sr) as usize).max(64);
        Self {
            delays_l: [vec![0.0; len_l0], vec![0.0; len_l1]],
            delays_r: [vec![0.0; len_r0], vec![0.0; len_r1]],
            idx_l: [0, 0],
            idx_r: [0, 0],
            damp_l: 0.0,
            damp_r: 0.0,
        }
    }

    fn resize(&mut self, sample_rate: f32) {
        *self = Self::new(sample_rate);
    }

    #[inline]
    fn process(&mut self, in_l: f32, in_r: f32, sample_rate: f32) -> (f32, f32) {
        let fb = 0.44_f32;
        let l0 = self.delays_l[0][self.idx_l[0]];
        let l1 = self.delays_l[1][self.idx_l[1]];
        let r0 = self.delays_r[0][self.idx_r[0]];
        let r1 = self.delays_r[1][self.idx_r[1]];

        // Cross-coupled feedback matrix for wide stereo shimmer tail
        let fb_l = 0.5 * (r0 - r1) * fb;
        let fb_r = 0.5 * (l0 + l1) * fb;

        self.delays_l[0][self.idx_l[0]] = (in_l * 0.55 + fb_l).tanh();
        self.delays_l[1][self.idx_l[1]] = (in_l * 0.45 - fb_l).tanh();
        self.delays_r[0][self.idx_r[0]] = (in_r * 0.55 + fb_r).tanh();
        self.delays_r[1][self.idx_r[1]] = (in_r * 0.45 - fb_r).tanh();

        self.idx_l[0] = (self.idx_l[0] + 1) % self.delays_l[0].len();
        self.idx_l[1] = (self.idx_l[1] + 1) % self.delays_l[1].len();
        self.idx_r[0] = (self.idx_r[0] + 1) % self.delays_r[0].len();
        self.idx_r[1] = (self.idx_r[1] + 1) % self.delays_r[1].len();

        let wet_l = 0.5 * (l0 + l1);
        let wet_r = 0.5 * (r0 + r1);

        // Gentle ~3.8 kHz damping on shimmer tail
        let alpha = (TAU * 3800.0 / (TAU * 3800.0 + sample_rate)).clamp(0.05, 0.75);
        self.damp_l += alpha * (wet_l - self.damp_l);
        self.damp_r += alpha * (wet_r - self.damp_r);
        (self.damp_l, self.damp_r)
    }
}

pub fn default_foundation_layers() -> [PadLayerConfig; MAX_PAD_LAYERS] {
    [
        PadLayerConfig {
            enabled: true,
            archetype: PadVoiceArchetype::WarmAnalog,
            register: PadRegister::Mid,
            tonality: PadTonality::Auto,
            gain: 0.85,
        },
        PadLayerConfig::default(),
        PadLayerConfig::default(),
        PadLayerConfig::default(),
    ]
}

fn normalize_layer_slice(layers: &[PadLayerConfig]) -> [PadLayerConfig; MAX_PAD_LAYERS] {
    let mut out = [
        PadLayerConfig::default(),
        PadLayerConfig::default(),
        PadLayerConfig::default(),
        PadLayerConfig::default(),
    ];
    for (idx, layer) in layers.iter().take(MAX_PAD_LAYERS).enumerate() {
        out[idx] = layer.clone().clamped();
    }
    out
}

/// Real-time stereo continuous 4-layer ambient drone pad synthesizer with equal-power crossfading.
#[derive(Debug, Clone)]
pub struct PadSynth {
    sample_rate: f32,
    pub enabled: bool,
    pub muted: bool,
    pub gain: f32,
    pub follow_song_key: bool,
    pub crossfade_sec: f32,
    pub brightness: f32,
    pub shimmer: f32,
    pub pan: f32,
    preset_id: Option<String>,
    preset_name: String,
    current_key: String,
    current_pc: usize,
    current_quality: PadKeyQuality,
    layers: [PadLayerConfig; MAX_PAD_LAYERS],
    rig_a: PadRig,
    rig_b: Option<PadRig>,
    crossfade_pos: usize,
    crossfade_total: usize,
    env_gain: f32,
    smoothed_gain: f32,
    smoothed_brightness: f32,
    smoothed_shimmer: f32,
    smoothed_pan: f32,
    svf: StereoSvfLowPass,
    shimmer_tank: ShimmerTank,
}

impl PadSynth {
    pub fn new(sample_rate: u32, initial_key: &str) -> Self {
        let (pc, quality) = parse_pad_key(initial_key).unwrap_or((0, PadKeyQuality::Major));
        let key_label = format_pad_key(pc, quality);
        let sr = sample_rate.max(8000) as f32;
        let crossfade_sec = 4.0_f32;
        let layers = default_foundation_layers();
        let brightness = 0.55_f32;
        let shimmer = 0.20_f32;
        let pan = 0.0_f32;
        let gain = 0.45_f32;

        Self {
            sample_rate: sr,
            enabled: false,
            muted: false,
            gain,
            follow_song_key: true,
            crossfade_sec,
            brightness,
            shimmer,
            pan,
            preset_id: Some("classic-foundation".to_string()),
            preset_name: "Classic Foundation".to_string(),
            current_key: key_label,
            current_pc: pc,
            current_quality: quality,
            layers: layers.clone(),
            rig_a: PadRig::new(pc, quality, layers),
            rig_b: None,
            crossfade_pos: 0,
            crossfade_total: (sr * crossfade_sec).max(1.0) as usize,
            env_gain: 0.0,
            smoothed_gain: gain,
            smoothed_brightness: brightness,
            smoothed_shimmer: shimmer,
            smoothed_pan: pan,
            svf: StereoSvfLowPass::default(),
            shimmer_tank: ShimmerTank::new(sr),
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.sample_rate = sample_rate.max(8000) as f32;
        self.crossfade_total = (self.sample_rate * self.crossfade_sec).max(1.0) as usize;
        self.shimmer_tank.resize(self.sample_rate);
    }

    pub fn set_crossfade_sec(&mut self, sec: f32) {
        self.crossfade_sec = if sec.is_finite() {
            sec.clamp(0.5, 12.0)
        } else {
            4.0
        };
        self.crossfade_total = (self.sample_rate * self.crossfade_sec).max(1.0) as usize;
    }

    pub fn set_brightness(&mut self, val: f32) {
        if val.is_finite() {
            self.brightness = val.clamp(0.0, 1.0);
        }
    }

    pub fn set_shimmer(&mut self, val: f32) {
        if val.is_finite() {
            self.shimmer = val.clamp(0.0, 1.0);
        }
    }

    pub fn set_pan(&mut self, val: f32) {
        if val.is_finite() {
            self.pan = val.clamp(-1.0, 1.0);
        }
    }

    pub fn current_key(&self) -> &str {
        &self.current_key
    }

    pub fn is_minor(&self) -> bool {
        self.current_quality == PadKeyQuality::Minor
    }

    pub fn preset_id(&self) -> Option<&str> {
        self.preset_id.as_deref()
    }

    pub fn preset_name(&self) -> &str {
        &self.preset_name
    }

    pub fn layers(&self) -> &[PadLayerConfig; MAX_PAD_LAYERS] {
        &self.layers
    }

    /// True if the pad should actively feed audio frames (either unmuted/enabled or finishing fade-out).
    pub fn is_active(&self) -> bool {
        (self.enabled && !self.muted) || self.env_gain > 1e-5
    }

    fn trigger_rig_crossfade(&mut self, next_pc: usize, next_quality: PadKeyQuality, next_layers: [PadLayerConfig; MAX_PAD_LAYERS]) {
        if !self.is_active() {
            self.rig_a = PadRig::new(next_pc, next_quality, next_layers);
            self.rig_b = None;
            self.crossfade_pos = 0;
            return;
        }
        if let Some(in_flight) = self.rig_b.take() {
            self.rig_a = in_flight;
        }
        self.rig_b = Some(PadRig::new(next_pc, next_quality, next_layers));
        self.crossfade_pos = 0;
        self.crossfade_total = (self.sample_rate * self.crossfade_sec).max(1.0) as usize;
    }

    pub fn set_key(&mut self, key: &str) {
        let Some((next_pc, next_quality)) = parse_pad_key(key) else {
            return;
        };
        let next_label = format_pad_key(next_pc, next_quality);
        if next_pc == self.current_pc && next_quality == self.current_quality && self.rig_b.is_none() {
            self.current_key = next_label;
            return;
        }

        self.current_pc = next_pc;
        self.current_quality = next_quality;
        self.current_key = next_label;
        self.trigger_rig_crossfade(next_pc, next_quality, self.layers.clone());
    }

    pub fn apply_patch(&mut self, patch: PadPatchSpec) {
        let next_layers = normalize_layer_slice(&patch.layers);
        let structural_change = next_layers
            .iter()
            .zip(self.layers.iter())
            .any(|(a, b)| {
                a.enabled != b.enabled
                    || a.archetype != b.archetype
                    || a.register != b.register
                    || a.tonality != b.tonality
            });

        self.preset_id = patch.preset_id;
        self.preset_name = if patch.preset_name.trim().is_empty() {
            "Custom Pad".to_string()
        } else {
            patch.preset_name
        };
        self.set_brightness(patch.brightness);
        self.set_shimmer(patch.shimmer);
        self.set_pan(patch.pan);
        if let Some(cf) = patch.crossfade_sec {
            self.set_crossfade_sec(cf);
        }
        self.layers = next_layers.clone();

        if structural_change {
            self.trigger_rig_crossfade(self.current_pc, self.current_quality, next_layers);
        } else {
            // Update gains smoothly in place
            for idx in 0..MAX_PAD_LAYERS {
                self.rig_a.configs[idx].gain = next_layers[idx].gain;
                if let Some(ref mut rb) = self.rig_b {
                    rb.configs[idx].gain = next_layers[idx].gain;
                }
            }
        }
    }

    pub fn set_layer(&mut self, index: usize, layer: Option<PadLayerConfig>) {
        if index >= MAX_PAD_LAYERS {
            return;
        }
        let next = layer.map(|l| l.clamped()).unwrap_or_default();
        let prev = &self.layers[index];
        let structural_change = prev.enabled != next.enabled
            || prev.archetype != next.archetype
            || prev.register != next.register
            || prev.tonality != next.tonality;

        self.layers[index] = next.clone();
        self.preset_id = None;
        self.preset_name = "Custom Mix".to_string();

        if structural_change {
            self.trigger_rig_crossfade(self.current_pc, self.current_quality, self.layers.clone());
        } else {
            self.rig_a.configs[index].gain = next.gain;
            if let Some(ref mut rb) = self.rig_b {
                rb.configs[index].gain = next.gain;
            }
        }
    }

    pub fn set_layer_gain(&mut self, index: usize, gain: f32) {
        if index >= MAX_PAD_LAYERS || !gain.is_finite() {
            return;
        }
        let clamped = gain.clamp(0.0, 1.0);
        self.layers[index].gain = clamped;
        self.rig_a.configs[index].gain = clamped;
        if let Some(ref mut rb) = self.rig_b {
            rb.configs[index].gain = clamped;
        }
    }

    #[inline]
    pub fn next_stereo_sample(&mut self) -> (f32, f32) {
        let target_env = if self.enabled && !self.muted { 1.0 } else { 0.0 };
        let ramp_step = 1.0 / (0.5 * self.sample_rate).max(1.0); // 500ms smooth ramp
        if (self.env_gain - target_env).abs() <= ramp_step {
            self.env_gain = target_env;
        } else if self.env_gain < target_env {
            self.env_gain += ramp_step;
        } else {
            self.env_gain -= ramp_step;
        }

        if self.env_gain <= 1e-6 && target_env == 0.0 {
            return (0.0, 0.0);
        }

        // Smooth continuous parameters (~30 ms one-pole)
        let smooth_alpha = 1.0 - (-1.0 / (0.03 * self.sample_rate)).exp();
        self.smoothed_gain += smooth_alpha * (self.gain - self.smoothed_gain);
        self.smoothed_brightness += smooth_alpha * (self.brightness - self.smoothed_brightness);
        self.smoothed_shimmer += smooth_alpha * (self.shimmer - self.smoothed_shimmer);
        self.smoothed_pan += smooth_alpha * (self.pan - self.smoothed_pan);

        let ((a_l, a_r), (sa_l, sa_r)) = self.rig_a.next_sample(self.sample_rate);
        let ((mix_l, mix_r), (shim_in_l, shim_in_r)) = if let Some(ref mut rb) = self.rig_b {
            let ((b_l, b_r), (sb_l, sb_r)) = rb.next_sample(self.sample_rate);
            let t = (self.crossfade_pos as f32 / self.crossfade_total as f32).clamp(0.0, 1.0);
            let angle = t * FRAC_PI_2;
            let gain_a = angle.cos();
            let gain_b = angle.sin();
            self.crossfade_pos += 1;
            let out_l = a_l * gain_a + b_l * gain_b;
            let out_r = a_r * gain_a + b_r * gain_b;
            let s_l = sa_l * gain_a + sb_l * gain_b;
            let s_r = sa_r * gain_a + sb_r * gain_b;
            if self.crossfade_pos >= self.crossfade_total {
                self.rig_a = self.rig_b.take().unwrap();
                self.crossfade_pos = 0;
            }
            ((out_l, out_r), (s_l, s_r))
        } else {
            ((a_l, a_r), (sa_l, sa_r))
        };

        // Brightness SVF low-pass: exponential sweep 260 Hz .. 4800 Hz
        let cutoff_hz = 260.0 * (4800.0_f32 / 260.0_f32).powf(self.smoothed_brightness.clamp(0.0, 1.0));
        let (flt_l, flt_r) = self.svf.process(mix_l, mix_r, cutoff_hz, self.sample_rate);

        // Diffuse Shimmer Tank (fed even softly so tank stays warm, mixed by smoothed_shimmer)
        let (wet_l, wet_r) = self.shimmer_tank.process(shim_in_l, shim_in_r, self.sample_rate);
        let shim_amt = self.smoothed_shimmer.clamp(0.0, 1.0) * 0.65;
        let toned_l = flt_l + wet_l * shim_amt;
        let toned_r = flt_r + wet_r * shim_amt;

        // Constant-power stereo pan (-1.0 .. +1.0, 0.0 -> equal 0.7071 * sqrt(2) = 1.0 unity at center)
        let pan_theta = (self.smoothed_pan.clamp(-1.0, 1.0) + 1.0) * FRAC_PI_4;
        let pan_gain_l = pan_theta.cos() * std::f32::consts::SQRT_2;
        let pan_gain_r = pan_theta.sin() * std::f32::consts::SQRT_2;

        let amp = self.env_gain * self.smoothed_gain;
        let out_l = (toned_l * pan_gain_l * amp).tanh();
        let out_r = (toned_r * pan_gain_r * amp).tanh();
        (out_l, out_r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pad_root_frequency_table() {
        let a3 = pitch_class_root_hz(key_to_pitch_class("A").unwrap());
        assert!((a3 - 220.0).abs() < 0.01);

        let c3 = pitch_class_root_hz(key_to_pitch_class("C").unwrap());
        assert!((c3 - 130.8128).abs() < 0.05);

        assert_eq!(key_to_pitch_class("C#"), key_to_pitch_class("Db"));
        assert_eq!(key_to_pitch_class("F#m"), key_to_pitch_class("Gb"));
        assert_eq!(key_to_pitch_class("Am"), key_to_pitch_class("A"));
        assert_eq!(normalize_key_label("Dbm"), "C#m");
    }

    #[test]
    fn test_pad_key_quality_and_transposition() {
        assert_eq!(parse_pad_key("Am"), Some((9, PadKeyQuality::Minor)));
        assert_eq!(parse_pad_key("F#m"), Some((6, PadKeyQuality::Minor)));
        assert_eq!(parse_pad_key("Bb minor"), Some((10, PadKeyQuality::Minor)));
        assert_eq!(parse_pad_key("G"), Some((7, PadKeyQuality::Major)));
        assert_eq!(parse_pad_key("C# major"), Some((1, PadKeyQuality::Major)));
        assert_eq!(transpose_pad_key("Am", 2), "Bm");
        assert_eq!(transpose_pad_key("G", -2), "F");
    }

    #[test]
    fn test_pad_equal_power_crossfade() {
        let sr = 48_000_u32;
        let mut pad = PadSynth::new(sr, "C");
        pad.enabled = true;
        pad.gain = 0.8;
        pad.set_crossfade_sec(2.0);

        for _ in 0..(sr * 6 / 10) {
            pad.next_stereo_sample();
        }
        let mut pre_sq = 0.0_f64;
        let n = 4800;
        for _ in 0..n {
            let (l, r) = pad.next_stereo_sample();
            pre_sq += (l * l + r * r) as f64;
        }
        let pre_rms = (pre_sq / (n * 2) as f64).sqrt();

        pad.set_key("G");

        for _ in 0..(sr - n / 2) {
            let (l, r) = pad.next_stereo_sample();
            assert!(l.abs() <= 1.0 && r.abs() <= 1.0);
        }

        let mut mid_sq = 0.0_f64;
        for _ in 0..n {
            let (l, r) = pad.next_stereo_sample();
            mid_sq += (l * l + r * r) as f64;
        }
        let mid_rms = (mid_sq / (n * 2) as f64).sqrt();

        assert!(pre_rms > 0.02);
        assert!(mid_rms > 0.02);
        let ratio_db = 20.0 * (mid_rms / pre_rms).log10();
        assert!(
            ratio_db.abs() < 3.5,
            "Midpoint crossfade RMS shifted unexpectedly: {ratio_db:.2} dB"
        );
    }

    #[test]
    fn test_pad_preset_and_layer_crossfade_across_sample_rates() {
        for sr in [44_100_u32, 48_000, 96_000] {
            let mut pad = PadSynth::new(sr, "Am");
            pad.enabled = true;
            pad.gain = 0.85;
            pad.set_crossfade_sec(1.0);
            assert!(pad.is_minor());

            for _ in 0..(sr * 6 / 10) {
                pad.next_stereo_sample();
            }

            pad.apply_patch(PadPatchSpec {
                preset_id: Some("reverse-piano".into()),
                preset_name: "Reverse Piano".into(),
                layers: vec![
                    PadLayerConfig {
                        enabled: true,
                        archetype: PadVoiceArchetype::ReversePiano,
                        register: PadRegister::High,
                        tonality: PadTonality::Minor,
                        gain: 0.9,
                    },
                    PadLayerConfig {
                        enabled: true,
                        archetype: PadVoiceArchetype::StringsCello,
                        register: PadRegister::Low,
                        tonality: PadTonality::Auto,
                        gain: 0.7,
                    },
                ],
                brightness: 0.75,
                shimmer: 0.65,
                pan: -0.25,
                crossfade_sec: Some(1.0),
            });

            let mut rms_acc = 0.0_f64;
            for _ in 0..sr {
                let (l, r) = pad.next_stereo_sample();
                assert!(l.is_finite() && r.is_finite());
                assert!(l.abs() <= 1.0 && r.abs() <= 1.0);
                rms_acc += (l * l + r * r) as f64;
            }
            let rms = (rms_acc / (sr * 2) as f64).sqrt();
            assert!(rms > 0.02, "Unexpected silence at sr={sr}");
        }
    }

    #[test]
    fn test_pad_minor_vs_major_spectrum() {
        // Measure DFT energy at A3 + 3 semitones (C4 = 261.63 Hz) vs A3 + 4 semitones (C#4 = 277.18 Hz)
        fn goertzel_mag(samples: &[f32], target_hz: f32, sr: f32) -> f64 {
            let omega = 2.0 * std::f64::consts::PI * (target_hz as f64) / (sr as f64);
            let coeff = 2.0 * omega.cos();
            let mut q1 = 0.0_f64;
            let mut q2 = 0.0_f64;
            for &x in samples {
                let q0 = coeff * q1 - q2 + (x as f64);
                q2 = q1;
                q1 = q0;
            }
            (q1 * q1 + q2 * q2 - q1 * q2 * coeff).sqrt()
        }

        let sr = 48_000_u32;
        let mut minor_pad = PadSynth::new(sr, "Am");
        minor_pad.enabled = true;
        minor_pad.gain = 0.9;
        minor_pad.set_brightness(0.95);
        minor_pad.set_shimmer(0.0);

        for _ in 0..(sr * 6 / 10) {
            minor_pad.next_stereo_sample();
        }
        let mut buf = Vec::with_capacity(sr as usize);
        for _ in 0..sr {
            let (l, r) = minor_pad.next_stereo_sample();
            buf.push((l + r) * 0.5);
        }

        let c4_hz = 220.0 * 2.0_f32.powf(3.0 / 12.0);
        let cs4_hz = 220.0 * 2.0_f32.powf(4.0 / 12.0);
        let mag_minor_3rd = goertzel_mag(&buf, c4_hz, sr as f32);
        let mag_major_3rd = goertzel_mag(&buf, cs4_hz, sr as f32);
        assert!(
            mag_minor_3rd > mag_major_3rd * 3.0,
            "Expected strong minor 3rd in Am: minor={mag_minor_3rd}, major={mag_major_3rd}"
        );
    }

    #[test]
    fn test_pad_brightness_shimmer_and_pan_bounds() {
        let sr = 48_000_u32;
        let mut pad = PadSynth::new(sr, "G");
        pad.enabled = true;
        pad.gain = 0.9;
        pad.apply_patch(PadPatchSpec {
            preset_id: None,
            preset_name: "Bright Test".into(),
            layers: vec![PadLayerConfig {
                enabled: true,
                archetype: PadVoiceArchetype::BrightSynth,
                register: PadRegister::High,
                tonality: PadTonality::OpenFifth,
                gain: 0.9,
            }],
            brightness: 0.0,
            shimmer: 0.0,
            pan: -0.9,
            crossfade_sec: None,
        });

        for _ in 0..sr {
            pad.next_stereo_sample();
        }
        let mut left_sq = 0.0_f64;
        let mut right_sq = 0.0_f64;
        for _ in 0..4800 {
            let (l, r) = pad.next_stereo_sample();
            left_sq += (l * l) as f64;
            right_sq += (r * r) as f64;
        }
        assert!(left_sq > right_sq * 4.0, "Hard left pan should dominate L channel");

        // Increase brightness & shimmer and verify finite bounded output
        pad.set_pan(0.0);
        pad.set_brightness(1.0);
        pad.set_shimmer(1.0);
        for _ in 0..sr {
            let (l, r) = pad.next_stereo_sample();
            assert!(l.is_finite() && r.is_finite());
            assert!(l.abs() <= 1.0 && r.abs() <= 1.0);
        }
    }

    #[test]
    fn test_pad_mute_decay() {
        let sr = 48_000_u32;
        let mut pad = PadSynth::new(sr, "D");
        pad.enabled = true;
        pad.gain = 0.8;

        for _ in 0..sr {
            pad.next_stereo_sample();
        }
        assert!(pad.is_active());

        pad.muted = true;
        let first = pad.next_stereo_sample();
        assert!(
            first.0.abs() > 1e-4 || first.1.abs() > 1e-4,
            "Mute should ramp down rather than zeroing on sample 1"
        );

        for _ in 0..(sr * 6 / 10) {
            pad.next_stereo_sample();
        }
        assert!(!pad.is_active());
        assert_eq!(pad.next_stereo_sample(), (0.0, 0.0));
    }
}
