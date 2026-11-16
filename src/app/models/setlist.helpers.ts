import {
  SERVICE_SECTION_PRESETS,
  type CustomPercussionPattern,
  type EngineSnapshot,
  type PercussionStyle,
  type ServiceSectionKind,
  type SetlistBatchPatch,
  type SetlistItemSnapshot,
  type SetlistManifest,
  type SetlistSection,
} from './engine.models';

export interface SetlistBucket {
  id: string;
  section: SetlistSection | null;
  items: SetlistItemSnapshot[];
}

export function buildSetlistBuckets(
  sections: SetlistSection[],
  items: SetlistItemSnapshot[],
): SetlistBucket[] {
  if (sections.length === 0) {
    return items.length > 0 ? [{ id: '__all__', section: null, items }] : [];
  }

  const sectionIds = new Set(sections.map((s) => s.id));
  const buckets: SetlistBucket[] = sections.map((section) => ({
    id: section.id,
    section,
    items: items.filter((item) => item.section_id === section.id),
  }));

  const unsectioned = items.filter(
    (item) => !item.section_id || !sectionIds.has(item.section_id),
  );
  if (unsectioned.length > 0) {
    buckets.push({
      id: '__unsectioned__',
      section: null,
      items: unsectioned,
    });
  }

  return buckets;
}

export function buildNewSection(
  kind: ServiceSectionKind,
  existing: SetlistSection[],
  customName?: string,
): SetlistSection {
  const preset =
    SERVICE_SECTION_PRESETS.find((p) => p.kind === kind) ?? SERVICE_SECTION_PRESETS[0];
  const baseName = customName?.trim() || preset.label;
  const slugBase = baseName
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '') || 'section';
  let id = `sec-${slugBase}`;
  let suffix = 2;
  const taken = new Set(existing.map((s) => s.id));
  while (taken.has(id)) {
    id = `sec-${slugBase}-${suffix++}`;
  }
  return {
    id,
    name: baseName,
    kind,
    color: preset.color,
    collapsed: false,
    note: null,
  };
}

export function toggleMultiSelectSlot(
  current: number[],
  anchor: number | null,
  slot: number,
  shiftKey: boolean,
  orderedSlots: number[],
): { nextSelected: number[]; nextAnchor: number } {
  const selected = new Set(current);
  if (shiftKey && anchor !== null) {
    const startIdx = orderedSlots.indexOf(anchor);
    const endIdx = orderedSlots.indexOf(slot);
    if (startIdx >= 0 && endIdx >= 0) {
      const [lo, hi] = startIdx <= endIdx ? [startIdx, endIdx] : [endIdx, startIdx];
      for (let i = lo; i <= hi; i++) {
        selected.add(orderedSlots[i]);
      }
      return {
        nextSelected: Array.from(selected).sort((a, b) => a - b),
        nextAnchor: slot,
      };
    }
  }

  if (selected.has(slot)) {
    selected.delete(slot);
  } else {
    selected.add(slot);
  }
  return {
    nextSelected: Array.from(selected).sort((a, b) => a - b),
    nextAnchor: slot,
  };
}

export function computeCrossBucketMove(
  fromSlot: number,
  targetBucket: SetlistBucket,
  targetLocalIndex: number,
  allItems: SetlistItemSnapshot[],
): { toSlot: number; sectionId: string | null } {
  const targetSectionId = targetBucket.section?.id ?? null;
  const otherItems = targetBucket.items.filter((it) => it.slot !== fromSlot);
  if (otherItems.length === 0) {
    // Keep roughly at end if unsectioned, or right after previous section's last item
    return { toSlot: fromSlot, sectionId: targetSectionId };
  }
  const clampedIdx = Math.max(0, Math.min(otherItems.length, targetLocalIndex));
  let desiredGlobalSlot: number;
  if (clampedIdx < otherItems.length) {
    const anchorSlot = otherItems[clampedIdx].slot;
    desiredGlobalSlot = fromSlot < anchorSlot ? anchorSlot - 1 : anchorSlot;
  } else {
    const lastAnchor = otherItems[otherItems.length - 1].slot;
    desiredGlobalSlot = fromSlot < lastAnchor ? lastAnchor : lastAnchor + 1;
  }
  const maxSlot = Math.max(0, allItems.length - 1);
  return {
    toSlot: Math.max(0, Math.min(maxSlot, desiredGlobalSlot)),
    sectionId: targetSectionId,
  };
}

export function resolveBatchTargetSlots(
  selectedSlots: number[],
  editedSlot: number,
  itemCount: number,
): number[] | null {
  if (selectedSlots.length <= 1 || !selectedSlots.includes(editedSlot)) {
    return null;
  }
  const valid = selectedSlots.filter((s) => s >= 0 && s < itemCount);
  return valid.length > 1 ? valid : null;
}

export function parseBatchBpmInput(
  raw: string | number | null | undefined,
): { kind: 'bpm'; value: number } | { kind: 'reset' } | null {
  const s = String(raw ?? '').trim();
  if (!s) return null;
  if (s.toLowerCase() === 'reset') return { kind: 'reset' };
  const val = Number(s);
  if (!Number.isFinite(val) || val < 30 || val > 260) return null;
  return { kind: 'bpm', value: Math.round(val * 10) / 10 };
}

export function parseBatchCrossfadeInput(raw: string | number | null | undefined): number | null {
  const s = String(raw ?? '').trim();
  if (!s) return null;
  const val = Number(s);
  if (!Number.isFinite(val) || val < 0 || val > 10) return null;
  return val;
}

export function serializeSetlistManifest(cur: EngineSnapshot): SetlistManifest {
  return {
    id: cur.setlist_name.toLowerCase().replace(/[^a-z0-9]+/g, '-') || 'sunday-set',
    name: cur.setlist_name,
    sections: cur.setlist_sections.map((s) => ({ ...s })),
    mixer: {
      master_gain: cur.master_gain,
      backing_gain: cur.backing_gain,
      click_gain: cur.click_gain,
      click_muted: cur.click_muted && cur.click_mode !== 'Off' ? true : false,
    },
    items: cur.setlist_items.map((item) => {
      const liveStems =
        item.slot === cur.active_setlist_slot && cur.stems.length > 0
          ? cur.stems.map((s) => ({ name: s.name, gain: s.gain, muted: s.muted }))
          : (item.stem_mix ?? []).map((s) => ({ ...s }));
      return {
        song_id: item.song_id,
        song_dir: item.song_dir,
        title: item.title,
        original_key: item.original_key,
        base_bpm: item.base_bpm,
        key_shift_semitones: item.key_shift_semitones,
        bpm_override: item.bpm_override,
        transition_mode: item.transition_mode,
        crossfade_sec: item.crossfade_sec,
        ambient_pad_enabled: item.ambient_pad_enabled,
        pad_key_override: item.pad_key_override,
        pad_lock: item.pad_lock ?? null,
        click_mode: item.click_mode,
        section_id: item.section_id ?? null,
        slot_kind: item.slot_kind ?? 'SongBundle',
        time_signature: item.time_signature ?? null,
        percussion_lock: item.percussion_lock ?? null,
        ...(liveStems.length > 0 ? { stem_mix: liveStems } : {}),
      };
    }),
  };
}

/**
 * Resolves a Style / Custom Loop dropdown selection (`custom:<id>`, `style:<Style>`, or bare
 * `PercussionStyle`) into fields for `BatchUpdateSetlistSlots`.
 */
export function resolveBatchGroovePatch(
  selected: string,
  savedLoops: CustomPercussionPattern[],
  activeCustom?: CustomPercussionPattern | null,
): Omit<SetlistBatchPatch, 'slots'> | null {
  if (!selected) return null;
  if (selected.startsWith('custom:')) {
    const id = selected.slice('custom:'.length);
    const found =
      savedLoops.find((l) => l.id === id) ??
      (activeCustom && activeCustom.id === id ? activeCustom : null);
    if (!found) return null;
    return {
      percussion_custom_pattern: {
        ...found,
        hits: found.hits.map((h) => ({ ...h })),
      },
    };
  }
  const styleVal = (
    selected.startsWith('style:') ? selected.slice('style:'.length) : selected
  ) as PercussionStyle;
  if (!styleVal) return null;
  return {
    percussion_style: styleVal,
    percussion_custom_pattern: null,
  };
}
