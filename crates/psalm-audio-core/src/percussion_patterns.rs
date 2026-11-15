use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PercussionMode {
    #[default]
    Off,
    Layer,
    PercussionOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PercussionStyle {
    #[default]
    Worship44,
    Worship68,
    Afrobeat,
    HighlifePraise,
    Woro,
    Makossa,
    Amapiano,
    FunkPocket,
    GospelPraise,
    AcousticCajon,
    LatinCaribbean,
    ReggaeSoca,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GrooveIntensity {
    Low,
    #[default]
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PercussionInstrument {
    Kick,
    Snare,
    HiHat,
    HandDrums,
    Tambourine,
    Shaker,
    Claps,
    Clave,
    Bell,
}

pub const ALL_PERCUSSION_INSTRUMENTS: [PercussionInstrument; 9] = [
    PercussionInstrument::Kick,
    PercussionInstrument::Snare,
    PercussionInstrument::HiHat,
    PercussionInstrument::HandDrums,
    PercussionInstrument::Tambourine,
    PercussionInstrument::Shaker,
    PercussionInstrument::Claps,
    PercussionInstrument::Clave,
    PercussionInstrument::Bell,
];

impl PercussionInstrument {
    pub fn default_gain(self) -> f32 {
        match self {
            Self::Kick => 0.92,
            Self::Snare => 0.85,
            Self::HiHat => 0.65,
            Self::HandDrums => 0.85,
            Self::Tambourine => 0.72,
            Self::Shaker => 0.75,
            Self::Claps => 0.80,
            Self::Clave => 0.78,
            Self::Bell => 0.68,
        }
    }

    pub fn default_pan(self) -> f32 {
        match self {
            Self::Kick => 0.0,
            Self::Snare => -0.05,
            Self::HiHat => -0.28,
            Self::HandDrums => 0.22,
            Self::Tambourine => 0.36,
            Self::Shaker => -0.34,
            Self::Claps => 0.14,
            Self::Clave => 0.28,
            Self::Bell => -0.18,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            Self::Kick => 0,
            Self::Snare => 1,
            Self::HiHat => 2,
            Self::HandDrums => 3,
            Self::Tambourine => 4,
            Self::Shaker => 5,
            Self::Claps => 6,
            Self::Clave => 7,
            Self::Bell => 8,
        }
    }

    pub const fn stem_slug(self) -> &'static str {
        match self {
            Self::Kick => "kick",
            Self::Snare => "snare",
            Self::HiHat => "hihat",
            Self::HandDrums => "hand_drums",
            Self::Tambourine => "tambourine",
            Self::Shaker => "shaker",
            Self::Claps => "claps",
            Self::Clave => "clave",
            Self::Bell => "bell",
        }
    }

    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Kick => "Kick",
            Self::Snare => "Snare",
            Self::HiHat => "Hi-Hat",
            Self::HandDrums => "Hand Drums",
            Self::Tambourine => "Tambourine",
            Self::Shaker => "Shaker",
            Self::Claps => "Claps",
            Self::Clave => "Clave",
            Self::Bell => "Bell / Agogo",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum HitVariant {
    #[default]
    Standard,
    /// Open hi-hat, open tambourine hit, high agogo, talking drum scoop, or log drum low
    Accent,
    /// Rim / ghost snare, low bell, soft hand drum bass
    Soft,
    /// Amapiano deep woody log drum or African talking drum glide
    Special,
}

impl HitVariant {
    pub const ALL: [HitVariant; 4] = [
        HitVariant::Standard,
        HitVariant::Accent,
        HitVariant::Soft,
        HitVariant::Special,
    ];

    pub const fn index(self) -> usize {
        match self {
            Self::Standard => 0,
            Self::Accent => 1,
            Self::Soft => 2,
            Self::Special => 3,
        }
    }

    pub const fn file_suffix(self) -> &'static str {
        match self {
            Self::Standard => "",
            Self::Accent => "_accent",
            Self::Soft => "_soft",
            Self::Special => "_special",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PatternHit {
    /// 0-based step within the 2-bar cycle (`0..cycle_steps`)
    pub step: u8,
    pub instrument: PercussionInstrument,
    /// 0.0..=1.0 velocity
    pub velocity: f32,
    pub variant: HitVariant,
    pub min_intensity: GrooveIntensity,
}

/// Editable, serializable hit entry for user-created dynamic percussion loops.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CustomPatternHit {
    pub step: u8,
    pub instrument: PercussionInstrument,
    pub velocity: f32,
    #[serde(default)]
    pub variant: HitVariant,
    #[serde(default)]
    pub min_intensity: GrooveIntensity,
}

/// User-authored or cloned dynamic percussion loop pattern.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomPercussionPattern {
    pub id: String,
    pub name: String,
    #[serde(default = "default_steps_per_bar")]
    pub steps_per_bar: u8,
    #[serde(default = "default_bars_in_cycle")]
    pub bars_in_cycle: u8,
    #[serde(default)]
    pub swing: f32,
    #[serde(default)]
    pub is_compound: bool,
    #[serde(default)]
    pub hits: Vec<CustomPatternHit>,
}

fn default_steps_per_bar() -> u8 {
    16
}

fn default_bars_in_cycle() -> u8 {
    1
}

impl CustomPercussionPattern {
    pub fn clamped_and_sorted(mut self) -> Self {
        self.id = self.id.trim().to_string();
        if self.id.is_empty() {
            self.id = "custom-loop".to_string();
        }
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            self.name = "Custom Worship Loop".to_string();
        }
        self.steps_per_bar = self.steps_per_bar.clamp(4, 32);
        self.bars_in_cycle = self.bars_in_cycle.clamp(1, 4);
        self.swing = if self.swing.is_finite() {
            self.swing.clamp(0.0, 0.35)
        } else {
            0.0
        };
        let max_steps = (self.steps_per_bar as usize) * (self.bars_in_cycle as usize);
        self.hits.retain(|h| (h.step as usize) < max_steps);
        for h in &mut self.hits {
            h.velocity = if h.velocity.is_finite() {
                h.velocity.clamp(0.05, 1.0)
            } else {
                0.8
            };
        }
        self.hits.sort_by_key(|h| (h.step, h.instrument.index()));
        // Deduplicate multiple hits on the exact same (step, instrument), keeping the last entry
        let mut deduped: Vec<CustomPatternHit> = Vec::with_capacity(self.hits.len());
        for hit in self.hits {
            if let Some(last) = deduped.last_mut() {
                if last.step == hit.step && last.instrument == hit.instrument {
                    *last = hit;
                    continue;
                }
            }
            deduped.push(hit);
        }
        self.hits = deduped;
        self
    }

    /// Expands or slices the cycle to `target_bars` (`1..=4`), repeating existing bars when expanding.
    pub fn with_bars(mut self, target_bars: u8) -> Self {
        let next_bars = target_bars.clamp(1, 4);
        let spb = self.steps_per_bar.clamp(4, 32);
        let cur_bars = self.bars_in_cycle.clamp(1, 4);
        if next_bars > cur_bars && !self.hits.is_empty() {
            let cur_cycle_steps = (spb as usize) * (cur_bars as usize);
            let mut expanded = self.hits.clone();
            for bar_idx in cur_bars..next_bars {
                let src_bar = bar_idx % cur_bars;
                let src_start = (src_bar as usize) * (spb as usize);
                let src_end = src_start + (spb as usize);
                let dst_offset = (bar_idx as usize) * (spb as usize) - src_start;
                for h in &self.hits {
                    let s = h.step as usize;
                    if s >= src_start && s < src_end && s < cur_cycle_steps {
                        let mut copy = *h;
                        copy.step = (s + dst_offset) as u8;
                        expanded.push(copy);
                    }
                }
            }
            self.hits = expanded;
        }
        self.bars_in_cycle = next_bars;
        self.clamped_and_sorted()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StylePatternSpec {
    pub style: PercussionStyle,
    /// Steps per bar (16 for 4/4, 12 for 3/4 or 6/8, 24 for 12/8)
    pub steps_per_bar: u8,
    /// Number of bars before repeating (1 or 2)
    pub bars_in_cycle: u8,
    /// Micro-swing amount applied to off-beat even steps (0.0..=0.35)
    pub swing: f32,
    pub is_compound: bool,
    pub hits: &'static [PatternHit],
}

impl PercussionStyle {
    pub fn all() -> &'static [PercussionStyle] {
        &[
            Self::Worship44,
            Self::Worship68,
            Self::Afrobeat,
            Self::HighlifePraise,
            Self::Woro,
            Self::Makossa,
            Self::Amapiano,
            Self::FunkPocket,
            Self::GospelPraise,
            Self::AcousticCajon,
            Self::LatinCaribbean,
            Self::ReggaeSoca,
        ]
    }

    pub fn pattern(self) -> StylePatternSpec {
        match self {
            Self::Worship44 => PATTERN_WORSHIP_44,
            Self::Worship68 => PATTERN_WORSHIP_68,
            Self::Afrobeat => PATTERN_AFROBEAT,
            Self::HighlifePraise => PATTERN_HIGHLIFE_PRAISE,
            Self::Woro => PATTERN_WORO,
            Self::Makossa => PATTERN_MAKOSSA,
            Self::Amapiano => PATTERN_AMAPIANO,
            Self::FunkPocket => PATTERN_FUNK_POCKET,
            Self::GospelPraise => PATTERN_GOSPEL_PRAISE,
            Self::AcousticCajon => PATTERN_ACOUSTIC_CAJON,
            Self::LatinCaribbean => PATTERN_LATIN_CARIBBEAN,
            Self::ReggaeSoca => PATTERN_REGGAE_SOCA,
        }
    }

    /// Converts a built-in style preset into an editable `CustomPercussionPattern`.
    pub fn to_custom_pattern(self, id: &str, name: &str) -> CustomPercussionPattern {
        let spec = self.pattern();
        CustomPercussionPattern {
            id: id.to_string(),
            name: name.to_string(),
            steps_per_bar: spec.steps_per_bar,
            bars_in_cycle: spec.bars_in_cycle,
            swing: spec.swing,
            is_compound: spec.is_compound,
            hits: spec
                .hits
                .iter()
                .map(|hit| CustomPatternHit {
                    step: hit.step,
                    instrument: hit.instrument,
                    velocity: hit.velocity,
                    variant: hit.variant,
                    min_intensity: hit.min_intensity,
                })
                .collect(),
        }
        .clamped_and_sorted()
    }
}

pub fn intensity_allows(min_req: GrooveIntensity, active: GrooveIntensity) -> bool {
    match (min_req, active) {
        (GrooveIntensity::Low, _) => true,
        (GrooveIntensity::Medium, GrooveIntensity::Medium | GrooveIntensity::High) => true,
        (GrooveIntensity::High, GrooveIntensity::High) => true,
        _ => false,
    }
}

const fn h(
    step: u8,
    instrument: PercussionInstrument,
    velocity: f32,
    variant: HitVariant,
    min_intensity: GrooveIntensity,
) -> PatternHit {
    PatternHit {
        step,
        instrument,
        velocity,
        variant,
        min_intensity,
    }
}

// Convenience aliases for concise pattern authoring
use GrooveIntensity::{High, Low, Medium};
use HitVariant::{Accent, Soft, Special, Standard};
use PercussionInstrument::{Bell, Claps, Clave, HandDrums, HiHat, Kick, Shaker, Snare, Tambourine};

// 1. Modern Worship 4/4 (1-bar 16th grid)
static WORSHIP_44_HITS: &[PatternHit] = &[
    // Low: shaker 8ths + rim/clave subtle + soft kick
    h(0, Kick, 0.85, Standard, Low),
    h(8, Kick, 0.78, Standard, Low),
    h(4, Clave, 0.45, Soft, Low),
    h(12, Clave, 0.50, Soft, Low),
    h(0, Shaker, 0.65, Accent, Low),
    h(2, Shaker, 0.42, Standard, Low),
    h(4, Shaker, 0.62, Accent, Low),
    h(6, Shaker, 0.40, Standard, Low),
    h(8, Shaker, 0.65, Accent, Low),
    h(10, Shaker, 0.42, Standard, Low),
    h(12, Shaker, 0.62, Accent, Low),
    h(14, Shaker, 0.44, Standard, Low),
    // Medium: snare backbeat + tambourine + 16th shaker fills
    h(4, Snare, 0.75, Standard, Medium),
    h(12, Snare, 0.82, Standard, Medium),
    h(10, Kick, 0.65, Soft, Medium),
    h(4, Tambourine, 0.72, Accent, Medium),
    h(12, Tambourine, 0.75, Accent, Medium),
    h(0, HiHat, 0.55, Standard, Medium),
    h(4, HiHat, 0.50, Standard, Medium),
    h(8, HiHat, 0.55, Standard, Medium),
    h(12, HiHat, 0.50, Standard, Medium),
    // High: driving claps, extra tambourine & 16th shaker + hand drum pulse
    h(4, Claps, 0.80, Standard, High),
    h(12, Claps, 0.85, Standard, High),
    h(2, Tambourine, 0.42, Standard, High),
    h(6, Tambourine, 0.45, Standard, High),
    h(10, Tambourine, 0.42, Standard, High),
    h(14, Tambourine, 0.48, Standard, High),
    h(14, HandDrums, 0.65, Standard, High),
    h(15, Shaker, 0.55, Standard, High),
];

const PATTERN_WORSHIP_44: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::Worship44,
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.0,
    is_compound: false,
    hits: WORSHIP_44_HITS,
};

// 2. Worship 6/8 (12 steps per bar: 2 dotted quarters x 6 sixteenths, or 6 eighths x 2)
// Grid: 0, 2, 4 (beat 1..3), 6, 8, 10 (beat 4..6)
static WORSHIP_68_HITS: &[PatternHit] = &[
    h(0, Kick, 0.88, Standard, Low),
    h(6, Kick, 0.68, Soft, Low),
    h(0, Shaker, 0.68, Accent, Low),
    h(2, Shaker, 0.42, Standard, Low),
    h(4, Shaker, 0.48, Standard, Low),
    h(6, Shaker, 0.65, Accent, Low),
    h(8, Shaker, 0.42, Standard, Low),
    h(10, Shaker, 0.50, Standard, Low),
    // Medium
    h(6, Snare, 0.72, Standard, Medium),
    h(6, Tambourine, 0.78, Accent, Medium),
    h(4, Kick, 0.58, Soft, Medium),
    h(0, HiHat, 0.52, Standard, Medium),
    h(2, HiHat, 0.35, Standard, Medium),
    h(4, HiHat, 0.38, Standard, Medium),
    h(6, HiHat, 0.50, Standard, Medium),
    h(8, HiHat, 0.35, Standard, Medium),
    h(10, HiHat, 0.40, Standard, Medium),
    // High
    h(6, Claps, 0.78, Standard, High),
    h(0, Tambourine, 0.52, Standard, High),
    h(3, Tambourine, 0.38, Soft, High),
    h(9, Tambourine, 0.42, Soft, High),
    h(10, HandDrums, 0.70, Standard, High),
    h(11, Snare, 0.45, Soft, High),
];

const PATTERN_WORSHIP_68: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::Worship68,
    steps_per_bar: 12,
    bars_in_cycle: 1,
    swing: 0.0,
    is_compound: true,
    hits: WORSHIP_68_HITS,
};

// 3. Afrobeat (2-bar 32-step cycle for authentic interlocking bell/clave & kick)
static AFROBEAT_HITS: &[PatternHit] = &[
    // 16th shaker interlock across 2 bars
    h(0, Shaker, 0.72, Accent, Low),
    h(1, Shaker, 0.38, Standard, Low),
    h(2, Shaker, 0.58, Standard, Low),
    h(3, Shaker, 0.42, Standard, Low),
    h(4, Shaker, 0.75, Accent, Low),
    h(5, Shaker, 0.38, Standard, Low),
    h(6, Shaker, 0.60, Standard, Low),
    h(7, Shaker, 0.42, Standard, Low),
    h(8, Shaker, 0.72, Accent, Low),
    h(9, Shaker, 0.38, Standard, Low),
    h(10, Shaker, 0.58, Standard, Low),
    h(11, Shaker, 0.42, Standard, Low),
    h(12, Shaker, 0.75, Accent, Low),
    h(13, Shaker, 0.38, Standard, Low),
    h(14, Shaker, 0.62, Standard, Low),
    h(15, Shaker, 0.45, Standard, Low),
    h(16, Shaker, 0.72, Accent, Low),
    h(17, Shaker, 0.38, Standard, Low),
    h(18, Shaker, 0.58, Standard, Low),
    h(19, Shaker, 0.42, Standard, Low),
    h(20, Shaker, 0.75, Accent, Low),
    h(21, Shaker, 0.38, Standard, Low),
    h(22, Shaker, 0.60, Standard, Low),
    h(23, Shaker, 0.42, Standard, Low),
    h(24, Shaker, 0.72, Accent, Low),
    h(25, Shaker, 0.38, Standard, Low),
    h(26, Shaker, 0.58, Standard, Low),
    h(27, Shaker, 0.42, Standard, Low),
    h(28, Shaker, 0.75, Accent, Low),
    h(29, Shaker, 0.38, Standard, Low),
    h(30, Shaker, 0.62, Standard, Low),
    h(31, Shaker, 0.45, Standard, Low),
    // Classic 3-2 Afrobeat Clave / Rim
    h(0, Clave, 0.85, Standard, Low),
    h(3, Clave, 0.82, Standard, Low),
    h(6, Clave, 0.78, Standard, Low),
    h(10, Clave, 0.80, Standard, Low),
    h(12, Clave, 0.85, Standard, Low),
    h(18, Clave, 0.78, Standard, Low),
    h(20, Clave, 0.82, Standard, Low),
    h(24, Clave, 0.80, Standard, Low),
    h(28, Clave, 0.78, Standard, Low),
    // Syncopated Afrobeat Kick
    h(0, Kick, 0.92, Standard, Low),
    h(6, Kick, 0.78, Standard, Low),
    h(8, Kick, 0.86, Standard, Low),
    h(16, Kick, 0.92, Standard, Low),
    h(22, Kick, 0.78, Standard, Low),
    h(24, Kick, 0.86, Standard, Low),
    // Medium: Snare rim/backbeat, HandDrums, HiHat
    h(4, Snare, 0.72, Soft, Medium),
    h(11, Snare, 0.68, Standard, Medium),
    h(14, Snare, 0.75, Standard, Medium),
    h(20, Snare, 0.72, Soft, Medium),
    h(27, Snare, 0.68, Standard, Medium),
    h(30, Snare, 0.78, Standard, Medium),
    h(2, HandDrums, 0.75, Standard, Medium),
    h(10, HandDrums, 0.82, Accent, Medium),
    h(18, HandDrums, 0.75, Standard, Medium),
    h(26, HandDrums, 0.80, Accent, Medium),
    h(28, HandDrums, 0.70, Soft, Medium),
    h(2, HiHat, 0.48, Accent, Medium),
    h(6, HiHat, 0.48, Accent, Medium),
    h(10, HiHat, 0.48, Accent, Medium),
    h(14, HiHat, 0.48, Accent, Medium),
    h(18, HiHat, 0.48, Accent, Medium),
    h(22, HiHat, 0.48, Accent, Medium),
    h(26, HiHat, 0.48, Accent, Medium),
    h(30, HiHat, 0.48, Accent, Medium),
    // High: Agogo Bell + Tambourine + Claps
    h(0, Bell, 0.80, Accent, High),
    h(3, Bell, 0.68, Soft, High),
    h(6, Bell, 0.75, Accent, High),
    h(10, Bell, 0.70, Soft, High),
    h(12, Bell, 0.78, Accent, High),
    h(18, Bell, 0.72, Soft, High),
    h(22, Bell, 0.78, Accent, High),
    h(26, Bell, 0.70, Soft, High),
    h(4, Tambourine, 0.68, Accent, High),
    h(12, Tambourine, 0.72, Accent, High),
    h(20, Tambourine, 0.68, Accent, High),
    h(28, Tambourine, 0.72, Accent, High),
    h(4, Claps, 0.75, Standard, High),
    h(12, Claps, 0.78, Standard, High),
    h(20, Claps, 0.75, Standard, High),
    h(28, Claps, 0.80, Standard, High),
];

const PATTERN_AFROBEAT: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::Afrobeat,
    steps_per_bar: 16,
    bars_in_cycle: 2,
    swing: 0.06,
    is_compound: false,
    hits: AFROBEAT_HITS,
};

// 4. Highlife Praise (4/4 joyful Nigerian/Ghanaian Praise)
static HIGHLIFE_HITS: &[PatternHit] = &[
    h(0, Kick, 0.90, Standard, Low),
    h(4, Kick, 0.84, Standard, Low),
    h(8, Kick, 0.90, Standard, Low),
    h(12, Kick, 0.86, Standard, Low),
    h(0, Shaker, 0.70, Accent, Low),
    h(2, Shaker, 0.45, Standard, Low),
    h(4, Shaker, 0.70, Accent, Low),
    h(6, Shaker, 0.45, Standard, Low),
    h(8, Shaker, 0.70, Accent, Low),
    h(10, Shaker, 0.45, Standard, Low),
    h(12, Shaker, 0.70, Accent, Low),
    h(14, Shaker, 0.48, Standard, Low),
    // Classic Highlife Bell / Clave timeline
    h(0, Clave, 0.82, Standard, Low),
    h(3, Clave, 0.78, Standard, Low),
    h(6, Clave, 0.80, Standard, Low),
    h(10, Clave, 0.78, Standard, Low),
    h(12, Clave, 0.82, Standard, Low),
    // Medium: Snare + Claps + Tambourine + HandDrums
    h(4, Snare, 0.78, Standard, Medium),
    h(12, Snare, 0.82, Standard, Medium),
    h(4, Claps, 0.78, Standard, Medium),
    h(12, Claps, 0.82, Standard, Medium),
    h(2, Tambourine, 0.55, Standard, Medium),
    h(6, Tambourine, 0.58, Standard, Medium),
    h(10, Tambourine, 0.55, Standard, Medium),
    h(14, Tambourine, 0.62, Accent, Medium),
    h(6, HandDrums, 0.75, Special, Medium),
    h(14, HandDrums, 0.80, Special, Medium),
    // High: Agogo Bell + 16th shaker energy
    h(0, Bell, 0.82, Accent, High),
    h(3, Bell, 0.72, Soft, High),
    h(6, Bell, 0.78, Accent, High),
    h(10, Bell, 0.70, Soft, High),
    h(12, Bell, 0.82, Accent, High),
    h(1, Shaker, 0.36, Standard, High),
    h(5, Shaker, 0.36, Standard, High),
    h(9, Shaker, 0.36, Standard, High),
    h(13, Shaker, 0.38, Standard, High),
    h(15, Snare, 0.55, Soft, High),
];

const PATTERN_HIGHLIFE_PRAISE: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::HighlifePraise,
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.04,
    is_compound: false,
    hits: HIGHLIFE_HITS,
};

// 5. Woro (12/8 Nigerian/West African rolling praise: 24 steps = 4 dotted-quarters x 6)
// Compound bell pattern: 0, 4, 8, 10, 14, 18, 22
static WORO_HITS: &[PatternHit] = &[
    // Driving compound shaker (every 8th: 0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22)
    h(0, Shaker, 0.80, Accent, Low),
    h(2, Shaker, 0.45, Standard, Low),
    h(4, Shaker, 0.60, Standard, Low),
    h(6, Shaker, 0.78, Accent, Low),
    h(8, Shaker, 0.45, Standard, Low),
    h(10, Shaker, 0.62, Standard, Low),
    h(12, Shaker, 0.80, Accent, Low),
    h(14, Shaker, 0.45, Standard, Low),
    h(16, Shaker, 0.60, Standard, Low),
    h(18, Shaker, 0.78, Accent, Low),
    h(20, Shaker, 0.45, Standard, Low),
    h(22, Shaker, 0.65, Standard, Low),
    // Standard African 12/8 bell/clave
    h(0, Clave, 0.84, Standard, Low),
    h(4, Clave, 0.78, Standard, Low),
    h(8, Clave, 0.80, Standard, Low),
    h(10, Clave, 0.82, Standard, Low),
    h(14, Clave, 0.78, Standard, Low),
    h(18, Clave, 0.80, Standard, Low),
    h(22, Clave, 0.82, Standard, Low),
    h(0, Kick, 0.90, Standard, Low),
    h(12, Kick, 0.86, Standard, Low),
    // Medium: Talking drum pitch glides, claps on 2 & 4 (steps 6, 18), tambourine
    h(6, Claps, 0.82, Standard, Medium),
    h(18, Claps, 0.85, Standard, Medium),
    h(6, Snare, 0.72, Standard, Medium),
    h(18, Snare, 0.75, Standard, Medium),
    h(4, HandDrums, 0.82, Special, Medium),
    h(10, HandDrums, 0.75, Standard, Medium),
    h(16, HandDrums, 0.85, Special, Medium),
    h(22, HandDrums, 0.78, Accent, Medium),
    h(0, Tambourine, 0.62, Standard, Medium),
    h(6, Tambourine, 0.75, Accent, Medium),
    h(12, Tambourine, 0.62, Standard, Medium),
    h(18, Tambourine, 0.75, Accent, Medium),
    // High: Agogo bell interlock + extra kick + lively 16th-triplet shaker
    h(0, Bell, 0.82, Accent, High),
    h(4, Bell, 0.72, Soft, High),
    h(8, Bell, 0.80, Accent, High),
    h(10, Bell, 0.75, Soft, High),
    h(14, Bell, 0.82, Accent, High),
    h(18, Bell, 0.72, Soft, High),
    h(22, Bell, 0.80, Accent, High),
    h(10, Kick, 0.72, Soft, High),
    h(22, Kick, 0.74, Soft, High),
    h(2, Tambourine, 0.45, Standard, High),
    h(14, Tambourine, 0.45, Standard, High),
];

const PATTERN_WORO: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::Woro,
    steps_per_bar: 24,
    bars_in_cycle: 1,
    swing: 0.0,
    is_compound: true,
    hits: WORO_HITS,
};

// 6. Makossa (4/4 Cameroonian/Central African driving praise groove)
static MAKOSSA_HITS: &[PatternHit] = &[
    // Driving 4-on-the-floor Makossa kick
    h(0, Kick, 0.94, Standard, Low),
    h(4, Kick, 0.90, Standard, Low),
    h(8, Kick, 0.94, Standard, Low),
    h(12, Kick, 0.90, Standard, Low),
    // Crisply syncopated 16th shaker
    h(0, Shaker, 0.65, Accent, Low),
    h(1, Shaker, 0.42, Standard, Low),
    h(2, Shaker, 0.70, Accent, Low),
    h(3, Shaker, 0.42, Standard, Low),
    h(4, Shaker, 0.65, Accent, Low),
    h(5, Shaker, 0.42, Standard, Low),
    h(6, Shaker, 0.72, Accent, Low),
    h(7, Shaker, 0.42, Standard, Low),
    h(8, Shaker, 0.65, Accent, Low),
    h(9, Shaker, 0.42, Standard, Low),
    h(10, Shaker, 0.70, Accent, Low),
    h(11, Shaker, 0.42, Standard, Low),
    h(12, Shaker, 0.65, Accent, Low),
    h(13, Shaker, 0.42, Standard, Low),
    h(14, Shaker, 0.72, Accent, Low),
    h(15, Shaker, 0.46, Standard, Low),
    // Clave syncopation
    h(0, Clave, 0.80, Standard, Low),
    h(3, Clave, 0.76, Standard, Low),
    h(6, Clave, 0.82, Standard, Low),
    h(10, Clave, 0.78, Standard, Low),
    h(14, Clave, 0.80, Standard, Low),
    // Medium: Snare/Claps + Congas + HiHat offbeat
    h(4, Snare, 0.80, Standard, Medium),
    h(12, Snare, 0.84, Standard, Medium),
    h(4, Claps, 0.78, Standard, Medium),
    h(12, Claps, 0.82, Standard, Medium),
    h(2, HiHat, 0.62, Accent, Medium),
    h(6, HiHat, 0.62, Accent, Medium),
    h(10, HiHat, 0.62, Accent, Medium),
    h(14, HiHat, 0.65, Accent, Medium),
    h(3, HandDrums, 0.76, Standard, Medium),
    h(11, HandDrums, 0.80, Accent, Medium),
    h(14, HandDrums, 0.74, Standard, Medium),
    // High: Bell & Tambourine fireworks
    h(0, Bell, 0.82, Accent, High),
    h(2, Bell, 0.68, Soft, High),
    h(6, Bell, 0.80, Accent, High),
    h(8, Bell, 0.72, Soft, High),
    h(11, Bell, 0.78, Accent, High),
    h(14, Bell, 0.82, Accent, High),
    h(2, Tambourine, 0.68, Accent, High),
    h(6, Tambourine, 0.68, Accent, High),
    h(10, Tambourine, 0.68, Accent, High),
    h(14, Tambourine, 0.72, Accent, High),
];

const PATTERN_MAKOSSA: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::Makossa,
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.03,
    is_compound: false,
    hits: MAKOSSA_HITS,
};

// 7. Amapiano (2-bar 32-step cycle: swung shakers, rim/claps, iconic Log Drum syncopation!)
static AMAPIANO_HITS: &[PatternHit] = &[
    // Iconic Amapiano bouncy shaker
    h(0, Shaker, 0.78, Accent, Low),
    h(2, Shaker, 0.56, Standard, Low),
    h(3, Shaker, 0.72, Accent, Low),
    h(4, Shaker, 0.45, Standard, Low),
    h(6, Shaker, 0.74, Accent, Low),
    h(8, Shaker, 0.78, Accent, Low),
    h(10, Shaker, 0.56, Standard, Low),
    h(11, Shaker, 0.72, Accent, Low),
    h(12, Shaker, 0.45, Standard, Low),
    h(14, Shaker, 0.74, Accent, Low),
    h(16, Shaker, 0.78, Accent, Low),
    h(18, Shaker, 0.56, Standard, Low),
    h(19, Shaker, 0.72, Accent, Low),
    h(20, Shaker, 0.45, Standard, Low),
    h(22, Shaker, 0.74, Accent, Low),
    h(24, Shaker, 0.78, Accent, Low),
    h(26, Shaker, 0.56, Standard, Low),
    h(27, Shaker, 0.72, Accent, Low),
    h(28, Shaker, 0.45, Standard, Low),
    h(30, Shaker, 0.76, Accent, Low),
    // Steady four-on-the-floor soft/deep kick
    h(0, Kick, 0.88, Standard, Low),
    h(4, Kick, 0.82, Standard, Low),
    h(8, Kick, 0.88, Standard, Low),
    h(12, Kick, 0.82, Standard, Low),
    h(16, Kick, 0.88, Standard, Low),
    h(20, Kick, 0.82, Standard, Low),
    h(24, Kick, 0.88, Standard, Low),
    h(28, Kick, 0.82, Standard, Low),
    // Syncopated rim/clave
    h(3, Clave, 0.72, Standard, Low),
    h(10, Clave, 0.75, Standard, Low),
    h(19, Clave, 0.72, Standard, Low),
    h(26, Clave, 0.75, Standard, Low),
    // Medium: Signature Amapiano Log Drum (HandDrums Special variant) + Claps
    h(0, HandDrums, 0.85, Special, Medium),
    h(6, HandDrums, 0.90, Special, Medium),
    h(10, HandDrums, 0.78, Special, Medium),
    h(14, HandDrums, 0.82, Special, Medium),
    h(19, HandDrums, 0.88, Special, Medium),
    h(22, HandDrums, 0.84, Special, Medium),
    h(28, HandDrums, 0.92, Special, Medium),
    h(30, HandDrums, 0.80, Special, Medium),
    h(4, Claps, 0.72, Standard, Medium),
    h(12, Claps, 0.75, Standard, Medium),
    h(20, Claps, 0.72, Standard, Medium),
    h(28, Claps, 0.75, Standard, Medium),
    h(4, Snare, 0.55, Soft, Medium),
    h(12, Snare, 0.58, Soft, Medium),
    h(20, Snare, 0.55, Soft, Medium),
    h(28, Snare, 0.58, Soft, Medium),
    // High: Tambourine + HiHat + extra log drum syncopation
    h(2, Tambourine, 0.62, Standard, High),
    h(10, Tambourine, 0.65, Standard, High),
    h(18, Tambourine, 0.62, Standard, High),
    h(26, Tambourine, 0.68, Accent, High),
    h(2, HiHat, 0.50, Accent, High),
    h(6, HiHat, 0.45, Standard, High),
    h(10, HiHat, 0.50, Accent, High),
    h(14, HiHat, 0.45, Standard, High),
    h(18, HiHat, 0.50, Accent, High),
    h(22, HiHat, 0.45, Standard, High),
    h(26, HiHat, 0.50, Accent, High),
    h(30, HiHat, 0.48, Standard, High),
    h(31, HandDrums, 0.76, Special, High),
];

const PATTERN_AMAPIANO: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::Amapiano,
    steps_per_bar: 16,
    bars_in_cycle: 2,
    swing: 0.14,
    is_compound: false,
    hits: AMAPIANO_HITS,
};

// 8. Funk Pocket (4/4 tight 16th-note syncopation)
static FUNK_POCKET_HITS: &[PatternHit] = &[
    h(0, Kick, 0.94, Standard, Low),
    h(3, Kick, 0.76, Standard, Low),
    h(6, Kick, 0.80, Standard, Low),
    h(10, Kick, 0.88, Standard, Low),
    h(0, HiHat, 0.75, Accent, Low),
    h(2, HiHat, 0.50, Standard, Low),
    h(4, HiHat, 0.72, Accent, Low),
    h(6, HiHat, 0.50, Standard, Low),
    h(8, HiHat, 0.75, Accent, Low),
    h(10, HiHat, 0.50, Standard, Low),
    h(12, HiHat, 0.72, Accent, Low),
    h(14, HiHat, 0.62, Accent, Low),
    h(4, Snare, 0.88, Standard, Low),
    h(12, Snare, 0.90, Standard, Low),
    // Medium: ghost notes, shaker, tambourine, claps
    h(7, Snare, 0.36, Soft, Medium),
    h(9, Snare, 0.34, Soft, Medium),
    h(15, Snare, 0.42, Soft, Medium),
    h(1, Shaker, 0.40, Standard, Medium),
    h(3, Shaker, 0.44, Standard, Medium),
    h(5, Shaker, 0.40, Standard, Medium),
    h(7, Shaker, 0.46, Standard, Medium),
    h(9, Shaker, 0.40, Standard, Medium),
    h(11, Shaker, 0.44, Standard, Medium),
    h(13, Shaker, 0.40, Standard, Medium),
    h(15, Shaker, 0.48, Standard, Medium),
    h(4, Claps, 0.75, Standard, Medium),
    h(12, Claps, 0.78, Standard, Medium),
    h(4, Tambourine, 0.65, Accent, Medium),
    h(12, Tambourine, 0.68, Accent, Medium),
    // High: Cowbell/Clave pocket + 16th hihat + slap hand drum
    h(0, Bell, 0.78, Standard, High),
    h(3, Bell, 0.65, Soft, High),
    h(6, Bell, 0.72, Standard, High),
    h(10, Bell, 0.74, Standard, High),
    h(14, Clave, 0.70, Standard, High),
    h(14, HandDrums, 0.76, Accent, High),
];

const PATTERN_FUNK_POCKET: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::FunkPocket,
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.08,
    is_compound: false,
    hits: FUNK_POCKET_HITS,
};

// 9. Gospel Praise / Sunday Shout (4/4 driving tambourine + claps + pocket)
static GOSPEL_PRAISE_HITS: &[PatternHit] = &[
    h(0, Kick, 0.92, Standard, Low),
    h(6, Kick, 0.78, Standard, Low),
    h(8, Kick, 0.90, Standard, Low),
    h(11, Kick, 0.72, Soft, Low),
    h(4, Snare, 0.85, Standard, Low),
    h(12, Snare, 0.88, Standard, Low),
    h(0, Tambourine, 0.65, Standard, Low),
    h(4, Tambourine, 0.85, Accent, Low),
    h(8, Tambourine, 0.65, Standard, Low),
    h(12, Tambourine, 0.88, Accent, Low),
    // Medium: full claps, 16th tambourine shakes, hihat, shaker
    h(4, Claps, 0.86, Standard, Medium),
    h(12, Claps, 0.90, Standard, Medium),
    h(2, Tambourine, 0.55, Standard, Medium),
    h(6, Tambourine, 0.58, Standard, Medium),
    h(10, Tambourine, 0.55, Standard, Medium),
    h(14, Tambourine, 0.62, Standard, Medium),
    h(0, HiHat, 0.60, Standard, Medium),
    h(2, HiHat, 0.45, Standard, Medium),
    h(4, HiHat, 0.60, Standard, Medium),
    h(6, HiHat, 0.45, Standard, Medium),
    h(8, HiHat, 0.60, Standard, Medium),
    h(10, HiHat, 0.45, Standard, Medium),
    h(12, HiHat, 0.60, Standard, Medium),
    h(14, HiHat, 0.52, Accent, Medium),
    h(0, Shaker, 0.55, Accent, Medium),
    h(4, Shaker, 0.55, Accent, Medium),
    h(8, Shaker, 0.55, Accent, Medium),
    h(12, Shaker, 0.55, Accent, Medium),
    // High: shout double claps, cowbell/clave drive, kick syncopation
    h(14, Kick, 0.80, Standard, High),
    h(15, Snare, 0.62, Soft, High),
    h(14, Claps, 0.68, Standard, High),
    h(0, Bell, 0.74, Standard, High),
    h(4, Bell, 0.70, Standard, High),
    h(8, Bell, 0.74, Standard, High),
    h(12, Bell, 0.70, Standard, High),
    h(6, Clave, 0.68, Standard, High),
    h(14, Clave, 0.72, Standard, High),
];

const PATTERN_GOSPEL_PRAISE: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::GospelPraise,
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.05,
    is_compound: false,
    hits: GOSPEL_PRAISE_HITS,
};

// 10. Acoustic Cajon (4/4 intimate acoustic set)
static ACOUSTIC_CAJON_HITS: &[PatternHit] = &[
    h(0, Kick, 0.80, Soft, Low),
    h(6, Kick, 0.62, Soft, Low),
    h(8, Kick, 0.78, Soft, Low),
    h(0, Shaker, 0.68, Accent, Low),
    h(2, Shaker, 0.42, Standard, Low),
    h(4, Shaker, 0.62, Accent, Low),
    h(6, Shaker, 0.42, Standard, Low),
    h(8, Shaker, 0.68, Accent, Low),
    h(10, Shaker, 0.42, Standard, Low),
    h(12, Shaker, 0.62, Accent, Low),
    h(14, Shaker, 0.45, Standard, Low),
    // Medium: Cajon slap (Snare Soft/Standard) + Tambourine
    h(4, Snare, 0.76, Soft, Medium),
    h(12, Snare, 0.80, Soft, Medium),
    h(10, Kick, 0.65, Soft, Medium),
    h(4, Tambourine, 0.65, Standard, Medium),
    h(12, Tambourine, 0.68, Standard, Medium),
    h(2, Clave, 0.50, Soft, Medium),
    h(10, Clave, 0.52, Soft, Medium),
    // High: Acoustic Claps & HandDrums
    h(4, Claps, 0.72, Standard, High),
    h(12, Claps, 0.76, Standard, High),
    h(14, HandDrums, 0.68, Standard, High),
    h(15, Snare, 0.45, Soft, High),
];

const PATTERN_ACOUSTIC_CAJON: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::AcousticCajon,
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.02,
    is_compound: false,
    hits: ACOUSTIC_CAJON_HITS,
};

// 11. Latin / Caribbean (2-bar 3-2 Son Clave + Conga Tumbao)
static LATIN_HITS: &[PatternHit] = &[
    // 3-2 Son Clave across 2 bars (0, 3, 6 | 10, 12 -> 16+10=26, 16+12=28; actually classic 3-2 is bar1: 0,3,6; bar2: 2,4 i.e. 18,20)
    h(0, Clave, 0.88, Standard, Low),
    h(3, Clave, 0.84, Standard, Low),
    h(6, Clave, 0.84, Standard, Low),
    h(18, Clave, 0.86, Standard, Low),
    h(20, Clave, 0.86, Standard, Low),
    // Shaker 8ths
    h(0, Shaker, 0.68, Accent, Low),
    h(2, Shaker, 0.46, Standard, Low),
    h(4, Shaker, 0.65, Accent, Low),
    h(6, Shaker, 0.46, Standard, Low),
    h(8, Shaker, 0.68, Accent, Low),
    h(10, Shaker, 0.46, Standard, Low),
    h(12, Shaker, 0.65, Accent, Low),
    h(14, Shaker, 0.48, Standard, Low),
    h(16, Shaker, 0.68, Accent, Low),
    h(18, Shaker, 0.46, Standard, Low),
    h(20, Shaker, 0.65, Accent, Low),
    h(22, Shaker, 0.46, Standard, Low),
    h(24, Shaker, 0.68, Accent, Low),
    h(26, Shaker, 0.46, Standard, Low),
    h(28, Shaker, 0.65, Accent, Low),
    h(30, Shaker, 0.48, Standard, Low),
    // Tumbao Kick (anticipated "and of 2" and 4: 6, 14)
    h(0, Kick, 0.82, Standard, Low),
    h(6, Kick, 0.80, Standard, Low),
    h(14, Kick, 0.78, Standard, Low),
    h(16, Kick, 0.82, Standard, Low),
    h(22, Kick, 0.80, Standard, Low),
    h(30, Kick, 0.78, Standard, Low),
    // Medium: Conga Tumbao + Timbale rim/Snare
    h(4, HandDrums, 0.82, Accent, Medium),
    h(6, HandDrums, 0.70, Standard, Medium),
    h(12, HandDrums, 0.78, Standard, Medium),
    h(14, HandDrums, 0.74, Standard, Medium),
    h(20, HandDrums, 0.82, Accent, Medium),
    h(22, HandDrums, 0.70, Standard, Medium),
    h(28, HandDrums, 0.78, Standard, Medium),
    h(30, HandDrums, 0.74, Standard, Medium),
    h(4, Snare, 0.60, Soft, Medium),
    h(12, Snare, 0.64, Soft, Medium),
    h(20, Snare, 0.60, Soft, Medium),
    h(28, Snare, 0.64, Soft, Medium),
    // High: Cowbell Cascara + Tambourine + Claps
    h(0, Bell, 0.80, Standard, High),
    h(4, Bell, 0.76, Standard, High),
    h(8, Bell, 0.80, Standard, High),
    h(12, Bell, 0.76, Standard, High),
    h(16, Bell, 0.80, Standard, High),
    h(20, Bell, 0.76, Standard, High),
    h(24, Bell, 0.80, Standard, High),
    h(28, Bell, 0.76, Standard, High),
    h(4, Tambourine, 0.68, Accent, High),
    h(12, Tambourine, 0.70, Accent, High),
    h(20, Tambourine, 0.68, Accent, High),
    h(28, Tambourine, 0.70, Accent, High),
];

const PATTERN_LATIN_CARIBBEAN: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::LatinCaribbean,
    steps_per_bar: 16,
    bars_in_cycle: 2,
    swing: 0.0,
    is_compound: false,
    hits: LATIN_HITS,
};

// 12. Reggae / Soca (4/4 one-drop & island groove)
static REGGAE_SOCA_HITS: &[PatternHit] = &[
    h(8, Kick, 0.92, Standard, Low), // Classic one-drop beat 3
    h(0, Kick, 0.72, Soft, Low),
    h(8, Snare, 0.85, Soft, Low),    // Cross-stick/rim on 3
    h(0, Shaker, 0.64, Accent, Low),
    h(2, Shaker, 0.45, Standard, Low),
    h(4, Shaker, 0.62, Accent, Low),
    h(6, Shaker, 0.45, Standard, Low),
    h(8, Shaker, 0.66, Accent, Low),
    h(10, Shaker, 0.45, Standard, Low),
    h(12, Shaker, 0.62, Accent, Low),
    h(14, Shaker, 0.48, Standard, Low),
    // Medium: offbeat skank clave/tambourine & hihat
    h(2, Clave, 0.75, Standard, Medium),
    h(6, Clave, 0.78, Standard, Medium),
    h(10, Clave, 0.75, Standard, Medium),
    h(14, Clave, 0.78, Standard, Medium),
    h(2, Tambourine, 0.65, Accent, Medium),
    h(6, Tambourine, 0.65, Accent, Medium),
    h(10, Tambourine, 0.65, Accent, Medium),
    h(14, Tambourine, 0.68, Accent, Medium),
    h(0, HiHat, 0.55, Standard, Medium),
    h(4, HiHat, 0.50, Standard, Medium),
    h(8, HiHat, 0.62, Accent, Medium),
    h(12, HiHat, 0.50, Standard, Medium),
    // High: Soca kick lift + Claps + Bell + HandDrums
    h(4, Kick, 0.80, Standard, High),
    h(12, Kick, 0.82, Standard, High),
    h(4, Claps, 0.76, Standard, High),
    h(12, Claps, 0.80, Standard, High),
    h(0, Bell, 0.76, Standard, High),
    h(3, Bell, 0.68, Soft, High),
    h(6, Bell, 0.74, Standard, High),
    h(10, Bell, 0.70, Soft, High),
    h(14, HandDrums, 0.76, Accent, High),
];

const PATTERN_REGGAE_SOCA: StylePatternSpec = StylePatternSpec {
    style: PercussionStyle::ReggaeSoca,
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.08,
    is_compound: false,
    hits: REGGAE_SOCA_HITS,
};
