use std::collections::HashMap;
use std::f32::consts::TAU;
use std::fs::File;
use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::manifest::{SectionDef, SongManifest, StemDef};
use crate::{AudioCoreError, Result};

/// Decoded stereo interleaved PCM (`[L, R, L, R, ...]`) normalized to the active device sample rate.
#[derive(Debug, Clone)]
pub struct DecodedStem {
    pub name: String,
    pub pitched: bool,
    pub is_cues: bool,
    pub gain: f32,
    pub muted: bool,
    /// Interleaved stereo samples at `LoadedSong::sample_rate`.
    pub samples: Vec<f32>,
}

impl DecodedStem {
    pub fn frame_count(&self) -> usize {
        self.samples.len() / 2
    }
}

#[derive(Debug, Clone)]
pub struct LoadedSong {
    pub manifest: SongManifest,
    pub sample_rate: u32,
    pub stems: Vec<DecodedStem>,
    pub total_frames: usize,
}

pub fn load_song_from_dir(dir: &Path, target_sample_rate: u32) -> Result<LoadedSong> {
    let manifest_path = dir.join("manifest.json");
    let manifest = SongManifest::from_file(&manifest_path)?;

    let mut stems = Vec::new();
    let mut max_frames = 0usize;

    // Preserve predictable stem display order in the mixer.
    let preferred_order = ["drums", "bass", "music", "vocals", "cues"];
    let mut keys: Vec<String> = manifest.stems.keys().cloned().collect();
    keys.sort_by_key(|k| {
        preferred_order
            .iter()
            .position(|p| p == k)
            .unwrap_or(preferred_order.len())
    });

    for key in keys {
        let stem_def = &manifest.stems[&key];
        let stem_path = dir.join(stem_def.file_name());
        let (raw_stereo, src_rate) = decode_audio_to_stereo(&stem_path)?;
        let samples = if src_rate == target_sample_rate {
            raw_stereo
        } else {
            resample_stereo_linear(&raw_stereo, src_rate, target_sample_rate)
        };
        max_frames = max_frames.max(samples.len() / 2);

        let default_gain = match stem_def {
            StemDef::File(_) => 1.0,
            StemDef::Detailed { default_gain, .. } => *default_gain,
        };

        stems.push(DecodedStem {
            name: key.clone(),
            pitched: stem_def.is_pitched(&key),
            is_cues: stem_def.is_cues(&key),
            gain: default_gain,
            muted: false,
            samples,
        });
    }

    Ok(LoadedSong {
        manifest,
        sample_rate: target_sample_rate,
        stems,
        total_frames: max_frames,
    })
}

pub fn decode_audio_to_stereo(path: &Path) -> Result<(Vec<f32>, u32)> {
    let file = File::open(path).map_err(|e| {
        AudioCoreError::Decode(format!("Failed to open stem {}: {e}", path.display()))
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
        .map_err(|e| AudioCoreError::Decode(format!("Unsupported format {}: {e}", path.display())))?;

    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| AudioCoreError::Decode(format!("No audio track in {}", path.display())))?;

    let codec_params = track.codec_params.clone();
    let track_id = track.id;
    let sample_rate = codec_params
        .sample_rate
        .ok_or_else(|| AudioCoreError::Decode("Missing sample rate".into()))?;
    let channels = codec_params.channels.map(|c| c.count()).unwrap_or(2);

    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| AudioCoreError::Decode(format!("Codec init failed: {e}")))?;

    let mut out = Vec::new();
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

        match decoder.decode(&packet) {
            Ok(audio_buf) => {
                let spec = *audio_buf.spec();
                let frames = audio_buf.frames();
                let buf = sample_buf.get_or_insert_with(|| {
                    SampleBuffer::<f32>::new(audio_buf.capacity() as u64, spec)
                });
                buf.copy_interleaved_ref(audio_buf);
                let interleaved = buf.samples();

                if channels == 1 {
                    for &s in interleaved.iter().take(frames) {
                        out.push(s);
                        out.push(s);
                    }
                } else {
                    for frame in interleaved.chunks_exact(channels).take(frames) {
                        out.push(frame[0]);
                        out.push(frame[1]);
                    }
                }
            }
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(e) => return Err(AudioCoreError::Decode(e.to_string())),
        }
    }

    Ok((out, sample_rate))
}

fn resample_stereo_linear(input: &[f32], src_rate: u32, dst_rate: u32) -> Vec<f32> {
    if input.len() < 4 || src_rate == dst_rate {
        return input.to_vec();
    }
    let src_frames = input.len() / 2;
    let ratio = dst_rate as f64 / src_rate as f64;
    let dst_frames = ((src_frames as f64) * ratio).round() as usize;
    let mut out = Vec::with_capacity(dst_frames * 2);

    for i in 0..dst_frames {
        let src_pos = (i as f64) / ratio;
        let idx0 = (src_pos.floor() as usize).min(src_frames - 1);
        let idx1 = (idx0 + 1).min(src_frames - 1);
        let frac = (src_pos - idx0 as f64) as f32;

        let l0 = input[idx0 * 2];
        let r0 = input[idx0 * 2 + 1];
        let l1 = input[idx1 * 2];
        let r1 = input[idx1 * 2 + 1];

        out.push(l0 + (l1 - l0) * frac);
        out.push(r0 + (r1 - r0) * frac);
    }

    out
}

/// Generates a musical 32-bar worship demo song (72 BPM, Key of G, 4/4) in memory so you can
/// test Focusrite Split L/R routing, section jumping, looping, and Key/BPM shifting immediately.
pub fn generate_demo_song(sample_rate: u32) -> LoadedSong {
    let bpm = 72.0_f64;
    let beats_per_bar = 4_u32;
    let total_bars = 32_u32;
    let sec_per_beat = 60.0 / bpm;
    let total_sec = (total_bars * beats_per_bar) as f64 * sec_per_beat;
    let total_frames = (total_sec * sample_rate as f64).round() as usize;

    let mut drums = vec![0.0_f32; total_frames * 2];
    let mut bass = vec![0.0_f32; total_frames * 2];
    let mut music = vec![0.0_f32; total_frames * 2];

    // Progression in G major: G (I), D/F# (V), Em7 (vi), Cmaj7 (IV) — 1 chord per bar.
    let chord_roots_hz = [98.00_f32, 73.42_f32, 82.41_f32, 65.41_f32]; // G2, D2, E2, C2
    let pad_triads_hz: [[f32; 3]; 4] = [
        [196.00, 246.94, 293.66], // G - B - D
        [185.00, 220.00, 293.66], // F# - A - D
        [196.00, 246.94, 329.63], // G - B - E
        [196.00, 261.63, 329.63], // G - C - E
    ];

    let frames_per_beat = (sec_per_beat * sample_rate as f64) as usize;

    for frame in 0..total_frames {
        let t = frame as f64 / sample_rate as f64;
        let beat_total = t / sec_per_beat;
        let bar_idx = (beat_total / beats_per_bar as f64).floor() as usize;
        let beat_in_bar = (beat_total % beats_per_bar as f64).floor() as usize;
        let beat_frac = (beat_total.fract()) as f32;

        // Distinct section energy so you can clearly hear Intro vs Verse vs Chorus vs Bridge!
        let section_energy = match bar_idx {
            0..=3 => 0.35_f32,   // Intro: warm soft pad
            4..=11 => 0.55_f32,  // Verse: pad + light bass
            12..=19 => 0.90_f32, // Chorus: fuller chords + octave + drums
            20..=27 => 0.78_f32, // Bridge: driving groove
            _ => 0.45_f32,       // Outro
        };

        let chord_idx = bar_idx % 4;
        let pad = &pad_triads_hz[chord_idx];
        let tf = t as f32;

        // Warm detuned worship pad on Music stem.
        let mut pad_sample = 0.0_f32;
        for &freq in pad {
            pad_sample += (tf * freq * TAU).sin() * 0.14;
            pad_sample += (tf * (freq * 1.002) * TAU).sin() * 0.07;
        }
        if bar_idx >= 12 && bar_idx < 28 {
            // Brighter upper shimmer in Chorus/Bridge.
            pad_sample += (tf * (pad[2] * 2.0) * TAU).sin() * 0.06;
        }
        let m_val = pad_sample * section_energy;
        music[frame * 2] = m_val;
        music[frame * 2 + 1] = m_val * 0.96;

        // Bass stem (comes in from Verse onward).
        if bar_idx >= 4 {
            let root = chord_roots_hz[chord_idx];
            let env = (-beat_frac * 2.5).exp() * 0.55 + 0.45;
            let b_val = (tf * root * TAU).sin() * 0.35 * env * section_energy;
            bass[frame * 2] = b_val;
            bass[frame * 2 + 1] = b_val;
        }

        // Kick/snare/pulse on Drums stem (from Bar 4 onward, unpitched).
        if bar_idx >= 4 {
            let sample_in_beat = frame % frames_per_beat.max(1);
            let s_t = sample_in_beat as f32 / sample_rate as f32;
            let mut d_val = 0.0_f32;
            if beat_in_bar == 0 || (bar_idx >= 12 && beat_in_bar == 2) {
                // Kick thump
                let pitch = 110.0 * (-s_t * 28.0).exp() + 48.0;
                d_val += (s_t * pitch * TAU).sin() * (-s_t * 14.0).exp() * 0.48;
            }
            if bar_idx >= 12 && (beat_in_bar == 1 || beat_in_bar == 3) {
                // Snare/rim transient
                d_val += (s_t * 220.0 * TAU).sin() * (-s_t * 30.0).exp() * 0.25;
            }
            drums[frame * 2] = d_val;
            drums[frame * 2 + 1] = d_val;
        }
    }

    let mut stems_map = HashMap::new();
    stems_map.insert(
        "drums".into(),
        StemDef::Detailed {
            file: "drums.wav".into(),
            pitched: false,
            default_gain: 0.9,
        },
    );
    stems_map.insert(
        "bass".into(),
        StemDef::Detailed {
            file: "bass.wav".into(),
            pitched: true,
            default_gain: 0.9,
        },
    );
    stems_map.insert(
        "music".into(),
        StemDef::Detailed {
            file: "music.wav".into(),
            pitched: true,
            default_gain: 0.9,
        },
    );

    let manifest = SongManifest {
        id: "demo-praise".into(),
        title: "Psalm 23 Demo (72 BPM)".into(),
        artist: "Psalm Stage Built-in Test".into(),
        bpm,
        time_signature: [4, 4],
        first_downbeat_sec: 0.0,
        original_key: "G".into(),
        beats: vec![],
        sections: vec![
            SectionDef {
                id: "intro".into(),
                name: "Intro".into(),
                start_bar: 1,
                end_bar: 5,
                color: Some("slate".into()),
            },
            SectionDef {
                id: "verse".into(),
                name: "Verse 1".into(),
                start_bar: 5,
                end_bar: 13,
                color: Some("sky".into()),
            },
            SectionDef {
                id: "chorus".into(),
                name: "Chorus".into(),
                start_bar: 13,
                end_bar: 21,
                color: Some("emerald".into()),
            },
            SectionDef {
                id: "bridge".into(),
                name: "Bridge".into(),
                start_bar: 21,
                end_bar: 29,
                color: Some("amber".into()),
            },
            SectionDef {
                id: "outro".into(),
                name: "Outro".into(),
                start_bar: 29,
                end_bar: 33,
                color: Some("violet".into()),
            },
        ],
        stems: stems_map,
    };

    LoadedSong {
        manifest,
        sample_rate,
        stems: vec![
            DecodedStem {
                name: "drums".into(),
                pitched: false,
                is_cues: false,
                gain: 0.9,
                muted: false,
                samples: drums,
            },
            DecodedStem {
                name: "bass".into(),
                pitched: true,
                is_cues: false,
                gain: 0.9,
                muted: false,
                samples: bass,
            },
            DecodedStem {
                name: "music".into(),
                pitched: true,
                is_cues: false,
                gain: 0.9,
                muted: false,
                samples: music,
            },
        ],
        total_frames,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_demo_song() {
        let song = generate_demo_song(48000);
        assert_eq!(song.sample_rate, 48000);
        assert_eq!(song.manifest.bpm, 72.0);
        assert_eq!(song.manifest.time_signature, [4, 4]);
        assert_eq!(song.stems.len(), 3);
        assert!(song.total_frames > 0);
        assert_eq!(song.manifest.sections.len(), 5);

        let drums = song.stems.iter().find(|s| s.name == "drums").unwrap();
        assert!(!drums.pitched);
        let music = song.stems.iter().find(|s| s.name == "music").unwrap();
        assert!(music.pitched);
    }

    #[test]
    fn test_resample_stereo_linear() {
        let input = vec![0.0, 0.0, 1.0, 1.0]; // 2 frames stereo
        let resampled = resample_stereo_linear(&input, 44100, 88200);
        assert_eq!(resampled.len(), 8); // 4 frames stereo
    }
}
