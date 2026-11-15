use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::loader::{
    classify_stem_filename, discover_audio_files, load_setlist_manifest, probe_audio_duration_sec,
};
use crate::manifest::{canonicalize_source_ref, ImportProvenance, SongManifest};
use crate::pad::key_to_pitch_class;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryConfig {
    pub library_root: String,
    #[serde(default = "default_setlists_root_string")]
    pub setlists_root: String,
    #[serde(default)]
    pub linked_folders: Vec<String>,
    #[serde(default)]
    pub recent_setlists: Vec<String>,
}

impl Default for LibraryConfig {
    fn default() -> Self {
        Self {
            library_root: default_library_root().to_string_lossy().to_string(),
            setlists_root: default_setlists_root_string(),
            linked_folders: Vec::new(),
            recent_setlists: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetlistSummary {
    pub path: String,
    pub id: String,
    pub name: String,
    pub song_count: usize,
    pub song_titles: Vec<String>,
    pub total_estimated_sec: f64,
    pub modified_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryEntryStatus {
    Ready,
    NeedsSetup,
    MissingStems,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySongEntry {
    pub dir: String,
    pub status: LibraryEntryStatus,
    pub song_id: String,
    pub title: String,
    pub artist: String,
    pub original_key: String,
    pub bpm: f64,
    pub time_signature: [u8; 2],
    pub estimated_duration_sec: f64,
    pub section_names: Vec<String>,
    pub stem_names: Vec<String>,
    pub has_cues: bool,
    pub missing_files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<ImportProvenance>,
}

pub fn default_library_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Music").join("PsalmStage").join("Library")
}

pub fn default_setlists_root() -> PathBuf {
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Music").join("PsalmStage").join("Setlists")
}

pub fn default_setlists_root_string() -> String {
    default_setlists_root().to_string_lossy().to_string()
}

pub fn normalize_library_config(mut config: LibraryConfig) -> LibraryConfig {
    if config.library_root.trim().is_empty() {
        config.library_root = default_library_root().to_string_lossy().to_string();
    }
    if config.setlists_root.trim().is_empty() {
        config.setlists_root = default_setlists_root_string();
    }
    let mut seen = HashSet::new();
    seen.insert(config.library_root.clone());
    config.linked_folders.retain(|folder| {
        let trimmed = folder.trim();
        !trimmed.is_empty() && seen.insert(trimmed.to_string())
    });
    let mut seen_recent = HashSet::new();
    config.recent_setlists.retain(|p| {
        let trimmed = p.trim();
        !trimmed.is_empty() && seen_recent.insert(trimmed.to_string())
    });
    config.recent_setlists.truncate(12);
    config
}

pub fn list_managed_setlists(config: &LibraryConfig) -> Vec<SetlistSummary> {
    let entries = scan_song_library(config);
    let mut candidate_paths: Vec<PathBuf> = Vec::new();
    let root = PathBuf::from(&config.setlists_root);
    if root.exists() && root.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&root) {
            for item in rd.flatten() {
                let p = item.path();
                if p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("json"))
                {
                    candidate_paths.push(p);
                }
            }
        }
    }
    for recent in &config.recent_setlists {
        let p = PathBuf::from(recent);
        if p.exists() && p.is_file() {
            candidate_paths.push(p);
        }
    }

    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut summaries: Vec<SetlistSummary> = Vec::new();
    for path in candidate_paths {
        let canon = std::fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
        if !seen.insert(canon) {
            continue;
        }
        let Ok(manifest) = load_setlist_manifest(&path) else {
            continue;
        };
        let modified_ms = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut total_estimated_sec = 0.0_f64;
        let mut song_titles = Vec::with_capacity(manifest.items.len());
        for item in &manifest.items {
            let lib_match = entries.iter().find(|e| {
                item.song_dir.as_deref() == Some(e.dir.as_str()) || e.song_id == item.song_id
            });
            let title = item
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .or_else(|| lib_match.map(|e| e.title.clone()))
                .unwrap_or_else(|| item.song_id.clone());
            song_titles.push(title);

            let base_dur = lib_match.map(|e| e.estimated_duration_sec).unwrap_or(240.0);
            let base_bpm = item
                .base_bpm
                .or_else(|| lib_match.map(|e| e.bpm))
                .unwrap_or(120.0);
            let eff_bpm = item.bpm_override.unwrap_or(base_bpm).max(1.0);
            total_estimated_sec += base_dur * (base_bpm / eff_bpm);
        }

        summaries.push(SetlistSummary {
            path: path.to_string_lossy().to_string(),
            id: manifest.id,
            name: manifest.name,
            song_count: manifest.items.len(),
            song_titles,
            total_estimated_sec,
            modified_ms,
        });
    }

    summaries.sort_by(|a, b| {
        b.modified_ms
            .cmp(&a.modified_ms)
            .then_with(|| a.name.to_ascii_lowercase().cmp(&b.name.to_ascii_lowercase()))
    });
    summaries
}

/// Computes the shortest signed semitone delta (`-6..=+6`) from `original_key` to `target_key`.
/// Returns an `Err` when either key cannot be parsed (e.g., `"Unknown"`).
pub fn compute_semitone_shift(original_key: &str, target_key: &str) -> Result<i32, String> {
    let orig_pc = key_to_pitch_class(original_key)
        .ok_or_else(|| format!("Original key '{original_key}' is unknown or invalid"))?;
    let target_pc = key_to_pitch_class(target_key)
        .ok_or_else(|| format!("Target key '{target_key}' is unknown or invalid"))?;
    let mut diff = (target_pc as i32) - (orig_pc as i32);
    if diff > 6 {
        diff -= 12;
    } else if diff < -6 {
        diff += 12;
    }
    Ok(diff.clamp(-12, 12))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LibraryMatchReason {
    ExactSourceRef,
    TitleAndArtist,
    TitleFuzzy,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryMatchResult {
    pub entry: LibrarySongEntry,
    pub score: f32,
    pub reason: LibraryMatchReason,
}

pub fn normalize_song_query_text(raw: &str) -> String {
    // Remove bracketed/parenthetical suffixes like "(Live)", "[Official Video]"
    let mut stripped = String::with_capacity(raw.len());
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    for ch in raw.chars() {
        match ch {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            _ if paren_depth == 0 && bracket_depth == 0 => stripped.push(ch),
            _ => {}
        }
    }

    let lower = stripped
        .replace(" - topic", "")
        .to_ascii_lowercase();
    let cleaned: String = lower
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned
        .split_whitespace()
        .filter(|tok| {
            !matches!(
                *tok,
                "official" | "video" | "audio" | "lyric" | "lyrics" | "live" | "ft" | "feat"
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn levenshtein_similarity(a: &str, b: &str) -> f32 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut curr = vec![0usize; b_chars.len() + 1];
    for (i, &ac) in a_chars.iter().enumerate() {
        curr[0] = i + 1;
        for (j, &bc) in b_chars.iter().enumerate() {
            let cost = usize::from(ac != bc);
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    let dist = prev[b_chars.len()] as f32;
    let max_len = a_chars.len().max(b_chars.len()) as f32;
    (1.0 - dist / max_len).max(0.0)
}

fn token_jaccard_similarity(a: &str, b: &str) -> f32 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();
    if set_a.is_empty() || set_b.is_empty() {
        return 0.0;
    }
    let inter = set_a.intersection(&set_b).count() as f32;
    let union = set_a.union(&set_b).count() as f32;
    inter / union
}

/// Fuzzy-matches a requested song against the indexed Library entries.
/// Ranks exact canonical source matches first, then normalized title/artist similarity,
/// breaking ties by readiness (`Ready > NeedsSetup > MissingStems`).
pub fn match_library_songs(
    entries: &[LibrarySongEntry],
    title: &str,
    artist: Option<&str>,
    youtube_url: Option<&str>,
) -> Vec<LibraryMatchResult> {
    let target_source_ref = youtube_url
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .map(|u| canonicalize_source_ref("youtube", u));

    let norm_query_title = normalize_song_query_text(title);
    let norm_query_artist = artist
        .map(normalize_song_query_text)
        .filter(|s| !s.is_empty());

    let mut results = Vec::new();
    for entry in entries {
        if let Some(ref target_ref) = target_source_ref {
            if entry
                .provenance
                .as_ref()
                .is_some_and(|p| p.source_ref == *target_ref)
            {
                results.push(LibraryMatchResult {
                    entry: entry.clone(),
                    score: 1.0,
                    reason: LibraryMatchReason::ExactSourceRef,
                });
                continue;
            }
        }

        let norm_entry_title = normalize_song_query_text(&entry.title);
        if norm_query_title.is_empty() || norm_entry_title.is_empty() {
            continue;
        }

        let title_lev = levenshtein_similarity(&norm_query_title, &norm_entry_title);
        let title_jaccard = token_jaccard_similarity(&norm_query_title, &norm_entry_title);
        let contains_boost = if norm_entry_title == norm_query_title {
            1.0
        } else if norm_entry_title.contains(&norm_query_title)
            || norm_query_title.contains(&norm_entry_title)
        {
            0.88
        } else {
            0.0
        };
        let title_score = title_lev.max(title_jaccard).max(contains_boost);
        if title_score < 0.55 {
            continue;
        }

        let (score, reason) = if let Some(ref q_artist) = norm_query_artist {
            let e_artist = normalize_song_query_text(&entry.artist);
            if !e_artist.is_empty() {
                let artist_sim = levenshtein_similarity(q_artist, &e_artist)
                    .max(token_jaccard_similarity(q_artist, &e_artist));
                let combined = (title_score * 0.78) + (artist_sim * 0.22);
                (combined.min(0.99), LibraryMatchReason::TitleAndArtist)
            } else {
                ((title_score * 0.94).min(0.96), LibraryMatchReason::TitleFuzzy)
            }
        } else {
            ((title_score * 0.97).min(0.97), LibraryMatchReason::TitleFuzzy)
        };

        if score >= 0.55 {
            results.push(LibraryMatchResult {
                entry: entry.clone(),
                score,
                reason,
            });
        }
    }

    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| status_rank(a.entry.status).cmp(&status_rank(b.entry.status)))
            .then_with(|| a.entry.title.cmp(&b.entry.title))
    });
    results
}

pub fn scan_song_library(config: &LibraryConfig) -> Vec<LibrarySongEntry> {
    let mut roots = Vec::new();
    roots.push(PathBuf::from(&config.library_root));
    for extra in &config.linked_folders {
        roots.push(PathBuf::from(extra));
    }

    let mut visited_dirs: HashSet<PathBuf> = HashSet::new();
    let mut entries: Vec<LibrarySongEntry> = Vec::new();

    for root in roots {
        if root.exists() && root.is_dir() {
            walk_library_dir(&root, 0, 4, &mut visited_dirs, &mut entries);
        }
    }

    entries.sort_by(|a, b| {
        status_rank(a.status)
            .cmp(&status_rank(b.status))
            .then_with(|| {
                a.title
                    .to_ascii_lowercase()
                    .cmp(&b.title.to_ascii_lowercase())
            })
    });
    entries
}

fn status_rank(status: LibraryEntryStatus) -> u8 {
    match status {
        LibraryEntryStatus::Ready => 0,
        LibraryEntryStatus::NeedsSetup => 1,
        LibraryEntryStatus::MissingStems => 2,
    }
}

fn walk_library_dir(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    visited: &mut HashSet<PathBuf>,
    out: &mut Vec<LibrarySongEntry>,
) {
    let canon = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canon) {
        return;
    }

    if let Some(entry) = inspect_library_candidate(dir) {
        out.push(entry);
        // Once a folder is a song bundle, don't descend further inside it.
        return;
    }

    if depth >= max_depth {
        return;
    }

    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for child in read_dir.flatten() {
        let path = child.path();
        if !path.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.starts_with('.') || name == "node_modules" || name == "target" {
            continue;
        }
        walk_library_dir(&path, depth + 1, max_depth, visited, out);
    }
}

fn inspect_library_candidate(dir: &Path) -> Option<LibrarySongEntry> {
    let manifest_path = dir.join("manifest.json");
    if manifest_path.exists() {
        if let Ok(manifest) = SongManifest::from_file(&manifest_path) {
            let mut missing_files = Vec::new();
            let mut max_duration_sec = 0.0_f64;
            let mut has_cues = false;
            let mut stem_names: Vec<String> = manifest.stems.keys().cloned().collect();
            stem_names.sort();

            for (key, stem_def) in &manifest.stems {
                if stem_def.is_cues(key) {
                    has_cues = true;
                }
                let stem_path = dir.join(stem_def.file_name());
                if !stem_path.exists() {
                    missing_files.push(stem_def.file_name().to_string());
                } else if max_duration_sec <= 0.0 {
                    if let Ok(dur) = probe_audio_duration_sec(&stem_path) {
                        max_duration_sec = max_duration_sec.max(dur);
                    }
                }
            }

            if max_duration_sec <= 0.0 {
                if let Some(last_sec) = manifest.sections.iter().map(|s| s.end_bar).max() {
                    max_duration_sec = manifest.bar_to_seconds(last_sec);
                }
            }

            let status = if missing_files.is_empty() && !manifest.stems.is_empty() {
                LibraryEntryStatus::Ready
            } else {
                LibraryEntryStatus::MissingStems
            };

            return Some(LibrarySongEntry {
                dir: dir.to_string_lossy().to_string(),
                status,
                song_id: manifest.id,
                title: manifest.title,
                artist: manifest.artist,
                original_key: manifest.original_key,
                bpm: manifest.bpm,
                time_signature: manifest.time_signature,
                estimated_duration_sec: max_duration_sec,
                section_names: manifest.sections.iter().map(|s| s.name.clone()).collect(),
                stem_names,
                has_cues,
                missing_files,
                provenance: manifest.provenance,
            });
        }
    }

    let audio_files = discover_audio_files(dir).ok()?;
    if audio_files.is_empty() {
        return None;
    }

    let folder_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("Untitled Song")
        .to_string();

    let mut max_duration_sec = 0.0_f64;
    if let Some(first) = audio_files.first() {
        if let Ok(dur) = probe_audio_duration_sec(&dir.join(first)) {
            max_duration_sec = dur;
        }
    }

    let stem_names: Vec<String> = audio_files
        .iter()
        .map(|f| {
            Path::new(f)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(f)
                .to_string()
        })
        .collect();

    let has_cues = stem_names
        .iter()
        .any(|s| classify_stem_filename(s).1);

    Some(LibrarySongEntry {
        dir: dir.to_string_lossy().to_string(),
        status: LibraryEntryStatus::NeedsSetup,
        song_id: folder_name.to_ascii_lowercase().replace(' ', "-"),
        title: folder_name,
        artist: String::new(),
        original_key: "Unknown".to_string(),
        bpm: 120.0,
        time_signature: [4, 4],
        estimated_duration_sec: max_duration_sec,
        section_names: Vec::new(),
        stem_names,
        has_cues,
        missing_files: Vec::new(),
        provenance: None,
    })
}

/// Normalizes UVR (`audio-separator`) and Demucs output stem filenames into clean Psalm Stage stem roles.
/// Returns `(canonical_stem_key, pitched, is_cues, default_gain)`.
pub fn normalize_separated_stem_role(filename: &str) -> (String, bool, bool, f32) {
    let raw_stem = Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename)
        .to_ascii_lowercase();

    if raw_stem.contains("(drums)") || raw_stem == "drums" || raw_stem.ends_with("_drums") {
        return ("drums".to_string(), false, false, 0.95);
    }
    if raw_stem.contains("(bass)") || raw_stem == "bass" || raw_stem.ends_with("_bass") {
        return ("bass".to_string(), true, false, 0.92);
    }
    if raw_stem.contains("(vocals)") || raw_stem == "vocals" || raw_stem.ends_with("_vocals") {
        return ("vocals".to_string(), true, false, 0.85);
    }
    if raw_stem.contains("(guitar)") || raw_stem == "guitar" || raw_stem.ends_with("_guitar") {
        return ("guitar".to_string(), true, false, 0.9);
    }
    if raw_stem.contains("(piano)") || raw_stem == "piano" || raw_stem.ends_with("_piano") {
        return ("piano".to_string(), true, false, 0.9);
    }
    if raw_stem.contains("(instrumental)")
        || raw_stem.contains("(no_vocals)")
        || raw_stem == "instrumental"
        || raw_stem == "no_vocals"
    {
        return ("instrumental".to_string(), true, false, 0.95);
    }
    if raw_stem.contains("(other)") || raw_stem == "other" || raw_stem.ends_with("_other") {
        return ("music".to_string(), true, false, 0.9);
    }

    let (pitched, is_cues, gain) = classify_stem_filename(&raw_stem);
    let clean_key: String = raw_stem
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let trimmed = clean_key.trim_matches('_');
    let final_key = if trimmed.is_empty() {
        "music".to_string()
    } else {
        trimmed.to_string()
    };
    (final_key, pitched, is_cues, gain)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_minimal_wav(path: &Path) {
        let sample_rate: u32 = 44100;
        let frames: usize = 4410; // 0.1s
        let data_len = (frames * 2) as u32;
        let mut wav = Vec::with_capacity(44 + data_len as usize);
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
        wav.resize(44 + data_len as usize, 0);
        std::fs::write(path, wav).unwrap();
    }

    #[test]
    fn test_scan_library_discovers_ready_needs_setup_and_missing_stems() {
        let root = std::env::temp_dir().join("psalm_stage_test_library_scan");
        let _ = std::fs::remove_dir_all(&root);

        let ready_dir = root.join("Goodness of God");
        let setup_dir = root.join("Raw Import");
        let broken_dir = root.join("Missing Audio");
        std::fs::create_dir_all(&ready_dir).unwrap();
        std::fs::create_dir_all(&setup_dir).unwrap();
        std::fs::create_dir_all(&broken_dir).unwrap();

        write_minimal_wav(&ready_dir.join("drums.wav"));
        std::fs::write(
            ready_dir.join("manifest.json"),
            r#"{
                "id": "goodness-of-god",
                "title": "Goodness of God",
                "artist": "Bethel",
                "bpm": 63.0,
                "time_signature": [4, 4],
                "first_downbeat_sec": 0.0,
                "original_key": "Ab",
                "sections": [{"id":"v1","name":"Verse 1","start_bar":1,"end_bar":5}],
                "stems": {"drums": {"file": "drums.wav", "pitched": false, "default_gain": 0.95}}
            }"#,
        )
        .unwrap();

        write_minimal_wav(&setup_dir.join("track.wav"));

        std::fs::write(
            broken_dir.join("manifest.json"),
            r#"{
                "id": "missing",
                "title": "Missing Audio",
                "bpm": 72.0,
                "sections": [{"id":"i","name":"Intro","start_bar":1,"end_bar":5}],
                "stems": {"music": "does_not_exist.wav"}
            }"#,
        )
        .unwrap();

        let cfg = normalize_library_config(LibraryConfig {
            library_root: root.to_string_lossy().to_string(),
            setlists_root: root.join("Setlists").to_string_lossy().to_string(),
            linked_folders: vec![root.to_string_lossy().to_string()], // duplicate should dedup
            recent_setlists: vec![],
        });
        assert!(cfg.linked_folders.is_empty());

        let entries = scan_song_library(&cfg);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].status, LibraryEntryStatus::Ready);
        assert_eq!(entries[0].title, "Goodness of God");
        assert_eq!(entries[1].status, LibraryEntryStatus::NeedsSetup);
        assert_eq!(entries[1].title, "Raw Import");
        assert_eq!(entries[2].status, LibraryEntryStatus::MissingStems);
        assert_eq!(entries[2].missing_files, vec!["does_not_exist.wav"]);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_compute_semitone_shift_shortest_path_and_unknown() {
        assert_eq!(compute_semitone_shift("B", "G").unwrap(), -4);
        assert_eq!(compute_semitone_shift("G", "A").unwrap(), 2);
        assert_eq!(compute_semitone_shift("Db", "C#").unwrap(), 0);
        assert_eq!(compute_semitone_shift("F#m", "Am").unwrap(), 3);
        assert!(compute_semitone_shift("Unknown", "G").is_err());
        assert!(compute_semitone_shift("G", "Unknown").is_err());
    }

    #[test]
    fn test_fuzzy_library_matching_ranks_ready_and_normalizes_titles() {
        let ready_entry = LibrarySongEntry {
            dir: "/lib/gratitude-ready".into(),
            status: LibraryEntryStatus::Ready,
            song_id: "gratitude".into(),
            title: "Gratitude (Live)".into(),
            artist: "Brandon Lake".into(),
            original_key: "B".into(),
            bpm: 78.0,
            time_signature: [4, 4],
            estimated_duration_sec: 320.0,
            section_names: vec!["Intro".into()],
            stem_names: vec!["drums".into(), "bass".into()],
            has_cues: false,
            missing_files: vec![],
            provenance: Some(ImportProvenance {
                source_kind: "youtube".into(),
                source_ref: "youtube:dQw4w9WgXcQ".into(),
                original_source: None,
                separation_engine: "AudioSeparator".into(),
                stem_preset: "FourStems".into(),
                custom_model: None,
                needs_review: false,
                key_confidence: Some(0.9),
                review_notes: vec![],
            }),
        };
        let setup_entry = LibrarySongEntry {
            dir: "/lib/gratitude-raw".into(),
            status: LibraryEntryStatus::NeedsSetup,
            song_id: "gratitude-raw".into(),
            title: "Gratitude".into(),
            artist: "".into(),
            original_key: "Unknown".into(),
            bpm: 120.0,
            time_signature: [4, 4],
            estimated_duration_sec: 300.0,
            section_names: vec![],
            stem_names: vec!["track".into()],
            has_cues: false,
            missing_files: vec![],
            provenance: None,
        };

        let entries = vec![setup_entry, ready_entry];
        let matched = match_library_songs(&entries, "Gratitude", Some("Brandon Lake"), None);
        assert!(!matched.is_empty());
        assert_eq!(matched[0].entry.dir, "/lib/gratitude-ready");
        assert!(matched[0].score >= 0.85);

        let url_matched = match_library_songs(
            &entries,
            "Different Name",
            None,
            Some("https://youtu.be/dQw4w9WgXcQ?t=10"),
        );
        assert_eq!(url_matched[0].reason, LibraryMatchReason::ExactSourceRef);
        assert_eq!(url_matched[0].score, 1.0);
    }

    #[test]
    fn test_list_managed_setlists_discovers_and_computes_duration() {
        let root = std::env::temp_dir().join(format!("psalm_managed_setlists_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let lib_dir = root.join("Library");
        let setlists_dir = root.join("Setlists");
        std::fs::create_dir_all(&lib_dir).unwrap();
        std::fs::create_dir_all(&setlists_dir).unwrap();

        let setlist_path = setlists_dir.join("sunday-nov-15.json");
        std::fs::write(
            &setlist_path,
            r#"{
                "id": "sunday-nov-15",
                "name": "Sunday Nov 15",
                "items": [
                    { "song_id": "gratitude", "title": "Gratitude", "key_shift_semitones": 0 }
                ]
            }"#,
        )
        .unwrap();

        let cfg = normalize_library_config(LibraryConfig {
            library_root: lib_dir.to_string_lossy().to_string(),
            setlists_root: setlists_dir.to_string_lossy().to_string(),
            linked_folders: vec![],
            recent_setlists: vec![],
        });
        let found = list_managed_setlists(&cfg);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Sunday Nov 15");
        assert_eq!(found[0].song_count, 1);

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_normalize_separated_stem_role() {
        assert_eq!(
            normalize_separated_stem_role("Song_(Drums)_htdemucs_ft.wav"),
            ("drums".to_string(), false, false, 0.95)
        );
        assert_eq!(
            normalize_separated_stem_role("Song_(Instrumental)_UVR-MDX.wav"),
            ("instrumental".to_string(), true, false, 0.95)
        );
        assert_eq!(
            normalize_separated_stem_role("vocals.wav"),
            ("vocals".to_string(), true, false, 0.85)
        );
        assert_eq!(
            normalize_separated_stem_role("other.wav"),
            ("music".to_string(), true, false, 0.9)
        );
    }
}

