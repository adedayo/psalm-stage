use std::f64::consts::TAU;
use std::fs::File;
use std::path::Path;

use serde::{Deserialize, Serialize};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::loader::{classify_stem_filename, discover_audio_files};
use crate::manifest::SongManifest;
use crate::pad::CHROMATIC_KEYS;
use crate::{AudioCoreError, Result};

/// Waveform peak summary and tempo/downbeat/key analysis for an audio file or song mixdown.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioAnalysisResult {
    pub source_file: String,
    pub duration_sec: f64,
    pub sample_rate: u32,
    pub channels: u16,
    pub estimated_bpm: f64,
    pub estimated_first_downbeat_sec: f64,
    #[serde(default)]
    pub estimated_key: Option<String>,
    #[serde(default)]
    pub key_confidence: f64,
    #[serde(default)]
    pub key_candidates: Vec<String>,
    /// Downsampled `[min, max]` amplitude buckets normalized to `[-1.0, 1.0]`.
    pub peaks: Vec<[f32; 2]>,
}

const CHROMA_FFT_LEN: usize = 8192;
const CHROMA_FFT_HOP: usize = 4096;
const CHROMA_FLUX_FFT_LEN: usize = 2048;
const CHROMA_FLUX_FFT_HOP: usize = 512;
const ONSET_FFT_LEN: usize = 1024;
const ONSET_BANDS: usize = 28;

struct StemEnvelopeRaw {
    source_file: String,
    duration_sec: f64,
    sample_rate: u32,
    channels: u16,
    onset_hz: f64,
    fine_peaks: Vec<[f32; 2]>,
    low_flux: Vec<f32>,
    full_flux: Vec<f32>,
    full_rms: Vec<f32>,
    hpcp_full_36: [f64; 36],
    hpcp_bass_36: [f64; 36],
    tuning_hist: [f64; 100],
    tonal_frames: usize,
    chroma_timeline: Vec<[f32; 12]>,
    chroma_hz: f64,
    chroma_offset_sec: f64,
    peak_amp: f32,
}

fn fft_radix2(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && im.len() == n);

    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2usize;
    while len <= n {
        let half = len >> 1;
        let angle = -TAU / len as f64;
        let wlen_re = angle.cos();
        let wlen_im = angle.sin();
        let mut start = 0usize;
        while start < n {
            let mut w_re = 1.0_f64;
            let mut w_im = 0.0_f64;
            for k in 0..half {
                let even = start + k;
                let odd = even + half;
                let t_re = w_re * re[odd] - w_im * im[odd];
                let t_im = w_re * im[odd] + w_im * re[odd];
                re[odd] = re[even] - t_re;
                im[odd] = im[even] - t_im;
                re[even] += t_re;
                im[even] += t_im;
                let next_re = w_re * wlen_re - w_im * wlen_im;
                w_im = w_re * wlen_im + w_im * wlen_re;
                w_re = next_re;
            }
            start += len;
        }
        len <<= 1;
    }
}

struct ChromaExtractor {
    decim_factor: usize,
    eff_sr: f64,
    decim_sum: f64,
    decim_count: usize,
    ring_buf: Vec<f64>,
    ring_filled: usize,
    ring_pos: usize,
    since_hop: usize,
    hann: Vec<f64>,
    re: Vec<f64>,
    im: Vec<f64>,
    pub hpcp_full_36: [f64; 36],
    pub hpcp_bass_36: [f64; 36],
    pub tuning_hist: [f64; 100],
    pub tonal_frames: usize,
    fast_ring: Vec<f64>,
    fast_filled: usize,
    fast_pos: usize,
    fast_since_hop: usize,
    fast_hann: Vec<f64>,
    fast_re: Vec<f64>,
    fast_im: Vec<f64>,
    pub chroma_timeline: Vec<[f32; 12]>,
}

impl ChromaExtractor {
    fn new(sample_rate: u32) -> Self {
        let decim_factor = (sample_rate as usize / 11_025).max(1);
        let eff_sr = sample_rate.max(1) as f64 / decim_factor as f64;
        let hann: Vec<f64> = (0..CHROMA_FFT_LEN)
            .map(|i| 0.5 * (1.0 - (TAU * i as f64 / CHROMA_FFT_LEN as f64).cos()))
            .collect();
        let fast_hann: Vec<f64> = (0..CHROMA_FLUX_FFT_LEN)
            .map(|i| 0.5 * (1.0 - (TAU * i as f64 / CHROMA_FLUX_FFT_LEN as f64).cos()))
            .collect();
        Self {
            decim_factor,
            eff_sr,
            decim_sum: 0.0,
            decim_count: 0,
            ring_buf: vec![0.0; CHROMA_FFT_LEN],
            ring_filled: 0,
            ring_pos: 0,
            since_hop: 0,
            hann,
            re: vec![0.0; CHROMA_FFT_LEN],
            im: vec![0.0; CHROMA_FFT_LEN],
            hpcp_full_36: [0.0; 36],
            hpcp_bass_36: [0.0; 36],
            tuning_hist: [0.0; 100],
            tonal_frames: 0,
            fast_ring: vec![0.0; CHROMA_FLUX_FFT_LEN],
            fast_filled: 0,
            fast_pos: 0,
            fast_since_hop: 0,
            fast_hann,
            fast_re: vec![0.0; CHROMA_FLUX_FFT_LEN],
            fast_im: vec![0.0; CHROMA_FLUX_FFT_LEN],
            chroma_timeline: Vec::new(),
        }
    }

    fn fast_chroma_hz(&self) -> f64 {
        self.eff_sr / CHROMA_FLUX_FFT_HOP as f64
    }

    fn fast_chroma_center_offset_sec(&self) -> f64 {
        (CHROMA_FLUX_FFT_LEN as f64 * 0.5) / self.eff_sr.max(1.0)
    }

    #[inline]
    fn push_sample(&mut self, mono: f32) {
        self.decim_sum += mono as f64;
        self.decim_count += 1;
        if self.decim_count < self.decim_factor {
            return;
        }
        let s = self.decim_sum / self.decim_count as f64;
        self.decim_sum = 0.0;
        self.decim_count = 0;

        self.fast_ring[self.fast_pos] = s;
        self.fast_pos = (self.fast_pos + 1) % CHROMA_FLUX_FFT_LEN;
        if self.fast_filled < CHROMA_FLUX_FFT_LEN {
            self.fast_filled += 1;
            if self.fast_filled == CHROMA_FLUX_FFT_LEN {
                self.process_fast_frame();
                self.fast_since_hop = 0;
            }
        } else {
            self.fast_since_hop += 1;
            if self.fast_since_hop >= CHROMA_FLUX_FFT_HOP {
                self.process_fast_frame();
                self.fast_since_hop = 0;
            }
        }

        self.ring_buf[self.ring_pos] = s;
        self.ring_pos = (self.ring_pos + 1) % CHROMA_FFT_LEN;
        if self.ring_filled < CHROMA_FFT_LEN {
            self.ring_filled += 1;
            if self.ring_filled == CHROMA_FFT_LEN {
                self.process_frame();
                self.since_hop = 0;
            }
        } else {
            self.since_hop += 1;
            if self.since_hop >= CHROMA_FFT_HOP {
                self.process_frame();
                self.since_hop = 0;
            }
        }
    }

    fn process_fast_frame(&mut self) {
        let mut sq_sum = 0.0_f64;
        for i in 0..CHROMA_FLUX_FFT_LEN {
            let sample = self.fast_ring[(self.fast_pos + i) % CHROMA_FLUX_FFT_LEN];
            sq_sum += sample * sample;
            self.fast_re[i] = sample * self.fast_hann[i];
            self.fast_im[i] = 0.0;
        }
        let rms = (sq_sum / CHROMA_FLUX_FFT_LEN as f64).sqrt();
        if rms < 0.003 {
            self.chroma_timeline.push([0.0; 12]);
            return;
        }
        fft_radix2(&mut self.fast_re, &mut self.fast_im);

        let nyquist_bins = CHROMA_FLUX_FFT_LEN / 2;
        let mut vec = [0.0_f64; 12];
        for midi in 36i32..=83i32 {
            let freq = 440.0 * 2.0_f64.powf((midi - 69) as f64 / 12.0);
            let bin_f = freq * CHROMA_FLUX_FFT_LEN as f64 / self.eff_sr;
            let lo = bin_f.floor() as usize;
            let hi = lo + 1;
            if lo < 1 || hi >= nyquist_bins {
                continue;
            }
            let frac = bin_f - lo as f64;
            let mag_lo = (self.fast_re[lo] * self.fast_re[lo] + self.fast_im[lo] * self.fast_im[lo]).sqrt();
            let mag_hi = (self.fast_re[hi] * self.fast_re[hi] + self.fast_im[hi] * self.fast_im[hi]).sqrt();
            let mag = (mag_lo * (1.0 - frac) + mag_hi * frac).powf(0.8);
            let pc = (midi.rem_euclid(12)) as usize;
            vec[pc] += mag;
        }

        let norm = vec.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm <= 1e-9 {
            self.chroma_timeline.push([0.0; 12]);
            return;
        }
        let amp = rms.sqrt().min(0.8) as f32;
        let mut out = [0.0_f32; 12];
        for pc in 0..12 {
            out[pc] = ((vec[pc] / norm) as f32) * amp;
        }
        self.chroma_timeline.push(out);
    }

    fn process_frame(&mut self) {
        let mut sq_sum = 0.0_f64;
        for i in 0..CHROMA_FFT_LEN {
            let sample = self.ring_buf[(self.ring_pos + i) % CHROMA_FFT_LEN];
            sq_sum += sample * sample;
            self.re[i] = sample * self.hann[i];
            self.im[i] = 0.0;
        }
        let rms = (sq_sum / CHROMA_FFT_LEN as f64).sqrt();
        if rms < 0.004 {
            return;
        }
        fft_radix2(&mut self.re, &mut self.im);

        let nyquist_bins = CHROMA_FFT_LEN / 2;
        let min_freq = 39.0_f64; // ~E1
        let max_freq = 2150.0_f64; // ~C7
        let min_bin = ((min_freq * CHROMA_FFT_LEN as f64 / self.eff_sr).floor() as usize).max(2);
        let max_bin = ((max_freq * CHROMA_FFT_LEN as f64 / self.eff_sr).ceil() as usize)
            .min(nyquist_bins.saturating_sub(2));
        if max_bin <= min_bin + 4 {
            return;
        }

        // Magnitude spectrum across tonal range + spectral whitening (subtracts broadband drum/noise floor)
        let span_len = max_bin + 2;
        let mut mag = vec![0.0_f64; span_len];
        for b in min_bin.saturating_sub(1)..=max_bin + 1 {
            mag[b] = (self.re[b] * self.re[b] + self.im[b] * self.im[b]).sqrt();
        }

        let mut full_hpcp = [0.0_f64; 36];
        let mut bass_hpcp = [0.0_f64; 36];
        for b in min_bin..=max_bin {
            let m = mag[b];
            if m <= 1e-8 {
                continue;
            }
            let bg_lo = b.saturating_sub(10).max(min_bin.saturating_sub(1));
            let bg_hi = (b + 10).min(max_bin + 1);
            let bg_mean = mag[bg_lo..=bg_hi].iter().sum::<f64>() / (bg_hi - bg_lo + 1) as f64;
            let whitened = (m - 0.55 * bg_mean).max(0.0);
            if whitened <= 1e-8 {
                continue;
            }

            let is_peak = m >= mag[b - 1] && m >= mag[b + 1];
            let peak_weight = if is_peak { 1.0 } else { 0.22 };

            // Quadratic frequency refinement when bin is a local spectral peak
            let bin_f = if is_peak {
                let denom = mag[b - 1] - 2.0 * m + mag[b + 1];
                if denom.abs() > 1e-12 {
                    b as f64 + (0.5 * (mag[b - 1] - mag[b + 1]) / denom).clamp(-0.5, 0.5)
                } else {
                    b as f64
                }
            } else {
                b as f64
            };
            let freq = bin_f * self.eff_sr / CHROMA_FFT_LEN as f64;
            let midi_exact = 69.0 + 12.0 * (freq / 440.0).log2();
            if !(28.0..=95.0).contains(&midi_exact) {
                continue;
            }

            let compressed = whitened.powf(0.75) * peak_weight;
            // Accumulate cents deviation histogram around A4=440Hz for tuning compensation
            if is_peak && whitened > bg_mean * 0.35 {
                let cents = (midi_exact - midi_exact.round()) * 100.0; // -50..+50
                let c_bin = ((cents + 50.0).floor() as isize).rem_euclid(100) as usize;
                self.tuning_hist[c_bin] += compressed;
            }

            // Map into 36-bin HPCP (3 sub-bins per semitone) with linear sub-bin split
            let hpcp_f = (midi_exact * 3.0).rem_euclid(36.0);
            let lo_idx = hpcp_f.floor() as usize % 36;
            let hi_idx = (lo_idx + 1) % 36;
            let frac = hpcp_f - hpcp_f.floor();
            if midi_exact >= 36.0 {
                full_hpcp[lo_idx] += compressed * (1.0 - frac);
                full_hpcp[hi_idx] += compressed * frac;
            }
            if midi_exact <= 53.5 {
                bass_hpcp[lo_idx] += compressed * (1.0 - frac);
                bass_hpcp[hi_idx] += compressed * frac;
            }
        }

        let norm = full_hpcp.iter().map(|v| v * v).sum::<f64>().sqrt();
        if norm <= 1e-9 {
            return;
        }
        let weight = rms.sqrt().min(0.8);
        for i in 0..36 {
            self.hpcp_full_36[i] += (full_hpcp[i] / norm) * weight;
        }
        let bass_norm = bass_hpcp.iter().map(|v| v * v).sum::<f64>().sqrt();
        if bass_norm > 1e-9 {
            for i in 0..36 {
                self.hpcp_bass_36[i] += (bass_hpcp[i] / bass_norm) * weight;
            }
        }
        self.tonal_frames += 1;
    }
}

/// Multi-band SuperFlux onset detector (~100 Hz hop, 28 log-mel bands) separating kick/bass
/// (`35-180 Hz`) from full-spectrum note/chord/snare transients while rejecting vibrato ripples.
struct OnsetExtractor {
    decim_factor: usize,
    hop_eff: usize,
    onset_hz: f64,
    decim_sum: f64,
    decim_count: usize,
    hop_sq_sum: f64,
    hop_sq_count: usize,
    ring: [f64; ONSET_FFT_LEN],
    ring_pos: usize,
    since_hop: usize,
    first_frame_done: bool,
    window: [f64; ONSET_FFT_LEN],
    re: [f64; ONSET_FFT_LEN],
    im: [f64; ONSET_FFT_LEN],
    band_filters: Vec<Vec<(usize, f64)>>,
    band_is_low: [bool; ONSET_BANDS],
    band_weights: [f64; ONSET_BANDS],
    prev1: [f64; ONSET_BANDS],
    prev2: [f64; ONSET_BANDS],
    has_prev: bool,
    low_flux: Vec<f32>,
    full_flux: Vec<f32>,
    full_rms: Vec<f32>,
}

impl OnsetExtractor {
    fn new(sample_rate: u32) -> Self {
        let sr = sample_rate.max(8_000) as f64;
        // Decimate to ~22-24 kHz for ~22 Hz STFT bin spacing at fast 1024-pt transforms
        let decim = (sr / 22_050.0).round().max(1.0) as usize;
        let eff_sr = sr / decim as f64;
        let hop_eff = (eff_sr / 100.0).round().max(1.0) as usize;
        let onset_hz = eff_sr / hop_eff as f64;

        let mut window = [0.0_f64; ONSET_FFT_LEN];
        for i in 0..ONSET_FFT_LEN {
            window[i] = 0.5 - 0.5 * (TAU * i as f64 / ONSET_FFT_LEN as f64).cos();
        }

        let hz_to_mel = |hz: f64| -> f64 { 2595.0 * (1.0 + hz / 700.0).log10() };
        let mel_to_hz = |mel: f64| -> f64 { 700.0 * (10.0_f64.powf(mel / 2595.0) - 1.0) };

        let mel_lo = hz_to_mel(35.0);
        let mel_hi = hz_to_mel((eff_sr * 0.46).min(9_500.0));
        let mut centers_hz = [0.0_f64; ONSET_BANDS + 2];
        for i in 0..ONSET_BANDS + 2 {
            let m = mel_lo + (mel_hi - mel_lo) * (i as f64 / (ONSET_BANDS + 1) as f64);
            centers_hz[i] = mel_to_hz(m);
        }

        let max_bin = ONSET_FFT_LEN / 2;
        let bin_hz = eff_sr / ONSET_FFT_LEN as f64;
        let mut band_filters = Vec::with_capacity(ONSET_BANDS);
        let mut band_is_low = [false; ONSET_BANDS];
        let mut band_weights = [1.0_f64; ONSET_BANDS];

        for b in 0..ONSET_BANDS {
            let f_lo = centers_hz[b];
            let f_mid = centers_hz[b + 1];
            let f_hi = centers_hz[b + 2];
            band_is_low[b] = f_mid <= 185.0;
            // Boost sub/kick bands slightly (fewer mel bins per Hz) and attenuate >2.6kHz hat shimmer
            band_weights[b] = if f_mid <= 185.0 {
                1.45
            } else if f_mid <= 2_600.0 {
                1.0
            } else if f_mid <= 4_000.0 {
                0.45
            } else {
                0.14
            };

            let b_start = (f_lo / bin_hz).floor().max(1.0) as usize;
            let b_end = (f_hi / bin_hz).ceil().min(max_bin as f64) as usize;
            let mut weights = Vec::new();
            let mut w_sum = 0.0_f64;
            for bin in b_start..=b_end {
                let freq = bin as f64 * bin_hz;
                let w = if freq <= f_mid {
                    (freq - f_lo) / (f_mid - f_lo).max(1.0)
                } else {
                    (f_hi - freq) / (f_hi - f_mid).max(1.0)
                };
                if w > 0.0 {
                    weights.push((bin, w));
                    w_sum += w;
                }
            }
            if w_sum > 0.0 {
                for (_, w) in &mut weights {
                    *w /= w_sum;
                }
            } else {
                let nearest = ((f_mid / bin_hz).round() as usize).clamp(1, max_bin);
                weights.push((nearest, 1.0));
            }
            band_filters.push(weights);
        }

        // Pre-fill ring pointer at half-window so frame k=0 centers cleanly near t=0.0s
        Self {
            decim_factor: decim,
            hop_eff,
            onset_hz,
            decim_sum: 0.0,
            decim_count: 0,
            hop_sq_sum: 0.0,
            hop_sq_count: 0,
            ring: [0.0; ONSET_FFT_LEN],
            ring_pos: ONSET_FFT_LEN / 2,
            since_hop: 0,
            first_frame_done: false,
            window,
            re: [0.0; ONSET_FFT_LEN],
            im: [0.0; ONSET_FFT_LEN],
            band_filters,
            band_is_low,
            band_weights,
            prev1: [0.0; ONSET_BANDS],
            prev2: [0.0; ONSET_BANDS],
            has_prev: false,
            low_flux: Vec::new(),
            full_flux: Vec::new(),
            full_rms: Vec::new(),
        }
    }

    fn push_sample(&mut self, mono: f32) {
        let m = mono as f64;
        self.hop_sq_sum += m * m;
        self.hop_sq_count += 1;

        self.decim_sum += m;
        self.decim_count += 1;
        if self.decim_count < self.decim_factor {
            return;
        }
        let s = self.decim_sum / self.decim_count as f64;
        self.decim_sum = 0.0;
        self.decim_count = 0;

        self.ring[self.ring_pos] = s;
        self.ring_pos = (self.ring_pos + 1) % ONSET_FFT_LEN;
        self.since_hop += 1;

        let trigger = if !self.first_frame_done {
            self.since_hop >= ONSET_FFT_LEN / 2
        } else {
            self.since_hop >= self.hop_eff
        };
        if trigger {
            self.process_frame();
            self.first_frame_done = true;
            self.since_hop = 0;
        }
    }

    fn process_frame(&mut self) {
        let rms = if self.hop_sq_count > 0 {
            (self.hop_sq_sum / self.hop_sq_count as f64).sqrt() as f32
        } else {
            0.0
        };
        self.hop_sq_sum = 0.0;
        self.hop_sq_count = 0;

        for i in 0..ONSET_FFT_LEN {
            let idx = (self.ring_pos + i) % ONSET_FFT_LEN;
            self.re[i] = self.ring[idx] * self.window[i];
            self.im[i] = 0.0;
        }
        fft_radix2(&mut self.re, &mut self.im);

        let inv_n = 2.0 / ONSET_FFT_LEN as f64;
        let mut cur = [0.0_f64; ONSET_BANDS];
        for b in 0..ONSET_BANDS {
            let mut energy = 0.0_f64;
            for &(bin, w) in &self.band_filters[b] {
                let mag = (self.re[bin] * self.re[bin] + self.im[bin] * self.im[bin]).sqrt() * inv_n;
                energy += w * mag;
            }
            cur[b] = (1.0 + 160.0 * energy).ln();
        }

        let (low_sf, full_sf) = if !self.has_prev {
            self.has_prev = true;
            (0.0_f32, 0.0_f32)
        } else {
            let mut low_acc = 0.0_f64;
            let mut low_cnt = 0.0_f64;
            let mut full_acc = 0.0_f64;
            let mut full_w = 0.0_f64;
            for b in 0..ONSET_BANDS {
                let nb_lo = b.saturating_sub(1);
                let nb_hi = (b + 1).min(ONSET_BANDS - 1);
                let mut local_ref = 0.0_f64;
                for nb in nb_lo..=nb_hi {
                    local_ref = local_ref.max(self.prev1[nb]).max(self.prev2[nb]);
                }
                let diff = (cur[b] - local_ref).max(0.0);
                if self.band_is_low[b] {
                    low_acc += diff;
                    low_cnt += 1.0;
                }
                let bw = self.band_weights[b];
                full_acc += bw * diff;
                full_w += bw;
            }
            (
                (low_acc / low_cnt.max(1.0)) as f32,
                (full_acc / full_w.max(1.0)) as f32,
            )
        };

        self.prev2 = self.prev1;
        self.prev1 = cur;

        self.low_flux.push(low_sf);
        self.full_flux.push(full_sf);
        self.full_rms.push(rms);
    }

    fn finish(&mut self) {
        if self.hop_sq_count > 0 && self.since_hop > self.hop_eff / 2 {
            self.process_frame();
        }
    }
}

fn extract_stem_envelopes(path: &Path, extract_chroma: bool) -> Result<StemEnvelopeRaw> {
    let file = File::open(path).map_err(|e| {
        AudioCoreError::Decode(format!("Failed to open audio file {}: {e}", path.display()))
    })?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| {
            AudioCoreError::Decode(format!("Unsupported audio format {}: {e}", path.display()))
        })?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| AudioCoreError::Decode(format!("No audio track in {}", path.display())))?;

    let codec_params = track.codec_params.clone();
    let track_id = track.id;
    let sample_rate = codec_params.sample_rate.unwrap_or(48_000);
    let channels = codec_params.channels.map(|c| c.count()).unwrap_or(2) as u16;

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| AudioCoreError::Decode(format!("Codec init failed: {e}")))?;

    // Collect fine-grained 20 ms blocks (50 Hz) for waveform re-bucketing,
    // plus ~100 Hz STFT multi-band SuperFlux onset envelopes for rock-solid BPM & beat alignment.
    let fine_block_frames = (sample_rate as usize / 50).max(1);

    let mut fine_peaks: Vec<[f32; 2]> = Vec::new();
    let mut cur_min = 0.0_f32;
    let mut cur_max = 0.0_f32;
    let mut cur_fine_count = 0usize;

    let mut peak_amp = 0.0_f32;

    let mut chroma = extract_chroma.then(|| ChromaExtractor::new(sample_rate));
    let mut onset = OnsetExtractor::new(sample_rate);

    let mut total_frames = 0usize;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(SymphoniaError::ResetRequired) => {
                decoder.reset();
                continue;
            }
            Err(e) => return Err(AudioCoreError::Decode(e.to_string())),
        };

        if packet.track_id() != track_id {
            continue;
        }

        let audio_buf = match decoder.decode(&packet) {
            Ok(b) => b,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(AudioCoreError::Decode(e.to_string())),
        };

        let spec = *audio_buf.spec();
        let frames = audio_buf.frames();
        let ch = spec.channels.count().max(1);
        let buf = sample_buf.get_or_insert_with(|| {
            SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec)
        });
        buf.copy_interleaved_ref(audio_buf);
        let interleaved = buf.samples();

        for f in 0..frames {
            let mut mono = 0.0_f32;
            let base = f * ch;
            for c in 0..ch {
                mono += interleaved[base + c];
            }
            mono /= ch as f32;
            peak_amp = peak_amp.max(mono.abs());

            if let Some(ref mut ch_ext) = chroma {
                ch_ext.push_sample(mono);
            }
            onset.push_sample(mono);

            if cur_fine_count == 0 {
                cur_min = mono;
                cur_max = mono;
            } else {
                if mono < cur_min {
                    cur_min = mono;
                }
                if mono > cur_max {
                    cur_max = mono;
                }
            }
            cur_fine_count += 1;
            if cur_fine_count >= fine_block_frames {
                fine_peaks.push([cur_min, cur_max]);
                cur_fine_count = 0;
            }

            total_frames += 1;
        }
    }

    if cur_fine_count > 0 {
        fine_peaks.push([cur_min, cur_max]);
    }
    onset.finish();
    let onset_hz = onset.onset_hz;

    let duration_sec = total_frames as f64 / sample_rate.max(1) as f64;
    let (
        hpcp_full_36,
        hpcp_bass_36,
        tuning_hist,
        tonal_frames,
        chroma_timeline,
        chroma_hz,
        chroma_offset_sec,
    ) = match chroma {
        Some(c) => {
            let hz = c.fast_chroma_hz();
            let off = c.fast_chroma_center_offset_sec();
            (
                c.hpcp_full_36,
                c.hpcp_bass_36,
                c.tuning_hist,
                c.tonal_frames,
                c.chroma_timeline,
                hz,
                off,
            )
        }
        None => ([0.0; 36], [0.0; 36], [0.0; 100], 0, Vec::new(), 0.0, 0.0),
    };

    Ok(StemEnvelopeRaw {
        source_file: path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("audio")
            .to_string(),
        duration_sec,
        sample_rate,
        channels,
        onset_hz,
        fine_peaks,
        low_flux: onset.low_flux,
        full_flux: onset.full_flux,
        full_rms: onset.full_rms,
        hpcp_full_36,
        hpcp_bass_36,
        tuning_hist,
        tonal_frames,
        chroma_timeline,
        chroma_hz,
        chroma_offset_sec,
        peak_amp,
    })
}

fn compute_chroma_flux_at_env(
    timeline: &[[f32; 12]],
    chroma_hz: f64,
    chroma_offset_sec: f64,
    env_len: usize,
    env_hz: f64,
) -> Vec<f32> {
    if timeline.len() < 4 || chroma_hz <= 0.0 || env_len == 0 || env_hz <= 0.0 {
        return Vec::new();
    }
    let mut flux = vec![0.0_f32; env_len];
    let half_span_sec = 0.34_f64;
    let gap_sec = 0.02_f64;

    for (idx, slot) in flux.iter_mut().enumerate() {
        let t = idx as f64 / env_hz;
        let to_cidx = |t_sec: f64| -> isize {
            ((t_sec - chroma_offset_sec) * chroma_hz).round() as isize
        };
        let b_lo = to_cidx(t - half_span_sec).max(0) as usize;
        let b_hi = to_cidx(t - gap_sec).clamp(0, timeline.len() as isize) as usize;
        let a_lo = to_cidx(t + gap_sec).clamp(0, timeline.len() as isize) as usize;
        let a_hi = to_cidx(t + half_span_sec).clamp(0, timeline.len() as isize) as usize;

        if a_lo >= a_hi {
            continue;
        }
        let mut a_sum = [0.0_f64; 12];
        for frame in &timeline[a_lo..a_hi] {
            for pc in 0..12 {
                a_sum[pc] += frame[pc] as f64;
            }
        }
        let a_tot: f64 = a_sum.iter().sum();
        if a_tot <= 1e-4 {
            continue;
        }

        if b_lo >= b_hi {
            *slot = 0.55;
            continue;
        }
        let mut b_sum = [0.0_f64; 12];
        for frame in &timeline[b_lo..b_hi] {
            for pc in 0..12 {
                b_sum[pc] += frame[pc] as f64;
            }
        }
        let b_tot: f64 = b_sum.iter().sum();
        if b_tot <= a_tot * 0.12 {
            // Musical entrance after silence/fade
            *slot = 0.55;
            continue;
        }

        let mut l1 = 0.0_f64;
        for pc in 0..12 {
            l1 += ((a_sum[pc] / a_tot) - (b_sum[pc] / b_tot)).abs();
        }
        *slot = (0.5 * l1) as f32;
    }
    flux
}

/// Streams an audio file through Symphonia without keeping full PCM in memory,
/// producing normalized `[min, max]` waveform buckets, comb-filter/phase-locked BPM & downbeat,
/// and confidence-gated chromagram key estimation.
pub fn analyze_audio_file(path: &Path, num_buckets: usize) -> Result<AudioAnalysisResult> {
    analyze_audio_file_with_timing(path, num_buckets, None, 4)
}

/// Same as `analyze_audio_file`, with optional `hint_bpm` and `beats_per_bar` so Tap Tempo
/// or manual tempo overrides re-lock Beat 1 cleanly onto the audio waveform.
pub fn analyze_audio_file_with_timing(
    path: &Path,
    num_buckets: usize,
    hint_bpm: Option<f64>,
    beats_per_bar: u8,
) -> Result<AudioAnalysisResult> {
    let target_buckets = num_buckets.clamp(64, 4096);
    let stem_name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("audio");
    let (is_pitched, is_cues, _) = classify_stem_filename(stem_name);
    let extract_chroma = is_pitched && !is_cues;

    let raw = extract_stem_envelopes(path, extract_chroma)?;
    let clamped_peaks: Vec<[f32; 2]> = raw
        .fine_peaks
        .iter()
        .map(|p| [p[0].clamp(-1.0, 1.0), p[1].clamp(-1.0, 1.0)])
        .collect();
    let peaks = resample_peak_buckets(&clamped_peaks, target_buckets);
    let env_len = raw
        .low_flux
        .len()
        .min(raw.full_flux.len())
        .min(raw.full_rms.len());
    let chroma_flux = if extract_chroma {
        compute_chroma_flux_at_env(
            &raw.chroma_timeline,
            raw.chroma_hz,
            raw.chroma_offset_sec,
            env_len,
            raw.onset_hz,
        )
    } else {
        Vec::new()
    };
    let (estimated_bpm, estimated_first_downbeat_sec) = estimate_bpm_and_downbeat_with_options(
        &raw.low_flux[..env_len],
        &raw.full_flux[..env_len],
        &raw.full_rms[..env_len],
        &chroma_flux,
        raw.onset_hz,
        hint_bpm,
        beats_per_bar.clamp(1, 16) as usize,
    );

    let (estimated_key, key_confidence, key_candidates) = if extract_chroma {
        let (full_chroma, bass_chroma) =
            resolve_tuned_hpcp_12(&raw.hpcp_full_36, &raw.hpcp_bass_36, &raw.tuning_hist);
        estimate_key_from_chroma(&full_chroma, &bass_chroma, raw.tonal_frames)
    } else {
        (None, 0.0, Vec::new())
    };

    Ok(AudioAnalysisResult {
        source_file: raw.source_file,
        duration_sec: raw.duration_sec,
        sample_rate: raw.sample_rate,
        channels: raw.channels,
        estimated_bpm,
        estimated_first_downbeat_sec,
        estimated_key,
        key_confidence,
        key_candidates,
        peaks,
    })
}

/// Analyzes ALL non-cue/non-click stems in a song directory as a unified mixdown so that
/// drumless intros still lock Bar 1 onto the musical start, harmonic stems feed BPM & key detection,
/// and unpitched drum/click stems never corrupt key estimates.
pub fn analyze_song_folder_mixdown(
    dir: &Path,
    manifest: Option<&SongManifest>,
    num_buckets: usize,
) -> Result<AudioAnalysisResult> {
    let beats_per_bar = manifest.map(|m| m.time_signature[0]).unwrap_or(4);
    analyze_song_folder_mixdown_with_timing(dir, manifest, num_buckets, None, beats_per_bar)
}

/// Same as `analyze_song_folder_mixdown`, with optional `hint_bpm` and `beats_per_bar` so
/// Tap Tempo calibration snaps Beat 1 directly onto harmonic & rhythmic downbeats at that BPM.
pub fn analyze_song_folder_mixdown_with_timing(
    dir: &Path,
    manifest: Option<&SongManifest>,
    num_buckets: usize,
    hint_bpm: Option<f64>,
    beats_per_bar: u8,
) -> Result<AudioAnalysisResult> {
    let target_buckets = num_buckets.clamp(64, 4096);
    let files = discover_audio_files(dir)?;
    if files.is_empty() {
        return Err(AudioCoreError::Manifest(format!(
            "No supported audio files found in {}",
            dir.display()
        )));
    }

    struct CandidateMeta {
        file: String,
        lower: String,
        pitched: bool,
        energy_weight: f32,
        chroma_weight: f64,
    }

    let mut candidates: Vec<CandidateMeta> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(m) = manifest {
        for (stem_key, stem_def) in &m.stems {
            let fname = stem_def.file_name().to_string();
            let lower = format!("{} {}", stem_key, fname).to_ascii_lowercase();
            if stem_def.is_cues(stem_key)
                || lower.contains("click")
                || lower.contains("cue")
                || lower.contains("guide")
            {
                continue;
            }
            if !dir.join(&fname).exists() || !seen.insert(fname.clone()) {
                continue;
            }
            let pitched = stem_def.is_pitched(stem_key);
            candidates.push(CandidateMeta {
                file: fname,
                lower,
                pitched,
                energy_weight: 1.0,
                chroma_weight: 1.0,
            });
        }
    }

    for fname in &files {
        let l = fname.to_ascii_lowercase();
        if l.contains("cue") || l.contains("click") || l.contains("guide") || !seen.insert(fname.clone()) {
            continue;
        }
        let stem_base = Path::new(fname)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(fname);
        let (pitched, is_cues, _) = classify_stem_filename(stem_base);
        if is_cues {
            continue;
        }
        candidates.push(CandidateMeta {
            file: fname.clone(),
            lower: l,
            pitched,
            energy_weight: 1.0,
            chroma_weight: 1.0,
        });
    }

    let has_non_vocal = candidates
        .iter()
        .any(|c| !(c.lower.contains("vocal") || c.lower.contains("vox")));
    for c in &mut candidates {
        let is_drums = c.lower.contains("drum") || c.lower.contains("perc");
        let is_bass = c.lower.contains("bass");
        let is_vocals = c.lower.contains("vocal") || c.lower.contains("vox");
        let is_music = c.lower.contains("music")
            || c.lower.contains("instr")
            || c.lower.contains("other")
            || c.lower.contains("backing")
            || c.lower.contains("key")
            || c.lower.contains("piano")
            || c.lower.contains("synth")
            || c.lower.contains("guitar");

        // Music + drums stems carry the steady groove & harmony even when sparse stems go silent.
        c.energy_weight = if is_drums {
            1.50
        } else if is_music {
            1.15
        } else if is_bass {
            0.90
        } else if is_vocals && has_non_vocal {
            0.12
        } else {
            1.0
        };
        c.chroma_weight = if !c.pitched || is_drums {
            0.0
        } else if is_music {
            1.40
        } else if is_bass {
            1.15
        } else if is_vocals && has_non_vocal {
            0.15
        } else {
            1.0
        };
    }

    if candidates.is_empty() {
        return analyze_audio_file_with_timing(
            &dir.join(&files[0]),
            target_buckets,
            hint_bpm,
            beats_per_bar,
        );
    }

    let raw_results: Vec<Option<(CandidateMeta, StemEnvelopeRaw)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = candidates
            .into_iter()
            .map(|meta| {
                let path = dir.join(&meta.file);
                let pitched = meta.pitched;
                scope.spawn(move || {
                    extract_stem_envelopes(&path, pitched)
                        .ok()
                        .map(|raw| (meta, raw))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().ok().flatten())
            .collect()
    });

    let mut extracted: Vec<(CandidateMeta, StemEnvelopeRaw)> = Vec::new();
    for (meta, raw) in raw_results.into_iter().flatten() {
        if raw.peak_amp >= 0.002 || extracted.is_empty() {
            extracted.push((meta, raw));
        }
    }

    if extracted.is_empty() {
        return Err(AudioCoreError::Decode(format!(
            "Failed to analyze any audio file in {}",
            dir.display()
        )));
    }

    let max_duration_sec = extracted
        .iter()
        .map(|(_, r)| r.duration_sec)
        .fold(0.0_f64, f64::max);
    let sample_rate = extracted[0].1.sample_rate;
    let channels = extracted[0].1.channels;
    let onset_hz = extracted[0].1.onset_hz;
    let max_fine_len = extracted
        .iter()
        .map(|(_, r)| r.fine_peaks.len())
        .max()
        .unwrap_or(0);
    let max_onset_len = extracted
        .iter()
        .map(|(_, r)| {
            let n = r.low_flux.len().min(r.full_flux.len()).min(r.full_rms.len());
            if (r.onset_hz - onset_hz).abs() < 1e-3 || r.onset_hz <= 0.0 {
                n
            } else {
                ((n as f64 * onset_hz / r.onset_hz).ceil() as usize).max(n)
            }
        })
        .max()
        .unwrap_or(0);

    let mut combined_fine = vec![[0.0_f32; 2]; max_fine_len];
    let mut combined_low_flux = vec![0.0_f32; max_onset_len];
    let mut combined_full_flux = vec![0.0_f32; max_onset_len];
    let mut full_sq = vec![0.0_f32; max_onset_len];
    let mut hpcp_full_36 = [0.0_f64; 36];
    let mut hpcp_bass_36 = [0.0_f64; 36];
    let mut tuning_hist = [0.0_f64; 100];
    let mut total_tonal_frames = 0usize;

    let ref_tonal = extracted
        .iter()
        .find(|(m, r)| m.chroma_weight > 0.0 && !r.chroma_timeline.is_empty());
    let ref_chroma_hz = ref_tonal.map(|(_, r)| r.chroma_hz).unwrap_or(0.0);
    let ref_chroma_offset = ref_tonal.map(|(_, r)| r.chroma_offset_sec).unwrap_or(0.0);
    let max_chroma_len = extracted
        .iter()
        .filter(|(m, _)| m.chroma_weight > 0.0)
        .map(|(_, r)| r.chroma_timeline.len())
        .max()
        .unwrap_or(0);
    let mut combined_chroma_timeline = vec![[0.0_f32; 12]; max_chroma_len];

    for (meta, raw) in &extracted {
        for (i, p) in raw.fine_peaks.iter().enumerate() {
            combined_fine[i][0] += p[0];
            combined_fine[i][1] += p[1];
        }
        let w = meta.energy_weight;
        let w2 = w * w;
        let n_on = raw
            .low_flux
            .len()
            .min(raw.full_flux.len())
            .min(raw.full_rms.len());
        // Gate sparse stem bleed during silent passages relative to each stem's active median level.
        let mut stem_active: Vec<f32> = raw.full_rms[..n_on]
            .iter()
            .copied()
            .filter(|&v| v > 0.002)
            .collect();
        let stem_gate = if stem_active.is_empty() {
            0.004
        } else {
            stem_active.sort_by(|a, b| a.total_cmp(b));
            (stem_active[stem_active.len() / 2] * 0.08).max(0.004)
        };

        // Normalize active onset flux per stem so quiet stems (e.g., dynamics-unmastered drums)
        // contribute proportionally according to their rhythmic role weight `w`.
        let mut active_low_sq = 0.0_f64;
        let mut active_full_sq = 0.0_f64;
        let mut active_cnt = 0usize;
        for i in 0..n_on {
            if raw.full_rms[i] >= stem_gate {
                active_low_sq += (raw.low_flux[i] as f64).powi(2);
                active_full_sq += (raw.full_flux[i] as f64).powi(2);
                active_cnt += 1;
            }
        }
        let low_scale = if active_cnt > 0 {
            let rms = (active_low_sq / active_cnt as f64).sqrt() as f32;
            if rms > 1e-4 { 1.0 / rms } else { 0.0 }
        } else {
            0.0
        };
        let full_scale = if active_cnt > 0 {
            let rms = (active_full_sq / active_cnt as f64).sqrt() as f32;
            if rms > 1e-4 { 1.0 / rms } else { 0.0 }
        } else {
            0.0
        };

        let same_rate = (raw.onset_hz - onset_hz).abs() < 1e-3 || raw.onset_hz <= 0.0;
        let dst_len = if same_rate {
            n_on.min(max_onset_len)
        } else {
            ((n_on as f64 * onset_hz / raw.onset_hz).round() as usize).min(max_onset_len)
        };
        for dst in 0..dst_len {
            let src = if same_rate {
                dst
            } else {
                ((dst as f64 * raw.onset_hz / onset_hz).round() as usize).min(n_on.saturating_sub(1))
            };
            if src < n_on && raw.full_rms[src] >= stem_gate {
                combined_low_flux[dst] += w * raw.low_flux[src] * low_scale;
                combined_full_flux[dst] += w * raw.full_flux[src] * full_scale;
                full_sq[dst] += w2 * raw.full_rms[src] * raw.full_rms[src];
            }
        }
        if meta.chroma_weight > 0.0 && raw.tonal_frames > 0 {
            for b in 0..36 {
                hpcp_full_36[b] += raw.hpcp_full_36[b] * meta.chroma_weight;
                hpcp_bass_36[b] += raw.hpcp_bass_36[b] * meta.chroma_weight;
            }
            for c in 0..100 {
                tuning_hist[c] += raw.tuning_hist[c] * meta.chroma_weight;
            }
            total_tonal_frames += raw.tonal_frames;
        }
        if meta.chroma_weight > 0.0 && !raw.chroma_timeline.is_empty() && ref_chroma_hz > 0.0 {
            let cw = meta.chroma_weight as f32;
            for (j, frame) in raw.chroma_timeline.iter().enumerate() {
                let dst = if (raw.chroma_hz - ref_chroma_hz).abs() < 1e-3 {
                    j
                } else {
                    let t_sec = raw.chroma_offset_sec + j as f64 / raw.chroma_hz;
                    ((t_sec - ref_chroma_offset) * ref_chroma_hz).round().max(0.0) as usize
                };
                if dst < combined_chroma_timeline.len() {
                    for pc in 0..12 {
                        combined_chroma_timeline[dst][pc] += frame[pc] * cw;
                    }
                }
            }
        }
    }

    let max_abs = combined_fine
        .iter()
        .fold(1.0_f32, |acc, p| acc.max(p[0].abs()).max(p[1].abs()));
    for p in &mut combined_fine {
        p[0] = (p[0] / max_abs).clamp(-1.0, 1.0);
        p[1] = (p[1] / max_abs).clamp(-1.0, 1.0);
    }

    let combined_full_rms: Vec<f32> = full_sq.into_iter().map(|v| v.sqrt()).collect();
    let chroma_flux = compute_chroma_flux_at_env(
        &combined_chroma_timeline,
        ref_chroma_hz,
        ref_chroma_offset,
        max_onset_len,
        onset_hz,
    );

    let peaks = resample_peak_buckets(&combined_fine, target_buckets);
    let (estimated_bpm, estimated_first_downbeat_sec) = estimate_bpm_and_downbeat_with_options(
        &combined_low_flux,
        &combined_full_flux,
        &combined_full_rms,
        &chroma_flux,
        onset_hz,
        hint_bpm,
        beats_per_bar.clamp(1, 16) as usize,
    );

    let (chroma_sum, bass_chroma_sum) =
        resolve_tuned_hpcp_12(&hpcp_full_36, &hpcp_bass_36, &tuning_hist);
    let (estimated_key, key_confidence, key_candidates) =
        estimate_key_from_chroma(&chroma_sum, &bass_chroma_sum, total_tonal_frames);

    let source_file = if extracted.len() == 1 {
        extracted[0].1.source_file.clone()
    } else {
        format!("Mixdown ({} stems)", extracted.len())
    };

    Ok(AudioAnalysisResult {
        source_file,
        duration_sec: max_duration_sec,
        sample_rate,
        channels,
        estimated_bpm,
        estimated_first_downbeat_sec,
        estimated_key,
        key_confidence,
        key_candidates,
        peaks,
    })
}

fn pearson_corr(a: &[f64; 12], b: &[f64; 12]) -> f64 {
    let mean_a = a.iter().sum::<f64>() / 12.0;
    let mean_b = b.iter().sum::<f64>() / 12.0;
    let mut num = 0.0_f64;
    let mut da2 = 0.0_f64;
    let mut db2 = 0.0_f64;
    for i in 0..12 {
        let da = a[i] - mean_a;
        let db = b[i] - mean_b;
        num += da * db;
        da2 += da * da;
        db2 += db * db;
    }
    let den = (da2 * db2).sqrt();
    if den <= 1e-12 {
        0.0
    } else {
        num / den
    }
}

/// Estimates A4 tuning deviation from the accumulated cents histogram, shifts the 36-bin HPCP
/// back to A4=440Hz concert pitch, and folds each semitone triplet into 12 chroma classes.
fn resolve_tuned_hpcp_12(
    hpcp_full_36: &[f64; 36],
    hpcp_bass_36: &[f64; 36],
    tuning_hist: &[f64; 100],
) -> ([f64; 12], [f64; 12]) {
    let hist_sum: f64 = tuning_hist.iter().sum();
    let cents_offset = if hist_sum > 1e-6 {
        let mut best_bin = 50usize;
        let mut best_val = -1.0_f64;
        for b in 0..100 {
            // Smooth circularly +/- 2 cents before picking dominant tuning peak
            let mut acc = 0.0_f64;
            for d in -2isize..=2isize {
                let idx = (b as isize + d).rem_euclid(100) as usize;
                let w = 1.0 - (d.abs() as f64) * 0.25;
                acc += tuning_hist[idx] * w;
            }
            if acc > best_val {
                best_val = acc;
                best_bin = b;
            }
        }
        // Check circular weighted centroid within +/- 5 cents around the peak
        let mut num = 0.0_f64;
        let mut den = 0.0_f64;
        for d in -5isize..=5isize {
            let idx = (best_bin as isize + d).rem_euclid(100) as usize;
            let v = tuning_hist[idx];
            num += v * (best_bin as f64 + d as f64 - 49.5);
            den += v;
        }
        if den > hist_sum * 0.04 {
            (num / den).clamp(-48.0, 48.0)
        } else {
            0.0
        }
    } else {
        0.0
    };

    let shift_bins = (cents_offset / 100.0) * 3.0;
    let sample_interp = |arr: &[f64; 36], pos: f64| -> f64 {
        let p = pos.rem_euclid(36.0);
        let lo = p.floor() as usize % 36;
        let hi = (lo + 1) % 36;
        let frac = p - p.floor();
        arr[lo] * (1.0 - frac) + arr[hi] * frac
    };

    let mut full_12 = [0.0_f64; 12];
    let mut bass_12 = [0.0_f64; 12];
    for pc in 0..12 {
        let center = pc as f64 * 3.0 + shift_bins;
        full_12[pc] = 0.25 * sample_interp(hpcp_full_36, center - 1.0)
            + 0.50 * sample_interp(hpcp_full_36, center)
            + 0.25 * sample_interp(hpcp_full_36, center + 1.0);
        bass_12[pc] = 0.25 * sample_interp(hpcp_bass_36, center - 1.0)
            + 0.50 * sample_interp(hpcp_bass_36, center)
            + 0.25 * sample_interp(hpcp_bass_36, center + 1.0);
    }
    (full_12, bass_12)
}

/// Evaluates all 24 major/minor key profiles using blended Krumhansl-Kessler, Temperley,
/// and Shaath/EDMA contemporary pop/worship profiles + bass tonic tie-break.
/// Returns `(estimated_key, confidence_0_to_1, top_candidates)` and sets `estimated_key = None`
/// whenever harmonic clarity, correlation, or separation margin is too low to trust.
fn estimate_key_from_chroma(
    full_chroma: &[f64; 12],
    bass_chroma: &[f64; 12],
    tonal_frames: usize,
) -> (Option<String>, f64, Vec<String>) {
    let full_sum: f64 = full_chroma.iter().sum();
    if tonal_frames < 8 || full_sum <= 1e-6 {
        return (None, 0.0, Vec::new());
    }

    let bass_sum: f64 = bass_chroma.iter().sum();
    let mut combined = [0.0_f64; 12];
    let mut bass_frac = [0.0_f64; 12];
    for pc in 0..12 {
        let f = full_chroma[pc] / full_sum;
        let b = if bass_sum > 1e-6 {
            bass_chroma[pc] / bass_sum
        } else {
            f
        };
        bass_frac[pc] = b;
        combined[pc] = 0.78 * f + 0.22 * b;
    }

    let mean = combined.iter().sum::<f64>() / 12.0;
    let variance = combined
        .iter()
        .map(|&v| (v - mean) * (v - mean))
        .sum::<f64>()
        / 12.0;
    let cv = variance.sqrt() / mean.max(1e-9);
    if cv < 0.17 {
        // Near-uniform pitch class distribution (percussion, broadband noise, or chromatic cluster).
        return (None, (cv / 0.17 * 0.25).clamp(0.0, 0.25), Vec::new());
    }

    const KK_MAJOR: [f64; 12] = [
        6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
    ];
    const KK_MINOR: [f64; 12] = [
        6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
    ];
    const TEMPERLEY_MAJOR: [f64; 12] = [
        5.0, 2.0, 3.5, 2.0, 4.5, 4.0, 2.0, 4.5, 2.0, 3.5, 1.5, 4.0,
    ];
    const TEMPERLEY_MINOR: [f64; 12] = [
        5.0, 2.0, 3.5, 4.5, 2.0, 4.0, 2.0, 4.5, 3.5, 2.0, 1.5, 4.0,
    ];
    const EDMA_MAJOR: [f64; 12] = [
        1.00, 0.29, 0.50, 0.40, 0.60, 0.56, 0.32, 0.80, 0.31, 0.45, 0.42, 0.39,
    ];
    const EDMA_MINOR: [f64; 12] = [
        1.00, 0.31, 0.44, 0.72, 0.35, 0.54, 0.29, 0.78, 0.55, 0.34, 0.58, 0.34,
    ];

    #[derive(Clone)]
    struct Cand {
        tonic: usize,
        is_minor: bool,
        label: String,
        raw_r: f64,
        score: f64,
    }

    let mut cands = Vec::with_capacity(24);
    for tonic in 0..12 {
        let mut rot = [0.0_f64; 12];
        for step in 0..12 {
            rot[step] = combined[(tonic + step) % 12];
        }
        let bass_bonus = 0.10 * (bass_frac[tonic] - 1.0 / 12.0);
        let r_maj = 0.40 * pearson_corr(&rot, &KK_MAJOR)
            + 0.35 * pearson_corr(&rot, &TEMPERLEY_MAJOR)
            + 0.25 * pearson_corr(&rot, &EDMA_MAJOR);
        cands.push(Cand {
            tonic,
            is_minor: false,
            label: CHROMATIC_KEYS[tonic].to_string(),
            raw_r: r_maj,
            score: r_maj + bass_bonus,
        });

        let r_min = 0.40 * pearson_corr(&rot, &KK_MINOR)
            + 0.35 * pearson_corr(&rot, &TEMPERLEY_MINOR)
            + 0.25 * pearson_corr(&rot, &EDMA_MINOR);
        cands.push(Cand {
            tonic,
            is_minor: true,
            label: format!("{}m", CHROMATIC_KEYS[tonic]),
            raw_r: r_min,
            score: r_min + bass_bonus,
        });
    }

    cands.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| b.raw_r.total_cmp(&a.raw_r)));

    let top1 = &cands[0];
    let top2 = &cands[1];
    let is_relative = |a: &Cand, b: &Cand| -> bool {
        if a.is_minor == b.is_minor {
            return false;
        }
        if a.is_minor {
            !b.is_minor && b.tonic == (a.tonic + 3) % 12
        } else {
            b.is_minor && b.tonic == (a.tonic + 9) % 12
        }
    };

    let best_non_rel = cands
        .iter()
        .skip(1)
        .find(|c| !is_relative(top1, c))
        .unwrap_or(top2);

    let gap = (top1.score - top2.score).max(0.0);
    let gap_non_rel = (top1.score - best_non_rel.score).max(0.0);

    let corr_score = ((top1.raw_r - 0.35) / 0.45).clamp(0.0, 1.0);
    let non_rel_score = (gap_non_rel / 0.12).clamp(0.0, 1.0);
    let rel_score = (gap / 0.06).clamp(0.0, 1.0);
    let peakedness = ((cv - 0.15) / 0.30).clamp(0.0, 1.0);
    let confidence =
        (0.40 * corr_score + 0.35 * non_rel_score + 0.15 * rel_score + 0.10 * peakedness)
            .clamp(0.0, 1.0);
    let rounded_conf = (confidence * 100.0).round() / 100.0;

    let candidates = if top1.raw_r >= 0.35 {
        vec![top1.label.clone(), top2.label.clone()]
    } else {
        Vec::new()
    };

    let confident = top1.raw_r >= 0.48
        && gap_non_rel >= 0.045
        && gap >= 0.015
        && rounded_conf >= 0.50;

    if confident {
        (Some(top1.label.clone()), rounded_conf, candidates)
    } else {
        (None, rounded_conf, candidates)
    }
}

fn resample_peak_buckets(fine: &[[f32; 2]], target: usize) -> Vec<[f32; 2]> {
    if fine.is_empty() {
        return vec![[0.0, 0.0]; target];
    }
    let mut out = Vec::with_capacity(target);
    let len = fine.len();
    for b in 0..target {
        let start = (b * len) / target;
        let end = (((b + 1) * len) / target).max(start + 1).min(len);
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        for item in &fine[start..end] {
            if item[0] < lo {
                lo = item[0];
            }
            if item[1] > hi {
                hi = item[1];
            }
        }
        out.push([lo, hi]);
    }
    out
}

fn rms_normalize_flux(vals: &[f32]) -> Vec<f32> {
    let mut sq = 0.0_f64;
    let mut cnt = 0usize;
    for &v in vals {
        if v > 1e-5 {
            sq += (v as f64) * (v as f64);
            cnt += 1;
        }
    }
    if cnt == 0 {
        return vec![0.0; vals.len()];
    }
    let scale = 1.0 / (sq / cnt as f64).sqrt().max(1e-6);
    vals.iter()
        .map(|&v| ((v as f64) * scale).min(8.0) as f32)
        .collect()
}

fn estimate_bpm_and_downbeat_with_options(
    low_flux: &[f32],
    full_flux: &[f32],
    full_rms: &[f32],
    chroma_flux: &[f32],
    env_hz: f64,
    hint_bpm: Option<f64>,
    beats_per_bar: usize,
) -> (f64, f64) {
    let bpb = beats_per_bar.clamp(1, 16);
    let len = low_flux.len().min(full_flux.len()).min(full_rms.len());
    if len < (env_hz * 3.0) as usize {
        let fallback_bpm = hint_bpm.map(|b| b.clamp(30.0, 260.0)).unwrap_or(120.0);
        return (fallback_bpm, 0.0);
    }

    // Independently normalize low & full SuperFlux envelopes so drumless chord/piano tracks
    // contribute just as cleanly as kick/bass-heavy tracks without scaling saturation.
    let norm_low = rms_normalize_flux(&low_flux[..len]);
    let norm_full = rms_normalize_flux(&full_flux[..len]);
    let has_low = norm_low.iter().any(|&v| v > 0.05);

    let mut onset = Vec::with_capacity(len);
    for i in 0..len {
        if has_low {
            onset.push(0.45 * norm_low[i] + 0.55 * norm_full[i]);
        } else {
            onset.push(norm_full[i]);
        }
    }

    // Subtract local moving mean (~240 ms window) so sustained pads/swells never smear periodicity
    let half_win = (0.12 * env_hz).round().max(2.0) as usize;
    let mut cleaned = vec![0.0_f32; len];
    let mut low_cleaned = vec![0.0_f32; len];
    let mut full_cleaned = vec![0.0_f32; len];
    let mut prefix_on = vec![0.0_f64; len + 1];
    let mut prefix_lo = vec![0.0_f64; len + 1];
    let mut prefix_fu = vec![0.0_f64; len + 1];
    for i in 0..len {
        prefix_on[i + 1] = prefix_on[i] + onset[i] as f64;
        prefix_lo[i + 1] = prefix_lo[i] + norm_low[i] as f64;
        prefix_fu[i + 1] = prefix_fu[i] + norm_full[i] as f64;
    }
    for i in 0..len {
        let lo = i.saturating_sub(half_win);
        let hi = (i + half_win + 1).min(len);
        let cnt = (hi - lo) as f64;
        cleaned[i] = (onset[i] - ((prefix_on[hi] - prefix_on[lo]) / cnt) as f32).max(0.0);
        low_cleaned[i] = (norm_low[i] - ((prefix_lo[hi] - prefix_lo[lo]) / cnt) as f32).max(0.0);
        full_cleaned[i] = (norm_full[i] - ((prefix_fu[hi] - prefix_fu[lo]) / cnt) as f32).max(0.0);
    }

    let mut phase_env = Vec::with_capacity(len);
    for i in 0..len {
        if has_low {
            phase_env.push(0.50 * cleaned[i] + 0.30 * low_cleaned[i] + 0.20 * full_cleaned[i]);
        } else {
            phase_env.push(cleaned[i]);
        }
    }

    // Find the first frame where actual audio energy starts (skipping pre-roll silence)
    let mut active_vals: Vec<f32> = full_rms.iter().copied().filter(|&v| v > 0.002).collect();
    let active_median = if active_vals.is_empty() {
        0.0
    } else {
        active_vals.sort_by(|a, b| a.total_cmp(b));
        active_vals[active_vals.len() / 2]
    };
    let energy_gate = (active_median * 0.05).max(0.003);
    let audio_start_idx = full_rms
        .iter()
        .position(|&v| v >= energy_gate)
        .unwrap_or(0);

    // Gaussian phase-locked grid scorer (~20ms pulse width) tolerating live human micro-timing
    // while tracking exact fractional tempo cleanly across the full song.
    let sigma_frames = (0.020 * env_hz).max(1.2);
    let inv_two_sigma2 = 0.5 / (sigma_frames * sigma_frames);
    let kern_r = (sigma_frames * 2.2).ceil() as isize;
    let score_bpm_phase = |bpm_cand: f64| -> (f64, f64) {
        let bp = (60.0 / bpm_cand) * env_hz;
        let phase_steps = (bp * 2.0).round().max(1.0) as usize;
        let mut best_p = 0.0_f64;
        let mut best_mean = -1.0_f64;
        for ph_step in 0..phase_steps {
            let ph = ph_step as f64 * 0.5;
            let mut sum = 0.0_f64;
            let mut count = 0usize;
            let mut k = 0usize;
            loop {
                let center_f = ph + k as f64 * bp;
                let center_i = center_f.round() as isize;
                if center_i >= len as isize {
                    break;
                }
                if center_i + kern_r >= audio_start_idx as isize {
                    let lo = (center_i - kern_r).max(0) as usize;
                    let hi = (center_i + kern_r).min((len - 1) as isize) as usize;
                    let mut w_peak = 0.0_f64;
                    for idx in lo..=hi {
                        let d = idx as f64 - center_f;
                        let g = (-d * d * inv_two_sigma2).exp();
                        let v = (phase_env[idx] as f64) * g;
                        if v > w_peak {
                            w_peak = v;
                        }
                    }
                    sum += w_peak;
                    count += 1;
                }
                k += 1;
            }
            let mean_score = if count > 0 { sum / count as f64 } else { 0.0 };
            if mean_score > best_mean {
                best_mean = mean_score;
                best_p = ph;
            }
        }
        (best_mean, best_p)
    };

    if let Some(hb) = hint_bpm {
        let fixed_bpm = ((hb.clamp(30.0, 260.0)) * 10.0).round() / 10.0;
        let (_, best_phase) = score_bpm_phase(fixed_bpm);
        let first_downbeat_sec = pick_bar_downbeat(
            &full_rms[..len],
            low_flux,
            full_flux,
            chroma_flux,
            env_hz,
            audio_start_idx,
            active_median,
            fixed_bpm,
            best_phase,
            bpb,
        );
        return (fixed_bpm, first_downbeat_sec);
    }

    // Windowed tempogram via zero-mean normalized FFT autocorrelation (~14s windows, 4s hop).
    // Clarity weighting automatically lets tight drum/strum grooves override quiet ambient swells.
    let active_start = audio_start_idx.min(len.saturating_sub((env_hz * 4.0) as usize));
    let active_end = (active_start + (env_hz * 240.0) as usize).min(len);
    let active_len = active_end.saturating_sub(active_start);

    let cand_min_bpm = 52.0_f64;
    let cand_max_bpm = 186.0_f64;
    let cand_min_lag = ((60.0 / cand_max_bpm) * env_hz).round() as usize;
    let cand_max_lag = (((60.0 / cand_min_bpm) * env_hz).round() as usize).min(active_len / 3);
    let min_acf_lag = (cand_min_lag / 2).max(4);
    let max_acf_lag = (cand_max_lag * 4 + 2).min(active_len.saturating_sub(2) / 2);

    if cand_max_lag <= cand_min_lag + 4 || max_acf_lag <= cand_min_lag + 4 {
        return (120.0, 0.0);
    }

    let win_len = ((14.0 * env_hz).round() as usize).min(active_len);
    let hop_len = ((4.0 * env_hz).round() as usize).max(1);
    let mut agg_acf = vec![0.0_f64; max_acf_lag + 1];
    let mut weight_sum = vec![0.0_f64; max_acf_lag + 1];

    let mut win_starts = Vec::new();
    let mut pos = active_start;
    while pos + win_len <= active_end {
        win_starts.push(pos);
        pos += hop_len;
    }
    if win_starts.is_empty() || *win_starts.last().unwrap() + win_len < active_end {
        win_starts.push(active_end - win_len);
    }

    let rms_win_floor = active_median * 0.28;
    for &w_start in &win_starts {
        let w_end = w_start + win_len;
        if win_starts.len() > 2 && active_median > 0.004 {
            let win_rms_mean = full_rms[w_start..w_end].iter().copied().sum::<f32>()
                / (w_end - w_start) as f32;
            if win_rms_mean < rms_win_floor {
                continue;
            }
        }
        let w_max_lag = max_acf_lag.min((win_len * 2) / 3);
        if w_max_lag <= cand_min_lag + 2 {
            continue;
        }
        let win_acf = fft_autocorrelation_norm(&cleaned[w_start..w_end], min_acf_lag, w_max_lag);
        let peak = win_acf[cand_min_lag..=cand_max_lag.min(w_max_lag)]
            .iter()
            .copied()
            .fold(0.0_f64, f64::max);
        let weight = peak.max(0.04).powf(1.5);
        for lag in min_acf_lag..=w_max_lag {
            agg_acf[lag] += weight * win_acf[lag];
            weight_sum[lag] += weight;
        }
    }

    // Fallback to whole-slice ACF if all windows were filtered out as ultra-quiet
    if weight_sum[cand_min_lag] <= 1e-9 {
        let full_acf =
            fft_autocorrelation_norm(&cleaned[active_start..active_end], min_acf_lag, max_acf_lag);
        agg_acf[min_acf_lag..=max_acf_lag].copy_from_slice(&full_acf[min_acf_lag..=max_acf_lag]);
    } else {
        for lag in min_acf_lag..=max_acf_lag {
            if weight_sum[lag] > 1e-9 {
                agg_acf[lag] /= weight_sum[lag];
            }
        }
    }

    let sample_acf = |lag_f: f64| -> Option<f64> {
        let lo = lag_f.floor() as usize;
        let hi = lo + 1;
        if lo < min_acf_lag || hi > max_acf_lag {
            return None;
        }
        let frac = lag_f - lo as f64;
        Some((agg_acf[lo] * (1.0 - frac) + agg_acf[hi] * frac).max(0.0))
    };

    let comb_at = |lag_f: f64| -> f64 {
        let harmonics = [(1.0_f64, 1.00), (2.0, 0.58), (3.0, 0.28), (4.0, 0.18)];
        let mut num = 0.0_f64;
        let mut den = 0.0_f64;
        for (mult, w) in harmonics {
            if let Some(v) = sample_acf(lag_f * mult) {
                num += w * v;
                den += w;
            }
        }
        if den > 0.0 {
            num / den
        } else {
            0.0
        }
    };

    // Ellis perceptual log-Gaussian tempo prior centered near 112 BPM (sigma = 0.65 octaves)
    let perceptual_prior = |bpm: f64| -> f64 {
        let oct = (bpm.max(20.0) / 112.0).log2() / 0.65;
        (-0.5 * oct * oct).exp()
    };

    let mut scores = vec![0.0_f64; cand_max_lag + 1];
    let mut best_lag = cand_min_lag;
    let mut best_score = -1.0_f64;
    for lag in cand_min_lag..=cand_max_lag {
        let l = lag as f64;
        let bpm = (60.0 * env_hz) / l;
        let s = comb_at(l) * (0.68 + 0.32 * perceptual_prior(bpm));
        scores[lag] = s;
        if s > best_score {
            best_score = s;
            best_lag = lag;
        }
    }

    // Parabolic interpolation around best_lag for sub-sample precision
    let mut refined_lag = if best_lag > cand_min_lag && best_lag < cand_max_lag {
        let y0 = scores[best_lag - 1];
        let y1 = scores[best_lag];
        let y2 = scores[best_lag + 1];
        let denom = 2.0 * (y0 - 2.0 * y1 + y2);
        if denom.abs() > 1e-12 {
            let delta = ((y0 - y2) / denom).clamp(-0.5, 0.5);
            best_lag as f64 + delta
        } else {
            best_lag as f64
        }
    } else {
        best_lag as f64
    };

    // Two-way harmonic/octave disambiguation (slow*2 vs fast/2 & triplet guard) using separate
    // kick/low pulse alternation vs full-spectrum backbeat symmetry.
    let mut raw_bpm = ((60.0 * env_hz) / refined_lag).clamp(40.0, 220.0);
    let subpulse_metrics = |period_fast: f64, k_mult: usize| -> (f64, f64, f64) {
        let macro_period = period_fast * k_mult as f64;
        let steps = (macro_period.round().max(1.0) as usize).min(len);
        let search_end = (active_start + (env_hz * 90.0) as usize).min(active_end);
        let mut best_phase = 0usize;
        let mut best_anchor = -1.0_f64;
        for ph in 0..steps {
            let mut sum = 0.0_f64;
            let mut m = 0usize;
            loop {
                let c = (active_start as f64 + ph as f64 + m as f64 * macro_period).round() as usize;
                if c >= search_end {
                    break;
                }
                let lo = c.saturating_sub(1);
                let hi = (c + 2).min(search_end);
                let val = (lo..hi)
                    .map(|idx| (0.55 * norm_low[idx] + 0.45 * norm_full[idx]) as f64)
                    .fold(0.0_f64, f64::max);
                sum += val;
                m += 1;
            }
            if sum > best_anchor {
                best_anchor = sum;
                best_phase = ph;
            }
        }

        let mut low_sums = vec![0.0_f64; k_mult];
        let mut full_sums = vec![0.0_f64; k_mult];
        let mut counts = vec![0usize; k_mult];
        let mut k = 0usize;
        loop {
            let c = (active_start as f64 + best_phase as f64 + k as f64 * period_fast).round() as usize;
            if c >= search_end {
                break;
            }
            let lo = c.saturating_sub(1);
            let hi = (c + 2).min(search_end);
            let low_v = (lo..hi).map(|idx| norm_low[idx] as f64).fold(0.0_f64, f64::max);
            let full_v = (lo..hi).map(|idx| norm_full[idx] as f64).fold(0.0_f64, f64::max);
            let slot = k % k_mult;
            low_sums[slot] += low_v;
            full_sums[slot] += full_v;
            counts[slot] += 1;
            k += 1;
        }
        let anchor_low = low_sums[0] / counts[0].max(1) as f64;
        let off_low = (1..k_mult)
            .map(|s| low_sums[s] / counts[s].max(1) as f64)
            .sum::<f64>()
            / (k_mult - 1).max(1) as f64;
        let anchor_full = full_sums[0] / counts[0].max(1) as f64;
        let off_full = (1..k_mult)
            .map(|s| full_sums[s] / counts[s].max(1) as f64)
            .sum::<f64>()
            / (k_mult - 1).max(1) as f64;
        (
            anchor_low / (off_low + 1e-6),
            anchor_full / (off_full + 1e-6),
            off_full / (anchor_full + 1e-6),
        )
    };

    if raw_bpm > 110.0 && raw_bpm / 2.0 >= 52.0 {
        let (low_r2, full_r2, odd_vs_even) = subpulse_metrics(refined_lag, 2);
        let c_fast = comb_at(refined_lag).max(1e-9);
        let c_half = comb_at(refined_lag * 2.0);
        let comb_ratio_2 = c_half / c_fast;
        // Halve when off-beats are primarily light high-frequency subdivisions rather than punchy snare backbeats
        if (low_r2 >= 1.28 && odd_vs_even <= 0.65)
            || (raw_bpm >= 146.0 && low_r2 >= 1.22 && full_r2 >= 1.15 && comb_ratio_2 >= 0.82)
        {
            refined_lag *= 2.0;
            raw_bpm /= 2.0;
        }
    } else if raw_bpm < 94.0 && raw_bpm * 2.0 <= 184.0 {
        let fast_lag = refined_lag * 0.5;
        let (low_r2, _full_r2, odd_vs_even) = subpulse_metrics(fast_lag, 2);
        let acf_slow = sample_acf(refined_lag).unwrap_or(0.0).max(1e-9);
        let acf_fast = sample_acf(fast_lag).unwrap_or(0.0);
        let odd_low_vs_even = 1.0 / low_r2.max(1e-6);
        let double_bpm = raw_bpm * 2.0;
        // Double when odd pulses have real low-end hits (four-on-the-floor / steady kick) OR balanced snare backbeats
        let strong_off_beats = odd_low_vs_even >= 0.45
            || (odd_vs_even >= 0.56 && (95.0..=165.0).contains(&double_bpm));
        if strong_off_beats && (acf_fast / acf_slow) >= 0.35 {
            refined_lag = fast_lag;
            raw_bpm = double_bpm;
        }
    }
    if raw_bpm >= 152.0 && raw_bpm / 3.0 >= 52.0 {
        let (low_r3, full_r3, _) = subpulse_metrics(refined_lag, 3);
        let c_fast = comb_at(refined_lag).max(1e-9);
        let comb_ratio_3 = comb_at(refined_lag * 3.0) / c_fast;
        if low_r3 >= 1.24 && full_r3 >= 1.12 && comb_ratio_3 >= 0.72 {
            refined_lag *= 3.0;
            raw_bpm /= 3.0;
        }
    }

    let _ = refined_lag;

    let base_center = (raw_bpm * 10.0).round() / 10.0;
    let mut best_coarse_bpm = raw_bpm;
    let mut best_coarse_phase = 0.0_f64;
    let mut best_coarse_weighted = -1.0_f64;

    // Coarse 0.1 BPM sweep (+/- 2.5 BPM), then narrow 0.025 BPM refinement (+/- 0.15 BPM)
    for step_idx in -25i32..=25i32 {
        let cand_bpm = (base_center + step_idx as f64 * 0.1).clamp(40.0, 220.0);
        let (mean_score, phase) = score_bpm_phase(cand_bpm);
        let int_dist = (cand_bpm - cand_bpm.round()).abs();
        let int_bonus = if int_dist < 1e-4 {
            1.008
        } else if (cand_bpm * 2.0 - (cand_bpm * 2.0).round()).abs() < 1e-4 {
            1.003
        } else {
            1.0
        };
        let weighted = mean_score * int_bonus;
        if weighted > best_coarse_weighted {
            best_coarse_weighted = weighted;
            best_coarse_bpm = cand_bpm;
            best_coarse_phase = phase;
        }
    }

    let mut best_fine_bpm = best_coarse_bpm;
    let mut best_phase = best_coarse_phase;
    let mut best_weighted_grid = best_coarse_weighted;
    for step_idx in -6i32..=6i32 {
        if step_idx == 0 {
            continue;
        }
        let cand_bpm = (best_coarse_bpm + step_idx as f64 * 0.025).clamp(40.0, 220.0);
        let (mean_score, phase) = score_bpm_phase(cand_bpm);
        let int_dist = (cand_bpm - cand_bpm.round()).abs();
        let int_bonus = if int_dist < 1e-4 {
            1.008
        } else if (cand_bpm * 2.0 - (cand_bpm * 2.0).round()).abs() < 1e-4 {
            1.003
        } else {
            1.0
        };
        let weighted = mean_score * int_bonus;
        if weighted > best_weighted_grid {
            best_weighted_grid = weighted;
            best_fine_bpm = cand_bpm;
            best_phase = phase;
        }
    }

    let nearest_int = best_fine_bpm.round();
    let estimated_bpm = if (best_fine_bpm - nearest_int).abs() <= 0.18 {
        let (int_mean, int_phase) = score_bpm_phase(nearest_int);
        let (fine_mean, _) = score_bpm_phase(best_fine_bpm);
        if int_mean >= fine_mean * 0.97 {
            best_phase = int_phase;
            nearest_int
        } else {
            (best_fine_bpm * 10.0).round() / 10.0
        }
    } else {
        (best_fine_bpm * 10.0).round() / 10.0
    };

    let first_downbeat_sec = pick_bar_downbeat(
        &full_rms[..len],
        low_flux,
        full_flux,
        chroma_flux,
        env_hz,
        audio_start_idx,
        active_median,
        estimated_bpm,
        best_phase,
        bpb,
    );

    (estimated_bpm, first_downbeat_sec)
}

/// Computes zero-mean normalized linear autocovariance over `[min_lag..=max_lag]` in O(N log N) via FFT.
fn fft_autocorrelation_norm(signal: &[f32], min_lag: usize, max_lag: usize) -> Vec<f64> {
    let n = signal.len();
    if n == 0 {
        return vec![0.0; max_lag + 1];
    }
    let mean = signal.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
    let needed = (n + max_lag + 1).next_power_of_two().max((n * 2).next_power_of_two());
    let mut re = vec![0.0_f64; needed];
    let mut im = vec![0.0_f64; needed];
    let mut var_sum = 0.0_f64;
    for (i, &v) in signal.iter().enumerate() {
        let c = v as f64 - mean;
        re[i] = c;
        var_sum += c * c;
    }
    if var_sum <= 1e-12 {
        return vec![0.0; max_lag + 1];
    }
    fft_radix2(&mut re, &mut im);
    for k in 0..needed {
        re[k] = re[k] * re[k] + im[k] * im[k];
        im[k] = 0.0;
    }
    fft_radix2(&mut re, &mut im);

    let mut acf = vec![0.0_f64; max_lag + 1];
    let scale = needed as f64;
    let var0 = var_sum / n as f64;
    for lag in min_lag..=max_lag {
        let count = n.saturating_sub(lag).max(1) as f64;
        let cov = (re[lag] / scale) / count;
        acf[lag] = (cov / var0).clamp(-1.0, 1.0);
    }
    acf
}

#[allow(clippy::too_many_arguments)]
fn pick_bar_downbeat(
    full_rms: &[f32],
    low_flux: &[f32],
    full_flux: &[f32],
    chroma_flux: &[f32],
    env_hz: f64,
    audio_start_idx: usize,
    active_median: f32,
    estimated_bpm: f64,
    best_phase: f64,
    beats_per_bar: usize,
) -> f64 {
    let len = low_flux.len().min(full_flux.len()).min(full_rms.len());
    let bpb = beats_per_bar.clamp(1, 16);
    let beat_period = (60.0 / estimated_bpm.max(20.0)) * env_hz;
    let start_tol = (0.06 * env_hz).min(0.25 * beat_period).max(2.0);
    // Project backward by one beat in case `best_phase` wrapped near `beat_period` while beat 0 sits at `~0`.
    let mut grid_origin = best_phase;
    if grid_origin - beat_period >= audio_start_idx as f64 - start_tol {
        grid_origin -= beat_period;
    }
    let mut first_beat_k = 0usize;
    while (grid_origin + first_beat_k as f64 * beat_period) < audio_start_idx as f64 - start_tol {
        first_beat_k += 1;
    }

    let chroma_pad = (0.045 * env_hz).round().max(2.0) as usize;
    let rms_span = (0.08 * env_hz).round().max(2.0) as usize;
    let has_chroma_curve = chroma_flux.len() >= len;

    let mut chroma_means = vec![0.0_f64; bpb];
    let mut low_means = vec![0.0_f64; bpb];
    let mut full_means = vec![0.0_f64; bpb];
    let mut contrast_means = vec![0.0_f64; bpb];

    for m in 0..bpb {
        let mut c_sum = 0.0_f64;
        let mut l_sum = 0.0_f64;
        let mut f_sum = 0.0_f64;
        let mut k_sum = 0.0_f64;
        let mut count = 0usize;
        let mut bar_idx = 0usize;
        loop {
            let k = first_beat_k + m + bar_idx * bpb;
            let raw_center = grid_origin + k as f64 * beat_period;
            let center = raw_center.max(0.0).round() as usize;
            if center >= len {
                break;
            }
            let lo = center.saturating_sub(1);
            let hi = (center + 2).min(len);
            let low_hit = low_flux[lo..hi].iter().copied().fold(0.0_f32, f32::max);
            let full_hit = full_flux[lo..hi].iter().copied().fold(0.0_f32, f32::max);
            l_sum += low_hit as f64;
            f_sum += full_hit as f64;

            if has_chroma_curve {
                let c_lo = center.saturating_sub(chroma_pad);
                let c_hi = (center + chroma_pad + 1).min(len);
                let c_hit = chroma_flux[c_lo..c_hi]
                    .iter()
                    .copied()
                    .fold(0.0_f32, f32::max);
                c_sum += c_hit as f64;
            }

            let post_hi = (center + rms_span).min(len);
            let post_peak = full_rms[center..post_hi]
                .iter()
                .copied()
                .fold(0.0_f32, f32::max);
            let pre_lo = center.saturating_sub(rms_span);
            let pre_min = if pre_lo < center {
                full_rms[pre_lo..center]
                    .iter()
                    .copied()
                    .fold(f32::MAX, f32::min)
            } else {
                0.0
            };
            let contrast = (post_peak - 0.7 * pre_min).max(0.0);
            k_sum += contrast as f64;

            count += 1;
            bar_idx += 1;
        }
        let denom = count.max(1) as f64;
        chroma_means[m] = c_sum / denom;
        low_means[m] = l_sum / denom;
        full_means[m] = f_sum / denom;
        contrast_means[m] = k_sum / denom;
    }

    let sum_chroma: f64 = chroma_means.iter().sum();
    let sum_low: f64 = low_means.iter().sum();
    let sum_full: f64 = full_means.iter().sum();
    let sum_contrast: f64 = contrast_means.iter().sum();
    let use_chroma = has_chroma_curve && sum_chroma > 1e-6;

    let first_center = (grid_origin + first_beat_k as f64 * beat_period)
        .max(0.0)
        .round() as usize;
    let first_has_energy = first_center < len && {
        let hi = (first_center + rms_span).min(len);
        let peak = full_rms[first_center..hi]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);
        peak >= active_median * 0.25
    };

    let mut best_bar_offset = 0usize;
    let mut best_bar_score = -1.0_f64;
    for m in 0..bpb {
        let n_low = if sum_low > 1e-9 { low_means[m] / sum_low } else { 0.0 };
        let n_full = if sum_full > 1e-9 { full_means[m] / sum_full } else { 0.0 };
        let n_con = if sum_contrast > 1e-9 { contrast_means[m] / sum_contrast } else { 0.0 };
        let n_chr = if use_chroma { chroma_means[m] / sum_chroma } else { 0.0 };

        let base_score = if use_chroma {
            0.45 * n_chr + 0.35 * n_low + 0.10 * n_full + 0.10 * n_con
        } else {
            0.60 * n_low + 0.25 * n_full + 0.15 * n_con
        };
        let start_bonus = if m == 0 && first_has_energy { 1.015 } else { 1.0 };
        let weighted_bar = base_score * start_bonus;
        if weighted_bar > best_bar_score {
            best_bar_score = weighted_bar;
            best_bar_offset = m;
        }
    }

    let downbeat_frame = (grid_origin + (first_beat_k + best_bar_offset) as f64 * beat_period).max(0.0);
    (((downbeat_frame / env_hz) * 1000.0).round() / 1000.0).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::TAU;

    fn write_click_fixture_wav(
        path: &Path,
        bpm: f64,
        first_downbeat_sec: f64,
        duration_sec: f64,
        sample_rate: u32,
    ) {
        let total_frames = (duration_sec * sample_rate as f64) as usize;
        let start_frame = (first_downbeat_sec * sample_rate as f64).round() as usize;
        let beat_interval = (60.0 / bpm) * sample_rate as f64;
        let mut pcm_bytes = Vec::with_capacity(total_frames * 2);
        for frame in 0..total_frames {
            let sample = if frame >= start_frame {
                let rel = (frame - start_frame) as f64;
                let beat_num = (rel / beat_interval).floor() as usize;
                let phase_in_beat = rel % beat_interval;
                let is_downbeat = beat_num % 4 == 0;
                if phase_in_beat < (0.045 * sample_rate as f64) {
                    let env = (1.0 - phase_in_beat / (0.045 * sample_rate as f64)) as f32;
                    let freq = if is_downbeat { 95.0 } else { 150.0 };
                    let amp = if is_downbeat { 0.9 } else { 0.6 };
                    let tone = (TAU * freq * (phase_in_beat as f32 / sample_rate as f32)).sin();
                    tone * env * amp
                } else {
                    0.0
                }
            } else {
                0.0
            };
            let s16 = (sample.clamp(-1.0, 1.0) * 32767.0) as i16;
            pcm_bytes.extend_from_slice(&s16.to_le_bytes());
        }

        let data_len = pcm_bytes.len() as u32;
        let mut wav = Vec::with_capacity(44 + pcm_bytes.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&pcm_bytes);

        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn test_analyze_audio_peaks_and_bpm_estimation() {
        let dir = std::env::temp_dir().join("psalm_stage_test_analysis");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let wav_path = dir.join("beat_120.wav");
        write_click_fixture_wav(&wav_path, 120.0, 0.0, 12.0, 44100);

        let result = analyze_audio_file(&wav_path, 256).unwrap();
        assert_eq!(result.peaks.len(), 256);
        assert!((result.duration_sec - 12.0).abs() < 0.05);
        assert!(result.peaks.iter().all(|p| p[0] >= -1.0 && p[1] <= 1.0 && p[0] <= p[1]));
        assert!(
            (result.estimated_bpm - 120.0).abs() <= 1.0,
            "Expected ~120 BPM, got {}",
            result.estimated_bpm
        );

        // Also test 74 BPM with 1.25s silence intro (downbeat lock)
        let worship_wav = dir.join("worship_74.wav");
        write_click_fixture_wav(&worship_wav, 74.0, 1.25, 16.0, 44100);
        let worship_res = analyze_audio_file(&worship_wav, 256).unwrap();
        assert!(
            (worship_res.estimated_bpm - 74.0).abs() <= 1.0,
            "Expected ~74 BPM, got {}",
            worship_res.estimated_bpm
        );
        assert!(
            (worship_res.estimated_first_downbeat_sec - 1.25).abs() <= 0.15,
            "Expected first downbeat ~1.25s, got {}",
            worship_res.estimated_first_downbeat_sec
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_subdivided_eighth_note_groove_avoids_double_tempo() {
        let sample_rate = 44_100u32;
        let bpm = 74.0_f64;
        let beat_sec = 60.0 / bpm;
        let eighth_sec = beat_sec / 2.0;
        let duration_sec = 18.0_f64;
        let total_frames = (duration_sec * sample_rate as f64) as usize;

        let dir = std::env::temp_dir().join("psalm_stage_test_octave");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let groove_path = dir.join("subdivided_74.wav");

        let mut pcm_bytes = Vec::with_capacity(total_frames * 2);
        let beat_frames = beat_sec * sample_rate as f64;
        let eighth_frames = eighth_sec * sample_rate as f64;

        for frame in 0..total_frames {
            let f = frame as f64;
            let phase_beat = f % beat_frames;
            let phase_eighth = f % eighth_frames;
            let t = f as f32 / sample_rate as f32;

            // Strong kick/low pulse on quarter-note beats (74 BPM)
            let beat_hit = if phase_beat < 0.055 * sample_rate as f64 {
                let env = (1.0 - phase_beat / (0.055 * sample_rate as f64)) as f32;
                (TAU * 75.0 * t).sin() * env * 0.85
            } else {
                0.0
            };

            // Loud high-frequency hi-hat click on every eighth note (148 BPM)
            let hat_hit = if phase_eighth < 0.020 * sample_rate as f64 {
                let env = (1.0 - phase_eighth / (0.020 * sample_rate as f64)) as f32;
                (TAU * 4200.0 * t).sin() * env * 0.45
            } else {
                0.0
            };

            let sample = (beat_hit + hat_hit).clamp(-1.0, 1.0);
            let s16 = (sample * 32767.0) as i16;
            pcm_bytes.extend_from_slice(&s16.to_le_bytes());
        }

        let data_len = pcm_bytes.len() as u32;
        let mut wav = Vec::with_capacity(44 + pcm_bytes.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&pcm_bytes);
        std::fs::write(&groove_path, wav).unwrap();

        let result = analyze_audio_file(&groove_path, 256).unwrap();
        assert!(
            (result.estimated_bpm - 74.0).abs() <= 1.0,
            "Expected ~74 BPM instead of 2x (148), got {}",
            result.estimated_bpm
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn write_mono_wav_samples(path: &Path, sample_rate: u32, samples: &[f32]) {
        let mut pcm_bytes = Vec::with_capacity(samples.len() * 2);
        for &s in samples {
            let s16 = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
            pcm_bytes.extend_from_slice(&s16.to_le_bytes());
        }
        let data_len = pcm_bytes.len() as u32;
        let mut wav = Vec::with_capacity(44 + pcm_bytes.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2u16.to_le_bytes());
        wav.extend_from_slice(&16u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&pcm_bytes);
        std::fs::write(path, wav).unwrap();
    }

    fn chord_freqs(midi_notes: &[i32]) -> Vec<f32> {
        midi_notes
            .iter()
            .map(|&m| 440.0 * 2.0_f32.powf((m - 69) as f32 / 12.0))
            .collect()
    }

    #[test]
    fn test_mixdown_detects_key_of_g_and_drumless_intro_downbeat() {
        let sample_rate = 44_100u32;
        let bpm = 90.0_f64;
        let beat_sec = 60.0 / bpm; // 0.6667s
        let duration_sec = 32.0_f64;
        let total_frames = (duration_sec * sample_rate as f64) as usize;

        let dir = std::env::temp_dir().join("psalm_stage_test_mixdown_g_major");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Piano & Bass stems play G - D - Em - C progression from t = 0.0s (key of G!)
        // MIDI G=43/55/59/62, D=50/54/57/62, Em=40/52/55/59, C=48/55/60/64
        let chords = [
            chord_freqs(&[43, 55, 59, 62]), // G
            chord_freqs(&[50, 54, 57, 62]), // D
            chord_freqs(&[40, 52, 55, 59]), // Em
            chord_freqs(&[48, 55, 60, 64]), // C
        ];

        let mut piano = vec![0.0_f32; total_frames];
        let mut bass = vec![0.0_f32; total_frames];
        let mut drums = vec![0.0_f32; total_frames];

        let beat_frames = beat_sec * sample_rate as f64;
        for f in 0..total_frames {
            let t = f as f32 / sample_rate as f32;
            let beat_idx = (f as f64 / beat_frames).floor() as usize;
            let bar_idx = beat_idx / 4;
            let beat_in_bar = beat_idx % 4;
            let phase_in_beat = (f as f64) % beat_frames;
            let chord = &chords[bar_idx % chords.len()];

            let p_env = (-(phase_in_beat as f32 / sample_rate as f32) * 3.5).exp();
            let p_sig: f32 = chord
                .iter()
                .skip(1)
                .map(|&hz| (TAU as f32 * hz * t).sin() * 0.22)
                .sum();
            piano[f] = p_sig * p_env;

            let b_env = (-(phase_in_beat as f32 / sample_rate as f32) * 2.8).exp();
            bass[f] = (TAU as f32 * chord[0] * t).sin() * 0.45 * b_env;

            // Drums stay completely silent during the 16-second intro (first 24 beats)
            if t >= 16.0 && phase_in_beat < (0.05 * sample_rate as f64) {
                let d_env = (1.0 - phase_in_beat / (0.05 * sample_rate as f64)) as f32;
                let freq = if beat_in_bar == 0 || beat_in_bar == 2 {
                    75.0
                } else {
                    240.0
                };
                drums[f] = (TAU as f32 * freq * t).sin() * d_env * 0.85;
            }
        }

        write_mono_wav_samples(&dir.join("drums.wav"), sample_rate, &drums);
        write_mono_wav_samples(&dir.join("bass.wav"), sample_rate, &bass);
        write_mono_wav_samples(&dir.join("piano.wav"), sample_rate, &piano);

        let res = analyze_song_folder_mixdown(&dir, None, 256).unwrap();
        assert!(
            (res.estimated_bpm - 90.0).abs() <= 0.5,
            "Expected ~90 BPM, got {}",
            res.estimated_bpm
        );
        assert!(
            res.estimated_first_downbeat_sec <= 0.5,
            "Downbeat must lock onto Bar 1 piano intro (~0.0s), NOT skip to 16s drums! Got {}",
            res.estimated_first_downbeat_sec
        );
        assert_eq!(
            res.estimated_key.as_deref(),
            Some("G"),
            "Expected confident key G (conf={}, cands={:?})",
            res.key_confidence,
            res.key_candidates
        );
        assert!(res.key_confidence >= 0.50);

        // Unpitched drums-only analysis must abstain instead of returning a bogus "C"
        let drums_only = analyze_audio_file(&dir.join("drums.wav"), 256).unwrap();
        assert_eq!(drums_only.estimated_key, None);
        assert_eq!(drums_only.key_confidence, 0.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_fast_rock_groove_140_bpm_not_halved_and_120_exact() {
        let sample_rate = 44_100u32;
        let dir = std::env::temp_dir().join("psalm_stage_test_140_120");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // 140 BPM kick (beat 1 & 3) + snare (beat 2 & 4)
        let bpm_fast = 140.0_f64;
        let beat_frames_fast = (60.0 / bpm_fast) * sample_rate as f64;
        let total_fast = (24.0 * sample_rate as f64) as usize;
        let mut fast = vec![0.0_f32; total_fast];
        for f in 0..total_fast {
            let t = f as f32 / sample_rate as f32;
            let beat_idx = (f as f64 / beat_frames_fast).floor() as usize;
            let ph = (f as f64) % beat_frames_fast;
            if ph < (0.045 * sample_rate as f64) {
                let env = (1.0 - ph / (0.045 * sample_rate as f64)) as f32;
                fast[f] = if beat_idx % 2 == 0 {
                    (TAU as f32 * 80.0 * t).sin() * env * 0.85
                } else {
                    (TAU as f32 * 320.0 * t).sin() * env * 0.78
                };
            }
        }
        let fast_path = dir.join("drums_140.wav");
        write_mono_wav_samples(&fast_path, sample_rate, &fast);
        let fast_res = analyze_audio_file(&fast_path, 256).unwrap();
        assert!(
            (fast_res.estimated_bpm - 140.0).abs() <= 1.0,
            "Expected ~140 BPM rock groove instead of 70 BPM half-time, got {}",
            fast_res.estimated_bpm
        );

        // Long 60s 120.0 BPM pulse should snap cleanly to exact 120.0 (not 120.3)
        let exact_path = dir.join("exact_120.wav");
        write_click_fixture_wav(&exact_path, 120.0, 0.0, 60.0, sample_rate);
        let exact_res = analyze_audio_file(&exact_path, 256).unwrap();
        assert_eq!(exact_res.estimated_bpm, 120.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ambiguous_atonal_audio_abstains_honestly() {
        let sample_rate = 44_100u32;
        let dir = std::env::temp_dir().join("psalm_stage_test_key_abstain");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        // Chromatic cluster hitting all 12 pitch classes equally -> should NOT guess a key!
        let total = (8.0 * sample_rate as f64) as usize;
        let mut cluster = vec![0.0_f32; total];
        for f in 0..total {
            let t = f as f32 / sample_rate as f32;
            let mut sum = 0.0_f32;
            for semi in 0..12 {
                let freq = 261.63 * 2.0_f32.powf(semi as f32 / 12.0);
                sum += (TAU as f32 * freq * t).sin() * 0.06;
            }
            cluster[f] = sum;
        }
        let path = dir.join("music_chromatic.wav");
        write_mono_wav_samples(&path, sample_rate, &cluster);
        let res = analyze_audio_file(&path, 128).unwrap();
        assert_eq!(
            res.estimated_key, None,
            "Chromatic cluster must abstain instead of hallucinating a key (got {:?}, conf={})",
            res.estimated_key, res.key_confidence
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_downbeat_prefers_chord_change_beat_one_and_hint_bpm_alignment() {
        let sample_rate = 44_100u32;
        let dir = std::env::temp_dir().join("psalm_stage_test_downbeat_chroma");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let bpm = 80.0_f64; // 0.75s per beat, 3.0s per bar
        let spb = 60.0 / bpm;
        let pickup_sec = 0.62_f64;
        let downbeat_sec = pickup_sec + spb; // 1.37s (Bar 1 Beat 1 starts 1 beat AFTER a Beat-4 pickup hit!)
        let duration_sec = 22.0_f64;
        let total = (duration_sec * sample_rate as f64) as usize;

        let mut drums = vec![0.0_f32; total];
        let mut keys = vec![0.0_f32; total];

        // 8-bar progression clearly tonicized in G major, changing on Beat 1 of each bar
        let chords: [[f32; 3]; 8] = [
            [196.00, 246.94, 293.66], // G
            [293.66, 369.99, 440.00], // D
            [164.81, 196.00, 246.94], // Em
            [261.63, 329.63, 392.00], // C
            [196.00, 246.94, 293.66], // G
            [293.66, 369.99, 440.00], // D
            [261.63, 329.63, 392.00], // C
            [196.00, 246.94, 293.66], // G
        ];

        let pickup_frame = (pickup_sec * sample_rate as f64).round() as usize;
        let downbeat_frame = (downbeat_sec * sample_rate as f64).round() as usize;
        let spb_frames = spb * sample_rate as f64;

        // Pickup beat-4 transient right before Bar 1 Beat 1 (must NOT trick downbeat onto 0.62s)
        for f in pickup_frame..(pickup_frame + (0.045 * sample_rate as f64) as usize).min(total) {
            let t = f as f32 / sample_rate as f32;
            let env = 1.0 - (f - pickup_frame) as f32 / (0.045 * sample_rate as f32);
            drums[f] = (TAU as f32 * 420.0 * t).sin() * env * 0.65;
        }

        for f in downbeat_frame..total {
            let t = f as f32 / sample_rate as f32;
            let rel_frames = (f - downbeat_frame) as f64;
            let beat_idx = (rel_frames / spb_frames).floor() as usize;
            let beat_in_bar = beat_idx % 4;
            let bar_idx = beat_idx / 4;
            let ph = rel_frames % spb_frames;

            // Drums: snare on 2 & 4 louder full-band, Beat 3 kick slightly heavier than Beat 1 kick
            if ph < 0.055 * sample_rate as f64 {
                let env = (1.0 - ph / (0.055 * sample_rate as f64)) as f32;
                drums[f] = match beat_in_bar {
                    0 => (TAU as f32 * 72.0 * t).sin() * env * 0.50,
                    2 => (TAU as f32 * 72.0 * t).sin() * env * 0.56,
                    _ => {
                        ((TAU as f32 * 340.0 * t).sin() + 0.6 * (TAU as f32 * 910.0 * t).sin())
                            * env
                            * 0.78
                    }
                };
            }

            // Keys hold one chord per bar -> chord transitions define Beat 1!
            let chord = chords[bar_idx % chords.len()];
            let bar_pos = rel_frames % (4.0 * spb_frames);
            let chord_env = if bar_pos < 0.03 * sample_rate as f64 {
                (bar_pos / (0.03 * sample_rate as f64)) as f32
            } else {
                0.82_f32
            };
            keys[f] = chord
                .iter()
                .map(|freq| (TAU as f32 * *freq * t).sin())
                .sum::<f32>()
                * (chord_env * 0.14);
        }

        write_mono_wav_samples(&dir.join("drums.wav"), sample_rate, &drums);
        write_mono_wav_samples(&dir.join("keys.wav"), sample_rate, &keys);

        let drums_only = analyze_audio_file(&dir.join("drums.wav"), 256).unwrap();
        let res = analyze_song_folder_mixdown(&dir, None, 256).unwrap();
        assert!(
            (res.estimated_bpm - bpm).abs() <= 1.0,
            "Expected ~{bpm} BPM, got {} (drums_only={})",
            res.estimated_bpm,
            drums_only.estimated_bpm
        );
        assert!(
            (res.estimated_first_downbeat_sec - downbeat_sec).abs() <= 0.06,
            "Expected Beat 1 ~{downbeat_sec}s (harmonic bar change), got {}s",
            res.estimated_first_downbeat_sec
        );
        assert_eq!(res.estimated_key.as_deref(), Some("G"));

        // Re-align using explicit tapped BPM hint
        let hinted = analyze_song_folder_mixdown_with_timing(&dir, None, 256, Some(80.0), 4).unwrap();
        assert_eq!(hinted.estimated_bpm, 80.0);
        assert!(
            (hinted.estimated_first_downbeat_sec - downbeat_sec).abs() <= 0.06,
            "Hinted BPM expected Beat 1 ~{downbeat_sec}s, got {}s",
            hinted.estimated_first_downbeat_sec
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_detuned_hpcp_detects_key_of_g_at_plus_30_cents() {
        let sample_rate = 44_100u32;
        let dir = std::env::temp_dir().join("psalm_stage_test_detuned_hpcp");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let duration_sec = 10.0_f64;
        let total = (duration_sec * sample_rate as f64) as usize;
        // Shift frequencies by +30 cents (2^(30/1200) ~= 1.01748) relative to A4=440Hz
        let detune = 2.0_f32.powf(30.0 / 1200.0);
        let chords: [[f32; 3]; 4] = [
            [196.00, 246.94, 293.66], // G
            [261.63, 329.63, 392.00], // C
            [293.66, 369.99, 440.00], // D
            [196.00, 246.94, 293.66], // G
        ];
        let mut samples = vec![0.0_f32; total];
        for (i, out) in samples.iter_mut().enumerate() {
            let t = i as f32 / sample_rate as f32;
            let bar = ((t / 2.5) as usize) % chords.len();
            let chord = chords[bar];
            *out = chord
                .iter()
                .map(|f| (TAU as f32 * (*f * detune) * t).sin())
                .sum::<f32>()
                * 0.16;
        }
        let path = dir.join("music.wav");
        write_mono_wav_samples(&path, sample_rate, &samples);

        let res = analyze_audio_file(&path, 128).unwrap();
        assert_eq!(
            res.estimated_key.as_deref(),
            Some("G"),
            "Expected tuning-compensated HPCP to detect G major at +30 cents (got {:?}, candidates={:?})",
            res.estimated_key,
            res.key_candidates
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_sparse_vocal_bleed_stems_do_not_corrupt_music_and_drums() {
        let sample_rate = 44_100u32;
        let dir = std::env::temp_dir().join("psalm_stage_test_sparse_stems");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let bpm = 116.0_f64;
        let spb = 60.0 / bpm;
        let duration_sec = 18.0_f64;
        let total = (duration_sec * sample_rate as f64) as usize;
        let spb_frames = spb * sample_rate as f64;

        let mut drums = vec![0.0_f32; total];
        let mut music = vec![0.0_f32; total];
        let mut vocals = vec![0.0_f32; total];

        // D major progression: D - A - Bm - G
        let chords: [[f32; 3]; 4] = [
            [146.83, 185.00, 220.00], // D
            [220.00, 277.18, 329.63], // A
            [123.47, 146.83, 185.00], // Bm
            [196.00, 246.94, 293.66], // G
        ];

        for f in 0..total {
            let t = f as f32 / sample_rate as f32;
            let beat_idx = (f as f64 / spb_frames).floor() as usize;
            let ph = f as f64 % spb_frames;
            if ph < 0.045 * sample_rate as f64 {
                let env = (1.0 - ph / (0.045 * sample_rate as f64)) as f32;
                let kick = (TAU as f32 * 65.0 * t).sin() * env * 0.65;
                let snare = if beat_idx % 2 == 1 {
                    (TAU as f32 * 360.0 * t).sin() * env * 0.35
                } else {
                    0.0
                };
                drums[f] = kick + snare;
            }
            let bar = (beat_idx / 4) % chords.len();
            music[f] = chords[bar]
                .iter()
                .map(|freq| (TAU as f32 * *freq * t).sin())
                .sum::<f32>()
                * 0.16;

            // Sparse off-key/poly-syllabic vocal bursts + quiet room bleed across sparse gaps
            if (t >= 3.0 && t <= 5.0) || (t >= 11.0 && t <= 13.0) {
                vocals[f] = (TAU as f32 * 587.33 * t).sin() * 0.25;
            } else {
                // Very low room hum/bleed in quiet sections (below adaptive stem bleed gate)
                vocals[f] = (TAU as f32 * 50.0 * t).sin() * 0.0015;
            }
        }

        write_mono_wav_samples(&dir.join("drums.wav"), sample_rate, &drums);
        write_mono_wav_samples(&dir.join("music.wav"), sample_rate, &music);
        write_mono_wav_samples(&dir.join("vocals.wav"), sample_rate, &vocals);

        let res = analyze_song_folder_mixdown(&dir, None, 256).unwrap();
        assert!(
            (res.estimated_bpm - bpm).abs() <= 1.0,
            "Expected mixdown to follow steady drums+music ~{bpm} BPM, got {}",
            res.estimated_bpm
        );
        assert_eq!(res.estimated_key.as_deref(), Some("D"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_compressed_pad_plus_80_bpm_kick_snare_eighth_hats() {
        let sample_rate = 44_100u32;
        let dir = std::env::temp_dir().join("psalm_stage_test_compressed_pad_80");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let bpm = 80.0_f64; // 0.75s beat
        let beat_frames = (60.0 / bpm) * sample_rate as f64;
        let eighth_frames = beat_frames * 0.5;
        let duration_sec = 22.0_f64;
        let total = (duration_sec * sample_rate as f64) as usize;

        let mut mix = vec![0.0_f32; total];
        let chords: [[f32; 3]; 4] = [
            [196.00, 246.94, 293.66],
            [261.63, 329.63, 392.00],
            [293.66, 369.99, 440.00],
            [196.00, 246.94, 293.66],
        ];
        for (f, out) in mix.iter_mut().enumerate() {
            let t = f as f32 / sample_rate as f32;
            let beat_idx = (f as f64 / beat_frames).floor() as usize;
            let ph_beat = f as f64 % beat_frames;
            let ph_eighth = f as f64 % eighth_frames;
            let bar = (beat_idx / 4) % chords.len();

            // Heavy sustained pad keeps overall waveform RMS nearly constant (simulates mastering limiter)
            let pad: f32 = chords[bar]
                .iter()
                .map(|freq| (TAU as f32 * *freq * t).sin())
                .sum::<f32>()
                * 0.22;

            let drum = if ph_beat < 0.045 * sample_rate as f64 {
                let env = (1.0 - ph_beat / (0.045 * sample_rate as f64)) as f32;
                if beat_idx % 2 == 0 {
                    (TAU as f32 * 65.0 * t).sin() * env * 0.45
                } else {
                    ((TAU as f32 * 240.0 * t).sin() + 0.5 * (TAU as f32 * 850.0 * t).sin())
                        * env
                        * 0.38
                }
            } else {
                0.0
            };

            let hat = if ph_eighth < 0.015 * sample_rate as f64 {
                let env = (1.0 - ph_eighth / (0.015 * sample_rate as f64)) as f32;
                (TAU as f32 * 5200.0 * t).sin() * env * 0.22
            } else {
                0.0
            };

            // Soft limiter compression mimics mastered Pop/Worship track
            let raw = pad + drum + hat;
            *out = (raw * 1.8).tanh() * 0.65;
        }

        let path = dir.join("mastered_80.wav");
        write_mono_wav_samples(&path, sample_rate, &mix);
        let res = analyze_audio_file(&path, 256).unwrap();

        assert!(
            (res.estimated_bpm - 80.0).abs() <= 1.0,
            "Expected ~80 BPM even under limiting + 8th hats, got {}",
            res.estimated_bpm
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ambient_intro_then_128_bpm_four_on_floor_chorus() {
        let sample_rate = 44_100u32;
        let dir = std::env::temp_dir().join("psalm_stage_test_ambient_intro_128");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let bpm = 128.0_f64;
        let beat_frames = (60.0 / bpm) * sample_rate as f64;
        let duration_sec = 22.0_f64;
        let intro_end = (8.0 * sample_rate as f64) as usize;
        let total = (duration_sec * sample_rate as f64) as usize;

        let mut mix = vec![0.0_f32; total];
        for (f, out) in mix.iter_mut().enumerate() {
            let t = f as f32 / sample_rate as f32;
            let swell = (TAU as f32 * 220.0 * t).sin() * 0.14
                + (TAU as f32 * 277.18 * t).sin() * 0.12;
            if f < intro_end {
                *out = swell * 0.6;
            } else {
                let rel = (f - intro_end) as f64;
                let beat_idx = (rel / beat_frames).floor() as usize;
                let ph = rel % beat_frames;
                let hit = if ph < 0.045 * sample_rate as f64 {
                    let env = (1.0 - ph / (0.045 * sample_rate as f64)) as f32;
                    let kick = (TAU as f32 * 64.0 * t).sin() * env * 0.65;
                    let snare = if beat_idx % 2 == 1 {
                        (TAU as f32 * 310.0 * t).sin() * env * 0.55
                    } else {
                        0.0
                    };
                    kick + snare
                } else {
                    0.0
                };
                *out = (swell + hit).clamp(-1.0, 1.0);
            }
        }

        let path = dir.join("intro_then_128.wav");
        write_mono_wav_samples(&path, sample_rate, &mix);
        let res = analyze_audio_file(&path, 256).unwrap();

        assert!(
            (res.estimated_bpm - 128.0).abs() <= 1.0,
            "Expected ~128 BPM instead of ambient intro drift or 64 BPM half-time, got {}",
            res.estimated_bpm
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
