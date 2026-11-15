import type {
  CustomPatternHit,
  CustomPercussionPattern,
  GrooveIntensity,
  HitVariant,
  PercussionInstrument,
} from './engine.models';

export type BarCycleLength = 1 | 2 | 4;

export type BarQuickPreset =
  | 'four-on-floor'
  | '16th-shaker'
  | 'backbeat-claps'
  | 'offbeat-tambourine';

/** Sanitizes a human-readable loop title into a stable filesystem slug ID. */
export function sanitizeLoopId(name: string): string {
  return (
    name
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, '-')
      .replace(/^-|-$/g, '') || 'custom-loop'
  );
}

/** Clones a CustomPercussionPattern deeply so edits never mutate shared state. */
export function cloneCustomPattern(pattern: CustomPercussionPattern): CustomPercussionPattern {
  return {
    ...pattern,
    hits: pattern.hits.map((h) => ({ ...h })),
  };
}

/** Normalizes, sorts, and deduplicates hits within a pattern's bar/step bounds. */
export function normalizePattern(pattern: CustomPercussionPattern): CustomPercussionPattern {
  const spb = [12, 16, 24].includes(pattern.steps_per_bar) ? pattern.steps_per_bar : 16;
  const bars = Math.max(1, Math.min(4, Math.round(pattern.bars_in_cycle || 1)));
  const maxStep = spb * bars;
  const seen = new Set<string>();
  const hits: CustomPatternHit[] = [];

  for (const h of pattern.hits) {
    if (h.step < 0 || h.step >= maxStep) continue;
    const key = `${h.step}:${h.instrument}`;
    if (seen.has(key)) continue;
    seen.add(key);
    hits.push({
      ...h,
      velocity: Math.max(0.05, Math.min(1.0, h.velocity)),
    });
  }

  hits.sort((a, b) =>
    a.step !== b.step ? a.step - b.step : a.instrument.localeCompare(b.instrument),
  );

  return {
    ...pattern,
    steps_per_bar: spb,
    bars_in_cycle: bars,
    swing: Math.max(0, Math.min(0.35, pattern.swing)),
    is_compound: spb === 12 || spb === 24,
    hits,
  };
}

/** Rescales existing bar hits proportionally when changing grid subdivision (`12` / `16` / `24`). */
export function rescalePatternGrid(
  pattern: CustomPercussionPattern,
  newStepsPerBar: number,
): CustomPercussionPattern {
  const spb = newStepsPerBar === 12 || newStepsPerBar === 24 ? newStepsPerBar : 16;
  const oldSpb = [12, 16, 24].includes(pattern.steps_per_bar) ? pattern.steps_per_bar : 16;
  if (spb === oldSpb) {
    return normalizePattern({
      ...pattern,
      steps_per_bar: spb,
      is_compound: spb === 12 || spb === 24,
    });
  }
  const scaledHits: CustomPatternHit[] = pattern.hits.map((h) => {
    const bar = Math.floor(h.step / oldSpb);
    const local = h.step % oldSpb;
    const scaledLocal = Math.min(spb - 1, Math.round((local / oldSpb) * spb));
    return {
      ...h,
      step: bar * spb + scaledLocal,
    };
  });
  return normalizePattern({
    ...pattern,
    steps_per_bar: spb,
    is_compound: spb === 12 || spb === 24,
    hits: scaledHits,
  });
}

/**
 * Expands or trims a pattern to 1, 2, or 4 bars.
 * When expanding, earlier bar grooves repeat into newly added bars so every bar starts full.
 */
export function expandPatternBars(
  pattern: CustomPercussionPattern,
  targetBars: number,
): CustomPercussionPattern {
  const clamped = Math.max(1, Math.min(4, Math.round(targetBars || 1)));
  const normalized = normalizePattern(pattern);
  const spb = normalized.steps_per_bar;
  const oldTotal = spb * normalized.bars_in_cycle;
  const newTotal = spb * clamped;

  if (clamped <= normalized.bars_in_cycle) {
    return {
      ...normalized,
      bars_in_cycle: clamped,
      hits: normalized.hits.filter((h) => h.step < newTotal),
    };
  }

  const hits: CustomPatternHit[] = normalized.hits.map((h) => ({ ...h }));
  for (let bar = normalized.bars_in_cycle; bar < clamped; bar++) {
    const destOffset = bar * spb;
    const srcOffset = (bar % normalized.bars_in_cycle) * spb;
    for (const h of normalized.hits) {
      if (h.step >= srcOffset && h.step < srcOffset + spb && srcOffset < oldTotal) {
        hits.push({
          ...h,
          step: destOffset + (h.step - srcOffset),
        });
      }
    }
  }

  return normalizePattern({
    ...normalized,
    bars_in_cycle: clamped,
    hits,
  });
}

/** Copies all hits from `fromBar` into `toBar` (0-indexed), replacing existing hits in `toBar`. */
export function copyBarHits(
  pattern: CustomPercussionPattern,
  fromBar: number,
  toBar: number,
): CustomPercussionPattern {
  const normalized = normalizePattern(pattern);
  const spb = normalized.steps_per_bar;
  if (
    fromBar < 0 ||
    toBar < 0 ||
    fromBar >= normalized.bars_in_cycle ||
    toBar >= normalized.bars_in_cycle ||
    fromBar === toBar
  ) {
    return normalized;
  }

  const srcStart = fromBar * spb;
  const srcEnd = srcStart + spb;
  const dstStart = toBar * spb;
  const dstEnd = dstStart + spb;

  const kept = normalized.hits.filter((h) => h.step < dstStart || h.step >= dstEnd);
  const copied = normalized.hits
    .filter((h) => h.step >= srcStart && h.step < srcEnd)
    .map((h) => ({ ...h, step: dstStart + (h.step - srcStart) }));

  return normalizePattern({
    ...normalized,
    hits: [...kept, ...copied],
  });
}

/** Replicates Bar 1 (index 0) across every subsequent bar in the cycle. */
export function copyBarOneToAllBars(pattern: CustomPercussionPattern): CustomPercussionPattern {
  let next = normalizePattern(pattern);
  for (let bar = 1; bar < next.bars_in_cycle; bar++) {
    next = copyBarHits(next, 0, bar);
  }
  return next;
}

/** Clears all hits within `barIndex` (0-indexed). */
export function clearBarHits(
  pattern: CustomPercussionPattern,
  barIndex: number,
): CustomPercussionPattern {
  const normalized = normalizePattern(pattern);
  const spb = normalized.steps_per_bar;
  const start = barIndex * spb;
  const end = start + spb;
  return {
    ...normalized,
    hits: normalized.hits.filter((h) => h.step < start || h.step >= end),
  };
}

/** Adds an expressive turnaround drum/percussion fill on the final beat of `barIndex`. */
export function applyTurnaroundFillToBar(
  pattern: CustomPercussionPattern,
  barIndex: number,
): CustomPercussionPattern {
  const normalized = normalizePattern(pattern);
  if (barIndex < 0 || barIndex >= normalized.bars_in_cycle) return normalized;

  const spb = normalized.steps_per_bar;
  const barStart = barIndex * spb;
  const fillLength = spb === 12 ? 3 : spb === 24 ? 6 : 4;
  const fillStart = barStart + spb - fillLength;
  const fillEnd = barStart + spb;

  // Preserve kick foundation while replacing snare/hand-drums/claps/tambourine in the fill window
  const fillInstruments = new Set<PercussionInstrument>([
    'Snare',
    'HandDrums',
    'Claps',
    'Tambourine',
  ]);
  const kept = normalized.hits.filter(
    (h) => !(h.step >= fillStart && h.step < fillEnd && fillInstruments.has(h.instrument)),
  );

  const mkHit = (
    localOffset: number,
    instrument: PercussionInstrument,
    variant: HitVariant,
    velocity: number,
    min_intensity: GrooveIntensity = 'Low',
  ): CustomPatternHit => ({
    step: fillStart + localOffset,
    instrument,
    variant,
    velocity,
    min_intensity,
  });

  const fillHits: CustomPatternHit[] =
    fillLength === 3
      ? [
          mkHit(0, 'HandDrums', 'Accent', 0.9, 'Low'),
          mkHit(1, 'Snare', 'Soft', 0.72, 'Low'),
          mkHit(2, 'Snare', 'Accent', 0.96, 'Low'),
          mkHit(2, 'Tambourine', 'Accent', 0.88, 'Medium'),
          mkHit(2, 'Claps', 'Accent', 0.85, 'High'),
        ]
      : fillLength === 6
        ? [
            mkHit(0, 'Snare', 'Standard', 0.8, 'Low'),
            mkHit(1, 'HandDrums', 'Standard', 0.78, 'Low'),
            mkHit(2, 'HandDrums', 'Special', 0.86, 'Low'),
            mkHit(3, 'Snare', 'Soft', 0.74, 'Medium'),
            mkHit(4, 'Snare', 'Accent', 0.94, 'Low'),
            mkHit(5, 'Claps', 'Accent', 0.9, 'Low'),
            mkHit(5, 'Tambourine', 'Accent', 0.92, 'Medium'),
          ]
        : [
            mkHit(0, 'Snare', 'Standard', 0.82, 'Low'),
            mkHit(1, 'HandDrums', 'Standard', 0.78, 'Low'),
            mkHit(2, 'HandDrums', 'Special', 0.9, 'Low'),
            mkHit(3, 'Snare', 'Accent', 0.96, 'Low'),
            mkHit(3, 'Claps', 'Accent', 0.88, 'Medium'),
            mkHit(3, 'Tambourine', 'Accent', 0.92, 'Medium'),
          ];

  return normalizePattern({
    ...normalized,
    hits: [...kept, ...fillHits],
  });
}

/** Applies a single-click groove foundation or top-end layer to `barIndex`. */
export function applyBarGrooveQuickFill(
  pattern: CustomPercussionPattern,
  barIndex: number,
  preset: BarQuickPreset,
): CustomPercussionPattern {
  const normalized = normalizePattern(pattern);
  if (barIndex < 0 || barIndex >= normalized.bars_in_cycle) return normalized;

  const spb = normalized.steps_per_bar;
  const barStart = barIndex * spb;
  const barEnd = barStart + spb;

  let targetInstrument: PercussionInstrument;
  const added: CustomPatternHit[] = [];

  if (preset === 'four-on-floor') {
    targetInstrument = 'Kick';
    const interval = spb === 12 ? 3 : spb === 24 ? 6 : 4;
    for (let s = 0; s < spb; s += interval) {
      added.push({
        step: barStart + s,
        instrument: 'Kick',
        velocity: s === 0 ? 0.96 : 0.88,
        variant: s === 0 ? 'Accent' : 'Standard',
        min_intensity: 'Low',
      });
    }
  } else if (preset === '16th-shaker') {
    targetInstrument = 'Shaker';
    const stepInc = spb === 24 ? 2 : 1;
    for (let s = 0; s < spb; s += stepInc) {
      const onBeat = s % (spb / 4) === 0;
      added.push({
        step: barStart + s,
        instrument: 'Shaker',
        velocity: onBeat ? 0.84 : s % 2 === 0 ? 0.7 : 0.56,
        variant: onBeat ? 'Accent' : 'Soft',
        min_intensity: s % 2 === 0 ? 'Low' : 'Medium',
      });
    }
  } else if (preset === 'backbeat-claps') {
    targetInstrument = 'Claps';
    const steps = spb === 12 ? [6] : spb === 24 ? [12] : [4, 12];
    for (const s of steps) {
      added.push({
        step: barStart + s,
        instrument: 'Claps',
        velocity: 0.9,
        variant: 'Accent',
        min_intensity: 'Low',
      });
    }
  } else {
    targetInstrument = 'Tambourine';
    const steps =
      spb === 12 ? [2, 5, 8, 11] : spb === 24 ? [3, 9, 15, 21] : [2, 6, 10, 14];
    for (const s of steps) {
      added.push({
        step: barStart + s,
        instrument: 'Tambourine',
        velocity: 0.82,
        variant: 'Standard',
        min_intensity: 'Low',
      });
    }
  }

  const kept = normalized.hits.filter(
    (h) => !(h.step >= barStart && h.step < barEnd && h.instrument === targetInstrument),
  );
  return normalizePattern({
    ...normalized,
    hits: [...kept, ...added],
  });
}
