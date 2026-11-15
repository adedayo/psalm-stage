use std::process::Command;

use psalm_audio_core::manifest::SectionDef;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YouTubeChapter {
    pub title: String,
    pub start_time: f64,
    pub end_time: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeMetadata {
    pub title: String,
    pub uploader: String,
    pub duration_sec: f64,
    pub webpage_url: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub chapters: Vec<YouTubeChapter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct YouTubeSearchCandidate {
    pub video_id: String,
    pub title: String,
    pub channel: String,
    pub duration_sec: f64,
    pub webpage_url: String,
    pub is_live: bool,
    pub score: f32,
}

pub fn probe_youtube(yt_dlp_bin: &str, url: &str) -> Result<YouTubeMetadata, String> {
    let output = Command::new(yt_dlp_bin)
        .args(["--dump-single-json", "--no-playlist", "--no-warnings", url])
        .output()
        .map_err(|e| format!("Failed to execute yt-dlp: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp metadata lookup failed: {}", err.trim()));
    }

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("Invalid yt-dlp metadata JSON: {e}"))?;

    Ok(parse_youtube_metadata_value(&json, url))
}

pub fn parse_youtube_metadata_value(json: &serde_json::Value, fallback_url: &str) -> YouTubeMetadata {
    let chapters = json
        .get("chapters")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let title = item.get("title")?.as_str()?.trim().to_string();
                    let start_time = item.get("start_time")?.as_f64()?;
                    let end_time = item.get("end_time")?.as_f64()?;
                    if title.is_empty() || end_time <= start_time {
                        return None;
                    }
                    Some(YouTubeChapter {
                        title,
                        start_time,
                        end_time,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    YouTubeMetadata {
        title: json
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("YouTube Audio")
            .to_string(),
        uploader: json
            .get("uploader")
            .or_else(|| json.get("channel"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        duration_sec: json.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0),
        webpage_url: json
            .get("webpage_url")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_url)
            .to_string(),
        chapters,
    }
}

/// Searches YouTube via `yt-dlp` flat-playlist JSON and returns ranked track candidates.
pub fn search_youtube(
    yt_dlp_bin: &str,
    query: &str,
    limit: usize,
) -> Result<Vec<YouTubeSearchCandidate>, String> {
    let clamped_limit = limit.clamp(1, 10);
    let search_expr = format!("ytsearch{clamped_limit}:{query}");
    let output = Command::new(yt_dlp_bin)
        .args(["--dump-json", "--flat-playlist", "--no-warnings", &search_expr])
        .output()
        .map_err(|e| format!("Failed to execute yt-dlp search: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp search failed: {}", err.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(parse_youtube_search_json(&stdout, query))
}

/// Parses newline-delimited JSON objects emitted by `yt-dlp --dump-json --flat-playlist`,
/// filtering/downranking obvious shorts, compilations, or livestreams.
pub fn parse_youtube_search_json(ndjson: &str, query: &str) -> Vec<YouTubeSearchCandidate> {
    let q_lower = query.to_ascii_lowercase();
    let wants_live = q_lower.contains("live");
    let mut candidates = Vec::new();

    for line in ndjson.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            continue;
        };
        let Some(video_id) = val.get("id").and_then(|v| v.as_str()).map(str::trim) else {
            continue;
        };
        if video_id.is_empty() {
            continue;
        }
        let title = val
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .trim()
            .to_string();
        let channel = val
            .get("channel")
            .or_else(|| val.get("uploader"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let duration_sec = val.get("duration").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let is_live = val
            .get("is_live")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            || val
                .get("live_status")
                .and_then(|v| v.as_str())
                .is_some_and(|s| s == "is_live");
        let webpage_url = val
            .get("url")
            .or_else(|| val.get("webpage_url"))
            .and_then(|v| v.as_str())
            .map(|u| {
                if u.starts_with("http") {
                    u.to_string()
                } else {
                    format!("https://www.youtube.com/watch?v={u}")
                }
            })
            .unwrap_or_else(|| format!("https://www.youtube.com/watch?v={video_id}"));

        // Score candidate suitability for live worship backing/stem import
        let mut score: f32 = 0.65;
        let t_lower = title.to_ascii_lowercase();
        let c_lower = channel.to_ascii_lowercase();

        if (90.0..=600.0).contains(&duration_sec) {
            score += 0.2;
        } else if duration_sec > 0.0 && duration_sec < 90.0 {
            // Likely YouTube Short or clip
            score -= 0.4;
        } else if duration_sec > 900.0 {
            // Likely full sermon, concert, or compilation
            score -= 0.45;
        }

        if is_live && !wants_live {
            score -= 0.25;
        }
        if t_lower.contains("reaction") || t_lower.contains("tutorial") || t_lower.contains("lesson")
        {
            score -= 0.25;
        }
        if t_lower.contains("official") || c_lower.contains("topic") || c_lower.contains("vevo") {
            score += 0.1;
        }

        candidates.push(YouTubeSearchCandidate {
            video_id: video_id.to_string(),
            title,
            channel,
            duration_sec,
            webpage_url,
            is_live,
            score: score.clamp(0.0, 1.0),
        });
    }

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    candidates
}

fn section_color_for_name(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("chorus") {
        "amber"
    } else if lower.contains("bridge") {
        "violet"
    } else if lower.contains("pre") {
        "cyan"
    } else if lower.contains("verse") {
        "sky"
    } else if lower.contains("outro") || lower.contains("tag") {
        "emerald"
    } else {
        "slate"
    }
}

/// Converts YouTube chapters into bar-aligned `SectionDef` markers when valid chapters exist.
pub fn chapters_to_sections(
    chapters: &[YouTubeChapter],
    bpm: f64,
    beats_per_bar: u8,
    first_downbeat_sec: f64,
    total_duration_sec: f64,
) -> Vec<SectionDef> {
    if chapters.is_empty() || bpm <= 0.0 || beats_per_bar == 0 {
        return Vec::new();
    }
    let spb = (60.0 / bpm) * (beats_per_bar as f64);
    let mut out: Vec<SectionDef> = Vec::new();

    for (idx, ch) in chapters.iter().enumerate() {
        let start_bar = (((ch.start_time - first_downbeat_sec).max(0.0) / spb).round() as u32) + 1;
        let end_bar_raw =
            (((ch.end_time - first_downbeat_sec).max(0.0) / spb).round() as u32) + 1;
        let adjusted_start = if let Some(prev) = out.last() {
            start_bar.max(prev.end_bar)
        } else {
            start_bar.max(1)
        };
        let adjusted_end = end_bar_raw.max(adjusted_start + 1);
        let slug: String = ch
            .title
            .to_ascii_lowercase()
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
            .collect();
        let clean_slug = slug.trim_matches('-').to_string();
        let id = if clean_slug.is_empty() {
            format!("sec-{}", idx + 1)
        } else {
            format!("{clean_slug}-{}", idx + 1)
        };
        out.push(SectionDef {
            id,
            name: ch.title.clone(),
            start_bar: adjusted_start,
            end_bar: adjusted_end,
            start_sec: Some((ch.start_time * 1000.0).round() / 1000.0),
            end_sec: Some((ch.end_time.min(total_duration_sec.max(ch.end_time)) * 1000.0).round() / 1000.0),
            color: Some(section_color_for_name(&ch.title).to_string()),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_youtube_search_results_and_filter_outliers() {
        let ndjson = r#"
{"id":"short123456","title":"Gratitude clip #shorts","channel":"Random Clips","duration":42.0,"is_live":false}
{"id":"sermon12345","title":"Full Sunday Service 2 Hours","channel":"Church Stream","duration":5400.0,"is_live":false}
{"id":"off12345678","title":"Gratitude (Official Audio)","channel":"Brandon Lake","duration":324.0,"is_live":false}
{"id":"react123456","title":"Vocal Coach Reaction to Gratitude","channel":"Voice Guy","duration":410.0,"is_live":false}
"#;
        let ranked = parse_youtube_search_json(ndjson, "Gratitude Brandon Lake");
        assert_eq!(ranked.len(), 4);
        assert_eq!(ranked[0].video_id, "off12345678");
        assert!(ranked[0].score > ranked[1].score);
        assert!(ranked.iter().find(|c| c.video_id == "short123456").unwrap().score < 0.5);
        assert!(ranked.iter().find(|c| c.video_id == "sermon12345").unwrap().score < 0.5);
    }

    #[test]
    fn test_chapters_to_sections_produces_monotonic_bars() {
        let chapters = vec![
            YouTubeChapter {
                title: "Intro".into(),
                start_time: 0.0,
                end_time: 8.0,
            },
            YouTubeChapter {
                title: "Chorus".into(),
                start_time: 8.0,
                end_time: 24.0,
            },
        ];
        // 120 BPM, 4/4 => 2.0 sec/bar
        let sections = chapters_to_sections(&chapters, 120.0, 4, 0.0, 30.0);
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].start_bar, 1);
        assert_eq!(sections[0].end_bar, 5);
        assert_eq!(sections[1].start_bar, 5);
        assert_eq!(sections[1].end_bar, 13);
        assert_eq!(sections[1].color.as_deref(), Some("amber"));
    }
}

