use std::collections::HashMap;
use std::f32::consts::TAU;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::loader::{decode_audio_to_stereo, save_song_manifest};
use crate::manifest::{ClickMode, SectionDef, SongManifest, StemDef};
use crate::percussion::{
    LoadedPercSample, PercussionEngine, PercussionSampleKitData, DEFAULT_PERCUSSION_ROOM_MIX,
};
use crate::percussion_patterns::{
    CustomPercussionPattern, GrooveIntensity, HitVariant, PercussionInstrument, PercussionMode,
    PercussionStyle, ALL_PERCUSSION_INSTRUMENTS,
};
use crate::{AudioCoreError, Result};

const KIT_SAMPLE_RATE: u32 = 44_100;

pub fn default_percussion_kits_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Music").join("PsalmStage").join("PercussionKits")
}

pub fn default_percussion_loops_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Music").join("PsalmStage").join("PercussionLoops")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StudioKitProfile {
    #[default]
    StudioPraise,
    MakossaLive,
    AcousticCajon,
}

impl StudioKitProfile {
    pub fn id(self) -> &'static str {
        match self {
            Self::StudioPraise => "studio-praise-kit",
            Self::MakossaLive => "makossa-live-kit",
            Self::AcousticCajon => "acoustic-cajon-kit",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::StudioPraise => "Studio Praise Kit",
            Self::MakossaLive => "Makossa African Live Kit",
            Self::AcousticCajon => "Acoustic Cajon & Shakers Kit",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PercussionKitInfo {
    pub id: String,
    pub name: String,
    pub dir: String,
    pub sample_count: usize,
    pub instruments_covered: Vec<PercussionInstrument>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KitMetadataFile {
    id: String,
    name: String,
    profile: Option<StudioKitProfile>,
    sample_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportLoopBundleRequest {
    pub title: String,
    #[serde(default)]
    pub output_dir: Option<String>,
    pub bpm: f64,
    #[serde(default = "default_ts")]
    pub time_signature: [u8; 2],
    #[serde(default = "default_bars")]
    pub bars: u32,
    #[serde(default = "default_true_flag")]
    pub dynamic_arrangement: bool,
    #[serde(default)]
    pub intensity: GrooveIntensity,
    #[serde(default = "default_room")]
    pub room_mix: f32,
    #[serde(default)]
    pub style: PercussionStyle,
    #[serde(default)]
    pub custom_pattern: Option<CustomPercussionPattern>,
    #[serde(default)]
    pub kit_dir: Option<String>,
}

fn default_ts() -> [u8; 2] {
    [4, 4]
}

fn default_bars() -> u32 {
    16
}

fn default_true_flag() -> bool {
    true
}

fn default_room() -> f32 {
    DEFAULT_PERCUSSION_ROOM_MIX
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportLoopBundleResult {
    pub song_dir: String,
    pub title: String,
    pub bpm: f64,
    pub duration_sec: f64,
    pub stem_files: Vec<String>,
}

fn sanitize_slug(input: &str, fallback: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_dash = false;
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        fallback.to_string()
    } else {
        out
    }
}

/// Writes an interleaved stereo 16-bit PCM WAV file cleanly with RIFF header.
pub fn write_stereo_wav_16(path: &Path, sample_rate: u32, left: &[f32], right: &[f32]) -> Result<()> {
    let frames = left.len().min(right.len());
    let data_bytes = (frames as u32).saturating_mul(4);
    let riff_size = 36u32.saturating_add(data_bytes);
    let byte_rate = sample_rate.saturating_mul(4);

    let file = File::create(path)?;
    let mut w = BufWriter::new(file);
    w.write_all(b"RIFF")?;
    w.write_all(&riff_size.to_le_bytes())?;
    w.write_all(b"WAVE")?;
    w.write_all(b"fmt ")?;
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?; // PCM
    w.write_all(&2u16.to_le_bytes())?; // Stereo
    w.write_all(&sample_rate.to_le_bytes())?;
    w.write_all(&byte_rate.to_le_bytes())?;
    w.write_all(&4u16.to_le_bytes())?; // Block align
    w.write_all(&16u16.to_le_bytes())?; // Bits per sample
    w.write_all(b"data")?;
    w.write_all(&data_bytes.to_le_bytes())?;

    for i in 0..frames {
        let sl = (left[i].clamp(-1.0, 1.0) * 32767.0).round() as i16;
        let sr = (right[i].clamp(-1.0, 1.0) * 32767.0).round() as i16;
        w.write_all(&sl.to_le_bytes())?;
        w.write_all(&sr.to_le_bytes())?;
    }
    w.flush()?;
    Ok(())
}

struct XorRng(u32);
impl XorRng {
    fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x.max(1);
        ((x as i32) as f32) / (i32::MAX as f32)
    }
}

/// Offline high-resolution studio percussion synthesizer for generating rich WAV sample kits.
fn synthesize_studio_one_shot(
    profile: StudioKitProfile,
    instrument: PercussionInstrument,
    variant: HitVariant,
    sr: u32,
) -> (Vec<f32>, Vec<f32>) {
    let dur_sec = match (instrument, variant) {
        (PercussionInstrument::Kick, _) => 0.38,
        (PercussionInstrument::Snare, HitVariant::Soft) => 0.14,
        (PercussionInstrument::Snare, _) => 0.28,
        (PercussionInstrument::HiHat, HitVariant::Accent) => 0.36,
        (PercussionInstrument::HiHat, _) => 0.09,
        (PercussionInstrument::HandDrums, HitVariant::Special) => 0.44,
        (PercussionInstrument::HandDrums, _) => 0.34,
        (PercussionInstrument::Tambourine, HitVariant::Accent) => 0.28,
        (PercussionInstrument::Tambourine, _) => 0.15,
        (PercussionInstrument::Shaker, _) => 0.11,
        (PercussionInstrument::Claps, _) => 0.32,
        (PercussionInstrument::Clave, _) => 0.09,
        (PercussionInstrument::Bell, _) => 0.28,
    };
    let n_frames = (dur_sec * sr as f32) as usize;
    let mut left = vec![0.0_f32; n_frames];
    let mut right = vec![0.0_f32; n_frames];
    let inv_sr = 1.0 / sr as f32;
    let mut rng = XorRng(
        0x51A7_9001u32
            .wrapping_add((instrument.index() as u32) * 131_071)
            .wrapping_add((variant.index() as u32) * 8_191),
    );

    let is_makossa = profile == StudioKitProfile::MakossaLive;
    let is_acoustic = profile == StudioKitProfile::AcousticCajon;

    let mut p1 = 0.0_f32;
    let mut p2 = 0.0_f32;
    let mut p3 = 0.0_f32;
    let mut bp_l = 0.0_f32;
    let mut bp_r = 0.0_f32;
    let mut hp_l = 0.0_f32;
    let mut hp_r = 0.0_f32;
    let mut lp_l = 0.0_f32;
    let mut lp_r = 0.0_f32;

    for i in 0..n_frames {
        let t = i as f32 * inv_sr;
        let (sl, sr_samp) = match instrument {
            PercussionInstrument::Kick => {
                // Multi-layer studio kick: tight sub + punchy shell + beater transient + warm drive
                let punch_hz = if is_makossa { 185.0 } else if is_acoustic { 140.0 } else { 168.0 };
                let sub_hz = if variant == HitVariant::Soft { 52.0 } else { 47.0 };
                let f_sweep = sub_hz + (punch_hz - sub_hz) * (-t * 46.0).exp();
                p1 = (p1 + f_sweep * inv_sr).fract();
                p2 = (p2 + 108.0 * inv_sr).fract();
                p3 = (p3 + 1750.0 * inv_sr).fract();

                let sub = (p1 * TAU).sin() * (-t * if is_makossa { 11.5 } else { 13.0 }).exp();
                let shell = (p2 * TAU).sin() * (-t * 30.0).exp() * 0.36;
                let n = rng.next_f32();
                lp_l = lp_l * 0.65 + n * 0.35;
                let click = ((p3 * TAU).sin() * 0.65 + lp_l * 0.35)
                    * (-t * 260.0).exp()
                    * if variant == HitVariant::Soft { 0.16 } else { 0.36 };
                let s = ((sub + shell) * 1.18 + click).tanh() * 0.94;
                (s, s)
            }
            PercussionInstrument::Snare => {
                if variant == HitVariant::Soft && !is_acoustic {
                    // Warm crisp cross-stick
                    p1 = (p1 + 885.0 * inv_sr).fract();
                    p2 = (p2 + 1620.0 * inv_sr).fract();
                    let s = ((p1 * TAU).sin() * 0.68 + (p2 * TAU).sin() * 0.32)
                        * (-t * 62.0).exp()
                        * 0.82;
                    (s, s * 0.96)
                } else if is_acoustic {
                    // Rich wooden cajon slap with stereo box room
                    let f = 148.0 + 105.0 * (-t * 54.0).exp();
                    p1 = (p1 + f * inv_sr).fract();
                    let box_tone = (p1 * TAU).sin() * (-t * 21.0).exp() * 0.72;
                    let nl = rng.next_f32();
                    let nr = rng.next_f32();
                    lp_l = lp_l * 0.72 + nl * 0.28;
                    lp_r = lp_r * 0.72 + nr * 0.28;
                    let slap_l = (nl - lp_l) * (-t * 32.0).exp() * 0.58;
                    let slap_r = (nr - lp_r) * (-t * 32.0).exp() * 0.58;
                    ((box_tone + slap_l).tanh() * 0.88, (box_tone + slap_r).tanh() * 0.88)
                } else {
                    // Studio acoustic snare: dual head/shell + stereo wire sizzle + optional rimshot
                    let f1 = 195.0 + 82.0 * (-t * 80.0).exp();
                    let f2 = 338.0;
                    p1 = (p1 + f1 * inv_sr).fract();
                    p2 = (p2 + f2 * inv_sr).fract();
                    p3 = (p3 + 1290.0 * inv_sr).fract();
                    let shell = ((p1 * TAU).sin() * 0.66 + (p2 * TAU).sin() * 0.34) * (-t * 24.0).exp();
                    let rim = if variant == HitVariant::Accent {
                        (p3 * TAU).sin() * (-t * 50.0).exp() * 0.36
                    } else {
                        0.0
                    };
                    let nl = rng.next_f32();
                    let nr = rng.next_f32();
                    let wire_l = nl - hp_l * 0.85;
                    hp_l = nl;
                    let wire_r = nr - hp_r * 0.85;
                    hp_r = nr;
                    let w_env = (-t * 17.5).exp();
                    let l = (shell * 0.55 + rim + wire_l * w_env * 0.68).tanh() * 0.90;
                    let r = (shell * 0.55 + rim + wire_r * w_env * 0.68).tanh() * 0.90;
                    (l, r)
                }
            }
            PercussionInstrument::HiHat => {
                let metal_freqs = [205.0, 304.0, 370.0, 523.0, 540.0, 800.0];
                let mut m = 0.0_f32;
                for (k, mf) in metal_freqs.iter().enumerate() {
                    let ph = (t * (*mf) + k as f32 * 0.13).fract();
                    m += if ph < 0.5 { 1.0 } else { -1.0 };
                }
                let nl = m * 0.13 + rng.next_f32() * 0.44;
                let nr = m * 0.13 + rng.next_f32() * 0.44;
                let h1l = nl - hp_l * 0.90;
                hp_l = nl;
                let h1r = nr - hp_r * 0.90;
                hp_r = nr;
                let h2l = h1l - lp_l * 0.86;
                lp_l = h1l;
                let h2r = h1r - lp_r * 0.86;
                lp_r = h1r;
                let decay = if variant == HitVariant::Accent { 12.0 } else { 54.0 };
                let env = (-t * decay).exp() * (t / 0.0015).min(1.0);
                (h2l * env * 0.64, h2r * env * 0.64)
            }
            PercussionInstrument::HandDrums => {
                if variant == HitVariant::Special && !is_acoustic {
                    // Authentic Talking Drum scoop (Makossa/Nigerian praise highlight!)
                    let glide = 1.0 - (-t * 16.0).exp();
                    let f = 148.0 + 148.0 * glide;
                    p1 = (p1 + f * inv_sr).fract();
                    p2 = (p2 + f * 1.56 * inv_sr).fract();
                    p3 = (p3 + f * 2.13 * inv_sr).fract();
                    let body = (p1 * TAU).sin() * 0.72
                        + (p2 * TAU).sin() * 0.24
                        + (p3 * TAU).sin() * 0.11;
                    let skin = rng.next_f32() * (-t * 115.0).exp() * 0.20;
                    let s = (body * (-t * 10.0).exp() + skin).tanh() * 0.90;
                    (s, s * 0.96)
                } else {
                    let base = match variant {
                        HitVariant::Accent => 272.0,
                        HitVariant::Soft => 158.0,
                        _ => 214.0,
                    };
                    let f = base + 96.0 * (-t * 68.0).exp();
                    p1 = (p1 + f * inv_sr).fract();
                    p2 = (p2 + f * 1.56 * inv_sr).fract();
                    p3 = (p3 + f * 2.13 * inv_sr).fract();
                    let body = ((p1 * TAU).sin() * 0.72
                        + (p2 * TAU).sin() * 0.22
                        + (p3 * TAU).sin() * 0.12)
                        * (-t * 13.5).exp();
                    let slap = if variant == HitVariant::Accent {
                        let n = rng.next_f32();
                        let hp = n - hp_l * 0.75;
                        hp_l = n;
                        hp * (-t * 55.0).exp() * 0.44
                    } else {
                        0.0
                    };
                    let s = (body * 0.84 + slap).tanh() * 0.90;
                    (s, s * 0.97)
                }
            }
            PercussionInstrument::Tambourine => {
                // 6 metallic zill partials + double-bounce stereo brass shimmer
                let z_freqs = [4850.0, 5620.0, 6480.0, 7420.0, 8650.0, 9910.0];
                let mut z = 0.0_f32;
                for (k, zf) in z_freqs.iter().enumerate() {
                    z += ((t * *zf + k as f32 * 0.19) * TAU).sin() * (0.24 - k as f32 * 0.02);
                }
                let nl = rng.next_f32();
                let nr = rng.next_f32();
                let air_l = nl - hp_l * 0.92;
                hp_l = nl;
                let air_r = nr - hp_r * 0.92;
                hp_r = nr;
                let b = |dt: f32, rate: f32| if dt < 0.0 { 0.0 } else { (-dt * rate).exp() };
                let env = if variant == HitVariant::Accent {
                    (b(t, 26.0) + b(t - 0.016, 24.0) * 0.76 + b(t - 0.035, 16.0) * 0.62).min(1.25)
                } else {
                    (b(t, 42.0) + b(t - 0.015, 36.0) * 0.56).min(1.1)
                };
                let atk = (t / 0.0025).min(1.0);
                (
                    (z * 0.48 + air_l * 0.52) * atk * env * 0.74,
                    (z * 0.48 + air_r * 0.52) * atk * env * 0.74,
                )
            }
            PercussionInstrument::Shaker => {
                let nl = rng.next_f32();
                let nr = rng.next_f32();
                bp_l = bp_l * 0.54 + nl * 0.46;
                bp_r = bp_r * 0.54 + nr * 0.46;
                let sl = bp_l - hp_l * 0.87;
                hp_l = bp_l;
                let sr_v = bp_r - hp_r * 0.87;
                hp_r = bp_r;
                let atk = (t / if variant == HitVariant::Accent { 0.006 } else { 0.014 }).min(1.0);
                let dec = if variant == HitVariant::Accent { 32.0 } else { 46.0 };
                let tail = if t > 0.018 { (-(t - 0.018) * 52.0).exp() * 0.25 } else { 0.0 };
                let env = atk * (-t * dec).exp() + tail;
                (sl * env * 0.86, sr_v * env * 0.86)
            }
            PercussionInstrument::Claps => {
                // 8-person stereo praise handclap ensemble with natural room scatter
                let offsets_l = [0.000, 0.007, 0.013, 0.020, 0.028];
                let offsets_r = [0.002, 0.009, 0.016, 0.023, 0.031];
                let mut el = 0.0_f32;
                let mut er = 0.0_f32;
                for &o in &offsets_l {
                    if t >= o {
                        el += (-(t - o) * 170.0).exp() * 0.82;
                    }
                }
                for &o in &offsets_r {
                    if t >= o {
                        er += (-(t - o) * 170.0).exp() * 0.82;
                    }
                }
                if t >= 0.033 {
                    let room_tail = (-(t - 0.033) * 18.0).exp() * 0.92;
                    el += room_tail;
                    er += room_tail;
                }
                let nl = rng.next_f32();
                let nr = rng.next_f32();
                bp_l = bp_l * 0.74 + nl * 0.26;
                bp_r = bp_r * 0.74 + nr * 0.26;
                let fl = bp_l - hp_l * 0.79;
                hp_l = bp_l;
                let fr = bp_r - hp_r * 0.79;
                hp_r = bp_r;
                (
                    (fl * el.min(1.45) * 1.45).tanh() * 0.88,
                    (fr * er.min(1.45) * 1.45).tanh() * 0.88,
                )
            }
            PercussionInstrument::Clave => {
                let f1 = if variant == HitVariant::Soft { 2140.0 } else { 2480.0 }
                    * (1.0 + 0.02 * (-t * 150.0).exp());
                let f2 = f1 * 1.52;
                p1 = (p1 + f1 * inv_sr).fract();
                p2 = (p2 + f2 * inv_sr).fract();
                let wood = (p1 * TAU).sin() * 0.76 + (p2 * TAU).sin() * 0.28;
                let click = rng.next_f32() * (-t * 320.0).exp() * 0.22;
                let s = (wood * (-t * 80.0).exp() + click) * 0.82;
                (s, s)
            }
            PercussionInstrument::Bell => {
                let agogo = is_makossa || profile == StudioKitProfile::StudioPraise;
                let (f1, f2, f3) = if agogo {
                    if variant == HitVariant::Soft {
                        (665.0, 998.0, 1680.0)
                    } else {
                        (935.0, 1408.0, 2345.0)
                    }
                } else {
                    (562.0, 845.0, 1390.0)
                };
                p1 = (p1 + f1 * inv_sr).fract();
                p2 = (p2 + f2 * inv_sr).fract();
                p3 = (p3 + f3 * inv_sr).fract();
                let sq = if p1 < 0.5 { 1.0 } else { -1.0 };
                let raw = sq * 0.34 + (p2 * TAU).sin() * 0.50 + (p3 * TAU).sin() * 0.16 * (-t * 48.0).exp();
                lp_l = lp_l * 0.68 + raw * 0.32;
                let s = lp_l * (-t * 20.0).exp() * 0.72;
                (s, s)
            }
        };
        left[i] = sl;
        right[i] = sr_samp;
    }

    // Smooth 3ms fade-out at end of one-shot to guarantee zero clicks
    let fade = ((0.003 * sr as f32) as usize).min(n_frames);
    for k in 0..fade {
        let g = (fade - k) as f32 / fade as f32;
        let idx = n_frames - fade + k;
        left[idx] *= g;
        right[idx] *= g;
    }

    (left, right)
}

/// Generates a complete multi-variant WAV one-shot sample kit folder ready for live loading or DAW use.
pub fn generate_studio_percussion_kit(
    kits_root: &Path,
    profile: StudioKitProfile,
) -> Result<PercussionKitInfo> {
    let kit_dir = kits_root.join(profile.id());
    fs::create_dir_all(&kit_dir)?;

    let variants_to_generate = [
        HitVariant::Standard,
        HitVariant::Accent,
        HitVariant::Soft,
        HitVariant::Special,
    ];
    let mut sample_count = 0usize;
    let mut covered = Vec::new();

    for &inst in &ALL_PERCUSSION_INSTRUMENTS {
        covered.push(inst);
        for &var in &variants_to_generate {
            // Only write Special where meaningful (HandDrums talking/log drum)
            if var == HitVariant::Special && inst != PercussionInstrument::HandDrums {
                continue;
            }
            let (l, r) = synthesize_studio_one_shot(profile, inst, var, KIT_SAMPLE_RATE);
            let filename = format!("{}{}.wav", inst.stem_slug(), var.file_suffix());
            write_stereo_wav_16(&kit_dir.join(filename), KIT_SAMPLE_RATE, &l, &r)?;
            sample_count += 1;
        }
    }

    let meta = KitMetadataFile {
        id: profile.id().to_string(),
        name: profile.display_name().to_string(),
        profile: Some(profile),
        sample_rate: KIT_SAMPLE_RATE,
    };
    let meta_json = serde_json::to_string_pretty(&meta)
        .map_err(|e| AudioCoreError::Manifest(format!("Failed to serialize kit.json: {e}")))?;
    fs::write(kit_dir.join("kit.json"), meta_json)?;

    Ok(PercussionKitInfo {
        id: meta.id,
        name: meta.name,
        dir: kit_dir.to_string_lossy().to_string(),
        sample_count,
        instruments_covered: covered,
    })
}

fn match_filename_to_slot(stem_lower: &str) -> Option<(PercussionInstrument, HitVariant)> {
    let variant = if stem_lower.contains("special")
        || stem_lower.contains("talking")
        || stem_lower.contains("log")
        || stem_lower.contains("glide")
    {
        HitVariant::Special
    } else if stem_lower.contains("accent")
        || stem_lower.contains("open")
        || stem_lower.contains("slap")
        || stem_lower.contains("rim")
    {
        HitVariant::Accent
    } else if stem_lower.contains("soft")
        || stem_lower.contains("ghost")
        || stem_lower.contains("low")
    {
        HitVariant::Soft
    } else {
        HitVariant::Standard
    };

    let inst = if stem_lower.contains("kick") || stem_lower.contains("bd") {
        PercussionInstrument::Kick
    } else if stem_lower.contains("snare") || stem_lower.contains("cajon") {
        PercussionInstrument::Snare
    } else if stem_lower.contains("hihat")
        || stem_lower.contains("hi-hat")
        || stem_lower.contains("hi_hat")
        || stem_lower.contains("hat")
    {
        PercussionInstrument::HiHat
    } else if stem_lower.contains("hand_drum")
        || stem_lower.contains("handdrum")
        || stem_lower.contains("conga")
        || stem_lower.contains("djembe")
        || stem_lower.contains("talking")
        || stem_lower.contains("log_drum")
    {
        PercussionInstrument::HandDrums
    } else if stem_lower.contains("tambourine") || stem_lower.contains("tamb") {
        PercussionInstrument::Tambourine
    } else if stem_lower.contains("shaker")
        || stem_lower.contains("cabasa")
        || stem_lower.contains("shekere")
        || stem_lower.contains("maraca")
    {
        PercussionInstrument::Shaker
    } else if stem_lower.contains("clap") {
        PercussionInstrument::Claps
    } else if stem_lower.contains("clave") || stem_lower.contains("woodblock") {
        PercussionInstrument::Clave
    } else if stem_lower.contains("bell")
        || stem_lower.contains("agogo")
        || stem_lower.contains("cowbell")
    {
        PercussionInstrument::Bell
    } else {
        return None;
    };

    Some((inst, variant))
}

/// Discovers installed or user-created sample kits under `kits_root`.
pub fn list_percussion_kits(kits_root: &Path) -> Vec<PercussionKitInfo> {
    let mut kits = Vec::new();
    let Ok(rd) = fs::read_dir(kits_root) else {
        return kits;
    };
    for entry in rd.flatten() {
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let dir_name = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("kit")
            .to_string();
        let meta: Option<KitMetadataFile> = fs::read_to_string(dir.join("kit.json"))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok());

        let mut sample_count = 0usize;
        let mut covered = Vec::new();
        if let Ok(files) = fs::read_dir(&dir) {
            for f in files.flatten() {
                let p = f.path();
                let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
                    continue;
                };
                if !matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "wav" | "mp3" | "flac" | "ogg"
                ) {
                    continue;
                }
                let stem = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if let Some((inst, _)) = match_filename_to_slot(&stem) {
                    sample_count += 1;
                    if !covered.contains(&inst) {
                        covered.push(inst);
                    }
                }
            }
        }

        if sample_count > 0 {
            kits.push(PercussionKitInfo {
                id: meta.as_ref().map(|m| m.id.clone()).unwrap_or_else(|| dir_name.clone()),
                name: meta.as_ref().map(|m| m.name.clone()).unwrap_or(dir_name),
                dir: dir.to_string_lossy().to_string(),
                sample_count,
                instruments_covered: covered,
            });
        }
    }
    kits.sort_by(|a, b| a.name.cmp(&b.name));
    kits
}

/// Loads all recognized WAV/audio one-shots from a kit folder into memory for zero-alloc playback.
pub fn load_percussion_kit_from_dir(dir: &Path) -> Result<PercussionSampleKitData> {
    if !dir.exists() || !dir.is_dir() {
        return Err(AudioCoreError::Manifest(format!(
            "Percussion kit folder does not exist: {}",
            dir.display()
        )));
    }
    let meta: Option<KitMetadataFile> = fs::read_to_string(dir.join("kit.json"))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());
    let kit_name = meta
        .map(|m| m.name)
        .unwrap_or_else(|| {
            dir.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Custom Kit")
                .to_string()
        });

    let mut kit = PercussionSampleKitData::empty(kit_name);
    let mut loaded_any = false;
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let p = entry.path();
        let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !matches!(
            ext.to_ascii_lowercase().as_str(),
            "wav" | "mp3" | "flac" | "ogg"
        ) {
            continue;
        }
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let Some((inst, var)) = match_filename_to_slot(&stem) else {
            continue;
        };
        let (interleaved, sr) = decode_audio_to_stereo(&p)?;
        let frames = interleaved.len() / 2;
        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        for chunk in interleaved.chunks_exact(2) {
            left.push(chunk[0]);
            right.push(chunk[1]);
        }
        kit.set_slot(
            inst,
            var,
            Arc::new(LoadedPercSample {
                sample_rate: sr,
                left,
                right,
            }),
        );
        loaded_any = true;
    }

    if !loaded_any {
        return Err(AudioCoreError::Manifest(format!(
            "No matching percussion samples (kick.wav, snare.wav, claps.wav, etc.) found in {}",
            dir.display()
        )));
    }
    Ok(kit)
}

pub fn list_percussion_loops(loops_root: &Path) -> Vec<CustomPercussionPattern> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(loops_root) else {
        return out;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        if let Ok(raw) = fs::read_to_string(&p) {
            if let Ok(pattern) = serde_json::from_str::<CustomPercussionPattern>(&raw) {
                out.push(pattern.clamped_and_sorted());
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn save_percussion_loop(
    loops_root: &Path,
    pattern: CustomPercussionPattern,
) -> Result<Vec<CustomPercussionPattern>> {
    fs::create_dir_all(loops_root)?;
    let clean = pattern.clamped_and_sorted();
    let slug = sanitize_slug(&clean.id, "custom-loop");
    let mut final_pattern = clean.clone();
    final_pattern.id = slug.clone();
    let json = serde_json::to_string_pretty(&final_pattern)
        .map_err(|e| AudioCoreError::Manifest(format!("Failed to serialize loop: {e}")))?;
    let target = loops_root.join(format!("{slug}.json"));
    let tmp = loops_root.join(format!("{slug}.json.tmp"));
    fs::write(&tmp, json)?;
    fs::rename(&tmp, &target)?;
    Ok(list_percussion_loops(loops_root))
}

pub fn delete_percussion_loop(loops_root: &Path, id: &str) -> Result<Vec<CustomPercussionPattern>> {
    let slug = sanitize_slug(id, "");
    if !slug.is_empty() {
        let target = loops_root.join(format!("{slug}.json"));
        if target.exists() {
            fs::remove_file(target)?;
        }
    }
    Ok(list_percussion_loops(loops_root))
}

/// Renders a custom or built-in drum loop into an isolated 9-stem Psalm Stage song bundle
/// (`kick.wav`, `snare.wav`, `hihat.wav`, `hand_drums.wav`, `tambourine.wav`, `shaker.wav`,
/// `claps.wav`, `clave.wav`, `bell.wav` + `manifest.json`) so every instrument is toggleable!
pub fn export_percussion_loop_bundle(
    library_root: &Path,
    req: ExportLoopBundleRequest,
) -> Result<ExportLoopBundleResult> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(AudioCoreError::Manifest(
            "Loop bundle title cannot be empty".into(),
        ));
    }
    let slug = sanitize_slug(title, "worship-drum-loop");
    let target_dir = match req.output_dir {
        Some(ref custom) if !custom.trim().is_empty() => PathBuf::from(custom.trim()),
        _ => library_root.join(&slug),
    };
    fs::create_dir_all(&target_dir)?;

    let bpm = if req.bpm.is_finite() {
        req.bpm.clamp(40.0, 240.0)
    } else {
        120.0
    };
    let bpb = req.time_signature[0].clamp(2, 16);
    let unit = req.time_signature[1].clamp(2, 16);
    let bars = req.bars.clamp(1, 64);
    let sr = KIT_SAMPLE_RATE;
    let spb = 60.0 / bpm;
    let total_beats = (bpb as f64) * (bars as f64);
    let duration_sec = total_beats * spb;
    let total_frames = (duration_sec * sr as f64).round() as usize;

    let loaded_kit = match req.kit_dir {
        Some(ref dir) if !dir.trim().is_empty() => {
            Some(load_percussion_kit_from_dir(Path::new(dir.trim()))?)
        }
        _ => None,
    };

    // Build section intensity map when dynamic arrangement is requested
    let sections: Vec<SectionDef> = if req.dynamic_arrangement && bars >= 8 {
        let q = (bars / 4).max(1);
        let b1 = 1;
        let b2 = b1 + q;
        let b3 = b2 + q;
        let b4 = b3 + q;
        vec![
            SectionDef {
                id: "intro".into(),
                name: "Intro".into(),
                start_bar: b1,
                end_bar: b2,
                start_sec: None,
                end_sec: None,
                color: Some("#64748b".into()),
            },
            SectionDef {
                id: "verse".into(),
                name: "Verse".into(),
                start_bar: b2,
                end_bar: b3,
                start_sec: None,
                end_sec: None,
                color: Some("#0ea5e9".into()),
            },
            SectionDef {
                id: "chorus".into(),
                name: "Chorus".into(),
                start_bar: b3,
                end_bar: b4,
                start_sec: None,
                end_sec: None,
                color: Some("#10b981".into()),
            },
            SectionDef {
                id: "praise-break".into(),
                name: "Praise Break".into(),
                start_bar: b4,
                end_bar: bars + 1,
                start_sec: None,
                end_sec: None,
                color: Some("#f59e0b".into()),
            },
        ]
    } else {
        vec![SectionDef {
            id: "groove".into(),
            name: "Groove".into(),
            start_bar: 1,
            end_bar: bars + 1,
            start_sec: None,
            end_sec: None,
            color: Some("#10b981".into()),
        }]
    };

    let mut stems_map: HashMap<String, StemDef> = HashMap::new();
    let mut stem_files = Vec::new();

    for &inst in &ALL_PERCUSSION_INSTRUMENTS {
        let mut engine = PercussionEngine::new(sr);
        engine.mode = PercussionMode::PercussionOnly;
        engine.style = req.style;
        engine.gain = 1.0;
        engine.room_mix = req.room_mix.clamp(0.0, 1.0);
        if let Some(ref kit) = loaded_kit {
            engine.set_sample_kit(Some(kit.clone()), req.kit_dir.clone());
        }
        if let Some(ref cp) = req.custom_pattern {
            engine.set_custom_pattern(Some(cp.clone()));
        }
        for ch in &mut engine.channels {
            ch.enabled = ch.instrument == inst;
            ch.gain = 1.0; // Export at clean 100% unity so live mixer gain scales cleanly
        }

        let mut left = vec![0.0_f32; total_frames];
        let mut right = vec![0.0_f32; total_frames];
        let mut peak = 0.0_f32;

        for frame in 0..total_frames {
            let beat = (frame as f64 / sr as f64) / spb;
            let bar_num = (beat / bpb as f64).floor() as u32 + 1;
            engine.intensity = if req.dynamic_arrangement && bars >= 8 {
                let q = (bars / 4).max(1);
                if bar_num <= q {
                    GrooveIntensity::Low
                } else if bar_num <= q * 2 {
                    GrooveIntensity::Low
                } else if bar_num <= q * 3 {
                    GrooveIntensity::Medium
                } else {
                    GrooveIntensity::High
                }
            } else {
                req.intensity
            };
            engine.advance_and_trigger_step(beat, bpb as u32);
            let (l, r) = engine.next_stereo_sample(0);
            left[frame] = l;
            right[frame] = r;
            peak = peak.max(l.abs()).max(r.abs());
        }

        if peak > 1e-4 {
            let filename = format!("{}.wav", inst.stem_slug());
            write_stereo_wav_16(&target_dir.join(&filename), sr, &left, &right)?;
            stems_map.insert(
                inst.stem_slug().to_string(),
                StemDef::Detailed {
                    file: filename.clone(),
                    pitched: false,
                    is_cues: Some(false),
                    default_gain: inst.default_gain(),
                    muted: false,
                },
            );
            stem_files.push(filename);
        }
    }

    if stems_map.is_empty() {
        return Err(AudioCoreError::Manifest(
            "Pattern produced no audible instrument hits to export".into(),
        ));
    }

    let manifest = SongManifest {
        id: slug,
        title: title.to_string(),
        artist: "Psalm Stage Loop Studio".to_string(),
        bpm,
        time_signature: [bpb, unit],
        first_downbeat_sec: 0.0,
        original_key: "C".to_string(),
        click_mode: ClickMode::AlwaysOn,
        provenance: None,
        sections,
        stems: stems_map,
    };
    save_song_manifest(&target_dir, &manifest)?;

    Ok(ExportLoopBundleResult {
        song_dir: target_dir.to_string_lossy().to_string(),
        title: title.to_string(),
        bpm,
        duration_sec,
        stem_files,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::loader::load_song_from_dir;

    #[test]
    fn test_generate_and_load_studio_percussion_kit() {
        let tmp = std::env::temp_dir().join(format!(
            "psalm_kit_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let info = generate_studio_percussion_kit(&tmp, StudioKitProfile::MakossaLive)
            .expect("Kit generation should succeed");
        assert!(info.sample_count >= 25);
        let kits = list_percussion_kits(&tmp);
        assert_eq!(kits.len(), 1);
        let loaded = load_percussion_kit_from_dir(Path::new(&info.dir))
            .expect("Loading generated WAV kit should succeed");
        assert!(loaded.resolve(PercussionInstrument::Kick, HitVariant::Standard).is_some());
        assert!(loaded.resolve(PercussionInstrument::Claps, HitVariant::Standard).is_some());
        assert!(loaded.resolve(PercussionInstrument::Tambourine, HitVariant::Accent).is_some());
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_export_multi_stem_loop_bundle_loads_as_unpitched_stems() {
        let tmp = std::env::temp_dir().join(format!(
            "psalm_loop_bundle_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let res = export_percussion_loop_bundle(
            &tmp,
            ExportLoopBundleRequest {
                title: "Makossa Sunday Groove".into(),
                output_dir: None,
                bpm: 129.0,
                time_signature: [4, 4],
                bars: 8,
                dynamic_arrangement: true,
                intensity: GrooveIntensity::High,
                room_mix: 0.25,
                style: PercussionStyle::Makossa,
                custom_pattern: None,
                kit_dir: None,
            },
        )
        .expect("Loop bundle export should succeed");
        assert!(res.stem_files.len() >= 6);
        let loaded = load_song_from_dir(Path::new(&res.song_dir), 48_000)
            .expect("Exported bundle should load as valid Psalm Stage song");
        assert_eq!(loaded.manifest.sections.len(), 4);
        for stem in &loaded.stems {
            assert!(
                !stem.pitched,
                "All exported percussion stems must be unpitched (pitched=false), got {}={}",
                stem.name, stem.pitched
            );
        }
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_custom_loop_crud_roundtrip() {
        let tmp = std::env::temp_dir().join(format!(
            "psalm_loops_crud_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let custom = PercussionStyle::Makossa.to_custom_pattern("my-makossa", "My Makossa Loop");
        let list = save_percussion_loop(&tmp, custom.clone()).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "My Makossa Loop");
        assert!(!list[0].hits.is_empty());

        let after_del = delete_percussion_loop(&tmp, "my-makossa").unwrap();
        assert!(after_del.is_empty());
        let _ = fs::remove_dir_all(&tmp);
    }
}
