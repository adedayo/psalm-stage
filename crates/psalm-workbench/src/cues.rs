use std::collections::HashMap;
use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use psalm_audio_core::loader::{decode_audio_to_stereo, resample_stereo_linear, save_song_manifest};
use psalm_audio_core::manifest::{SectionDef, SongManifest, StemDef};
use psalm_audio_core::percussion_kit::write_stereo_wav_16;
use serde::{Deserialize, Serialize};

pub const GENERATED_CUES_FILENAME: &str = "section-cues.wav";
const DEFAULT_CUE_SAMPLE_RATE: u32 = 48_000;

static TEMP_CUE_COUNTER: AtomicU64 = AtomicU64::new(1);

fn default_lead_in_beats() -> f32 {
    2.0
}

fn default_include_chime() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionCueOptions {
    #[serde(default = "default_lead_in_beats")]
    pub lead_in_beats: f32,
    #[serde(default = "default_include_chime")]
    pub include_chime: bool,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    #[serde(default)]
    pub use_os_tts: Option<bool>,
}

impl Default for SectionCueOptions {
    fn default() -> Self {
        Self {
            lead_in_beats: default_lead_in_beats(),
            include_chime: default_include_chime(),
            sample_rate: None,
            use_os_tts: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SectionCueTimingOverride {
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub first_downbeat_sec: Option<f64>,
    #[serde(default)]
    pub beats_per_bar: Option<u8>,
    #[serde(default)]
    pub sections: Option<Vec<SectionDef>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SectionCuePlacement {
    pub section_id: String,
    pub section_name: String,
    pub phrase: String,
    pub section_start_sec: f64,
    pub cue_start_sec: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedSectionCues {
    pub stem_key: String,
    pub file_name: String,
    pub used_os_tts: bool,
    pub placements: Vec<SectionCuePlacement>,
    pub duration_sec: f64,
    pub manifest: SongManifest,
}

pub fn normalize_cue_phrase(name: &str) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return "Section".to_string();
    }
    let normalized = trimmed
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let lower = normalized.to_ascii_lowercase();
    match lower.as_str() {
        "pc" | "pre" => "Pre Chorus".to_string(),
        "inst" | "instr" => "Instrumental".to_string(),
        "v" => "Verse".to_string(),
        "c" => "Chorus".to_string(),
        "b" | "br" => "Bridge".to_string(),
        _ if lower.starts_with('v')
            && lower.len() <= 3
            && lower[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("Verse {}", &lower[1..])
        }
        _ if lower.starts_with('c')
            && lower.len() <= 3
            && lower[1..].chars().all(|c| c.is_ascii_digit()) =>
        {
            format!("Chorus {}", &lower[1..])
        }
        _ => normalized.replace('-', " "),
    }
}

fn render_cue_chime_mono(sample_rate: u32) -> Vec<f32> {
    let frames = (sample_rate as f32 * 0.14).round() as usize;
    let split = frames / 2;
    let mut out = vec![0.0_f32; frames];
    let sr = sample_rate.max(8000) as f32;
    for (i, sample) in out.iter_mut().enumerate() {
        let (freq, local_i, dur) = if i < split {
            (1174.66_f32, i, split.max(1))
        } else {
            (1567.98_f32, i - split, (frames - split).max(1))
        };
        let t = local_i as f32 / sr;
        let norm = local_i as f32 / dur as f32;
        let attack = (local_i as f32 / (sr * 0.004)).clamp(0.0, 1.0);
        let decay = (-norm * 5.0).exp();
        *sample = ((TAU * freq * t).sin() * 0.85 + (TAU * freq * 2.0 * t).sin() * 0.15)
            * attack
            * decay
            * 0.28;
    }
    out
}

fn render_fallback_phrase_mono(phrase: &str, sample_rate: u32) -> Vec<f32> {
    let words = phrase.split_whitespace().count().clamp(1, 4);
    let sr = sample_rate.max(8000) as f32;
    let pulse_frames = (sr * 0.14).round() as usize;
    let gap_frames = (sr * 0.04).round() as usize;
    let total_frames = words * pulse_frames + words.saturating_sub(1) * gap_frames;
    let mut out = vec![0.0_f32; total_frames.max(1)];

    let base_freq = match phrase.to_ascii_lowercase().as_str() {
        s if s.contains("intro") => 523.25_f32,
        s if s.contains("verse") => 587.33_f32,
        s if s.contains("pre") => 659.25_f32,
        s if s.contains("chorus") => 783.99_f32,
        s if s.contains("bridge") => 880.0_f32,
        s if s.contains("outro") || s.contains("end") => 440.0_f32,
        _ => 698.46_f32,
    };

    for w in 0..words {
        let start = w * (pulse_frames + gap_frames);
        let freq = base_freq * (1.0 + 0.06 * (w as f32));
        for i in 0..pulse_frames {
            let t = i as f32 / sr;
            let norm = i as f32 / pulse_frames.max(1) as f32;
            let env = (i as f32 / (sr * 0.008)).clamp(0.0, 1.0) * (1.0 - norm).powf(1.4);
            let tone = (TAU * freq * t).sin() * 0.72 + (TAU * freq * 2.0 * t).sin() * 0.28;
            if let Some(slot) = out.get_mut(start + i) {
                *slot = tone * env * 0.55;
            }
        }
    }
    out
}

fn trim_and_normalize_mono(samples: &[f32], target_peak: f32) -> Vec<f32> {
    let threshold = 0.003_f32;
    let first = samples.iter().position(|s| s.abs() > threshold).unwrap_or(0);
    let last = samples
        .iter()
        .rposition(|s| s.abs() > threshold)
        .map(|idx| idx + 1)
        .unwrap_or(samples.len());
    if last <= first {
        return Vec::new();
    }
    let slice = &samples[first..last];
    let peak = slice.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()));
    if peak <= 1e-6 {
        return Vec::new();
    }
    let scale = target_peak / peak;
    slice.iter().map(|s| s * scale).collect()
}

fn decode_mono_resampled(path: &Path, target_sr: u32) -> Option<Vec<f32>> {
    let (stereo, src_rate) = decode_audio_to_stereo(path).ok()?;
    let resampled = if src_rate == target_sr {
        stereo
    } else {
        resample_stereo_linear(&stereo, src_rate, target_sr)
    };
    let mut mono = Vec::with_capacity(resampled.len() / 2);
    for chunk in resampled.chunks_exact(2) {
        mono.push(0.5 * (chunk[0] + chunk[1]));
    }
    let cleaned = trim_and_normalize_mono(&mono, 0.78);
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn try_os_tts_mono(phrase: &str, target_sr: u32) -> Option<Vec<f32>> {
    if std::env::var_os("PSALM_STAGE_DISABLE_OS_TTS").is_some_and(|v| v == "1") {
        return None;
    }

    let id = TEMP_CUE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_wav: PathBuf = std::env::temp_dir().join(format!(
        "psalm_stage_cue_{}_{}.wav",
        std::process::id(),
        id
    ));

    let success = if cfg!(target_os = "macos") {
        Command::new("/usr/bin/say")
            .args([
                "-r",
                "210",
                "-o",
                &tmp_wav.to_string_lossy(),
                "--file-format=WAVE",
                "--data-format=LEF32@48000",
                phrase,
            ])
            .status()
            .is_ok_and(|s| s.success())
    } else if cfg!(target_os = "linux") {
        let escaped = tmp_wav.to_string_lossy().to_string();
        Command::new("espeak-ng")
            .args(["-s", "190", "-w", &escaped, phrase])
            .status()
            .is_ok_and(|s| s.success())
            || Command::new("espeak")
                .args(["-s", "190", "-w", &escaped, phrase])
                .status()
                .is_ok_and(|s| s.success())
    } else if cfg!(target_os = "windows") {
        let escaped_path = tmp_wav.to_string_lossy().replace('\'', "''");
        let escaped_text = phrase.replace('\'', "''");
        let script = format!(
            "Add-Type -AssemblyName System.Speech; \
             $s = New-Object System.Speech.Synthesis.SpeechSynthesizer; \
             $s.Rate = 2; \
             $s.SetOutputToWaveFile('{escaped_path}'); \
             $s.Speak('{escaped_text}'); \
             $s.Dispose()"
        );
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .status()
            .is_ok_and(|s| s.success())
    } else {
        false
    };

    let result = if success && tmp_wav.exists() {
        decode_mono_resampled(&tmp_wav, target_sr)
    } else {
        None
    };
    let _ = std::fs::remove_file(&tmp_wav);
    result
}

/// Renders and writes an IEM-only section cue stem (`section-cues.wav`) for all
/// sections in `manifest.json` (plus any unsaved workbench timing/section overrides)
/// and registers it in `manifest.stems` with `is_cues = true`.
pub fn generate_section_cues_for_song(
    song_dir: &Path,
    timing_override: Option<SectionCueTimingOverride>,
    options: &SectionCueOptions,
) -> Result<GeneratedSectionCues, String> {
    let manifest_path = song_dir.join("manifest.json");
    let mut manifest = SongManifest::from_file(&manifest_path).map_err(|e| e.to_string())?;

    if let Some(ov) = timing_override {
        if let Some(bpm) = ov.bpm {
            if (30.0..=260.0).contains(&bpm) {
                manifest.bpm = bpm;
            }
        }
        if let Some(downbeat) = ov.first_downbeat_sec {
            if downbeat >= 0.0 && downbeat.is_finite() {
                manifest.first_downbeat_sec = downbeat;
            }
        }
        if let Some(bpb) = ov.beats_per_bar {
            if (1..=12).contains(&bpb) {
                manifest.time_signature[0] = bpb;
            }
        }
        if let Some(sections) = ov.sections {
            if !sections.is_empty() {
                manifest.sections = sections;
            }
        }
    }

    if manifest.sections.is_empty() {
        return Err("At least one arrangement section is required to generate cues.".into());
    }

    let sr = options.sample_rate.unwrap_or(DEFAULT_CUE_SAMPLE_RATE).clamp(22_050, 96_000);
    let beat_sec = 60.0 / manifest.bpm.clamp(30.0, 260.0);
    let lead_in_sec = (options.lead_in_beats.clamp(0.0, 8.0) as f64) * beat_sec;
    let allow_os_tts = options.use_os_tts.unwrap_or(true);

    let chime = if options.include_chime {
        render_cue_chime_mono(sr)
    } else {
        Vec::new()
    };

    let mut phrase_cache: HashMap<String, (Vec<f32>, bool)> = HashMap::new();
    let mut placements = Vec::with_capacity(manifest.sections.len());
    let mut clip_plans: Vec<(usize, Vec<f32>)> = Vec::with_capacity(manifest.sections.len());
    let mut used_os_tts_any = false;
    let mut max_end_frame: usize = sr as usize;

    for sec in &manifest.sections {
        let section_start_sec = sec
            .start_sec
            .unwrap_or_else(|| manifest.bar_to_seconds(sec.start_bar))
            .max(0.0);
        let cue_start_sec = (section_start_sec - lead_in_sec).max(0.0);
        let phrase = normalize_cue_phrase(&sec.name);

        let (voice_clip, used_tts) = if let Some(cached) = phrase_cache.get(&phrase) {
            cached.clone()
        } else {
            let rendered = if allow_os_tts {
                match try_os_tts_mono(&phrase, sr) {
                    Some(v) => (v, true),
                    None => (render_fallback_phrase_mono(&phrase, sr), false),
                }
            } else {
                (render_fallback_phrase_mono(&phrase, sr), false)
            };
            phrase_cache.insert(phrase.clone(), rendered.clone());
            rendered
        };
        if used_tts {
            used_os_tts_any = true;
        }

        let mut combined = if chime.is_empty() {
            voice_clip
        } else {
            let gap = (sr as f32 * 0.03).round() as usize;
            let mut buf = Vec::with_capacity(chime.len() + gap + voice_clip.len());
            buf.extend_from_slice(&chime);
            buf.resize(chime.len() + gap, 0.0);
            buf.extend_from_slice(&voice_clip);
            buf
        };

        // Keep long section labels concise so successive short sections never overlap.
        let max_clip_frames = ((sr as f64) * 2.2).round() as usize;
        if combined.len() > max_clip_frames {
            combined.truncate(max_clip_frames);
            let fade = (sr as usize / 40).min(combined.len());
            let start_fade = combined.len() - fade;
            for (idx, sample) in combined[start_fade..].iter_mut().enumerate() {
                *sample *= 1.0 - (idx as f32 / fade.max(1) as f32);
            }
        }

        let start_frame = (cue_start_sec * sr as f64).round() as usize;
        max_end_frame = max_end_frame.max(start_frame + combined.len() + (sr as usize / 20));
        placements.push(SectionCuePlacement {
            section_id: sec.id.clone(),
            section_name: sec.name.clone(),
            phrase,
            section_start_sec,
            cue_start_sec,
        });
        clip_plans.push((start_frame, combined));
    }

    let mut mix = vec![0.0_f32; max_end_frame];
    for (start_frame, clip) in clip_plans {
        for (i, sample) in clip.iter().enumerate() {
            if let Some(dst) = mix.get_mut(start_frame + i) {
                *dst = (*dst + *sample).clamp(-0.95, 0.95);
            }
        }
    }

    let out_path = song_dir.join(GENERATED_CUES_FILENAME);
    write_stereo_wav_16(&out_path, sr, &mix, &mix).map_err(|e| e.to_string())?;

    let stem_key = match manifest.stems.get("cues") {
        Some(existing) if existing.file_name() != GENERATED_CUES_FILENAME => {
            "section_cues".to_string()
        }
        _ => "cues".to_string(),
    };

    manifest.stems.insert(
        stem_key.clone(),
        StemDef::Detailed {
            file: GENERATED_CUES_FILENAME.to_string(),
            pitched: false,
            is_cues: Some(true),
            default_gain: 0.9,
            muted: false,
        },
    );

    save_song_manifest(song_dir, &manifest).map_err(|e| e.to_string())?;

    Ok(GeneratedSectionCues {
        stem_key,
        file_name: GENERATED_CUES_FILENAME.to_string(),
        used_os_tts: used_os_tts_any,
        placements,
        duration_sec: mix.len() as f64 / sr as f64,
        manifest,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use psalm_audio_core::loader::load_song_from_dir;
    use psalm_audio_core::manifest::{ClickMode, SongManifest};

    #[test]
    fn test_generate_section_cues_places_cues_with_lead_in_and_iem_role() {
        let dir = std::env::temp_dir().join(format!(
            "psalm_stage_cues_test_{}_{}",
            std::process::id(),
            TEMP_CUE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();

        // Write a tiny dummy backing stem and manifest at 120 BPM (0.5s per beat)
        let sr = 48_000u32;
        let tone: Vec<f32> = (0..(sr * 12))
            .map(|i| (i as f32 * 0.01).sin() * 0.1)
            .collect();
        write_stereo_wav_16(&dir.join("music.wav"), sr, &tone, &tone).unwrap();

        let mut stems = HashMap::new();
        stems.insert(
            "music".to_string(),
            StemDef::Detailed {
                file: "music.wav".to_string(),
                pitched: true,
                is_cues: Some(false),
                default_gain: 1.0,
                muted: false,
            },
        );

        let manifest = SongManifest {
            id: "test-song".into(),
            title: "Test Cue Song".into(),
            artist: String::new(),
            bpm: 120.0,
            original_key: "G".into(),
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            click_mode: ClickMode::AlwaysOn,
            sections: vec![
                SectionDef {
                    id: "intro".into(),
                    name: "Intro".into(),
                    start_bar: 1,
                    end_bar: 5,
                    start_sec: Some(0.0),
                    end_sec: Some(4.0),
                    color: Some("sky".into()),
                },
                SectionDef {
                    id: "verse-1".into(),
                    name: "V1".into(),
                    start_bar: 5,
                    end_bar: 9,
                    start_sec: Some(4.0),
                    end_sec: Some(8.0),
                    color: Some("emerald".into()),
                },
                SectionDef {
                    id: "outro".into(),
                    name: "Outro".into(),
                    start_bar: 9,
                    end_bar: 13,
                    start_sec: Some(8.0),
                    end_sec: Some(12.0),
                    color: Some("amber".into()),
                },
            ],
            stems,
            provenance: None,
        };
        save_song_manifest(&dir, &manifest).unwrap();

        let opts = SectionCueOptions {
            lead_in_beats: 2.0, // 1.0s at 120 BPM
            include_chime: true,
            sample_rate: Some(48_000),
            use_os_tts: Some(false), // Deterministic pure-Rust synth for CI
        };
        let res = generate_section_cues_for_song(&dir, None, &opts).unwrap();
        assert_eq!(res.stem_key, "cues");
        assert_eq!(res.file_name, GENERATED_CUES_FILENAME);
        assert!(!res.used_os_tts);
        assert_eq!(res.placements.len(), 3);
        assert_eq!(res.placements[0].cue_start_sec, 0.0);
        assert_eq!(res.placements[1].phrase, "Verse 1");
        assert!((res.placements[1].cue_start_sec - 3.0).abs() < 1e-6);
        assert!((res.placements[2].cue_start_sec - 7.0).abs() < 1e-6);

        let cues_def = &res.manifest.stems["cues"];
        assert!(cues_def.is_cues("cues"));
        assert!(!cues_def.is_pitched("cues"));

        // Verify audio core loads the cue stem strictly as is_cues = true and has energy near 3.0s
        let loaded = load_song_from_dir(&dir, 48_000).unwrap();
        let cue_stem = loaded.stems.iter().find(|s| s.name == "cues").unwrap();
        assert!(cue_stem.is_cues);
        assert!(!cue_stem.pitched);
        let at_3s_idx = (3.05 * 48_000.0) as usize * 2;
        let window_peak = cue_stem.samples[at_3s_idx..at_3s_idx + 4096]
            .iter()
            .fold(0.0_f32, |acc, s| acc.max(s.abs()));
        assert!(window_peak > 0.05, "Expected cue signal at ~3.05s, peak={window_peak}");

        // Verify silent region before Verse 1 cue (e.g. ~1.8s)
        let quiet_idx = (1.8 * 48_000.0) as usize * 2;
        let quiet_peak = cue_stem.samples[quiet_idx..quiet_idx + 2048]
            .iter()
            .fold(0.0_f32, |acc, s| acc.max(s.abs()));
        assert!(quiet_peak < 1e-5, "Expected silence between Intro and Verse 1 cue, peak={quiet_peak}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_generate_section_cues_preserves_custom_cues_stem() {
        let dir = std::env::temp_dir().join(format!(
            "psalm_stage_cues_preserve_{}_{}",
            std::process::id(),
            TEMP_CUE_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let sr = 48_000u32;
        let silence = vec![0.0_f32; sr as usize * 2];
        write_stereo_wav_16(&dir.join("Guide.wav"), sr, &silence, &silence).unwrap();

        let mut stems = HashMap::new();
        stems.insert(
            "cues".to_string(),
            StemDef::Detailed {
                file: "Guide.wav".to_string(),
                pitched: false,
                is_cues: Some(true),
                default_gain: 1.0,
                muted: false,
            },
        );

        let manifest = SongManifest {
            id: "preserve-test".into(),
            title: "Preserve Cues".into(),
            artist: String::new(),
            bpm: 100.0,
            original_key: "C".into(),
            time_signature: [4, 4],
            first_downbeat_sec: 0.0,
            click_mode: ClickMode::AlwaysOn,
            sections: vec![SectionDef {
                id: "chorus".into(),
                name: "Chorus".into(),
                start_bar: 1,
                end_bar: 5,
                start_sec: Some(2.0),
                end_sec: Some(6.0),
                color: None,
            }],
            stems,
            provenance: None,
        };
        save_song_manifest(&dir, &manifest).unwrap();

        let res = generate_section_cues_for_song(
            &dir,
            None,
            &SectionCueOptions {
                use_os_tts: Some(false),
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(res.stem_key, "section_cues");
        assert_eq!(res.manifest.stems["cues"].file_name(), "Guide.wav");
        assert_eq!(
            res.manifest.stems["section_cues"].file_name(),
            GENERATED_CUES_FILENAME
        );
        assert!(res.manifest.stems["section_cues"].is_cues("section_cues"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
