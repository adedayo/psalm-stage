import { CdkDrag, CdkDragDrop, CdkDragHandle, CdkDropList } from '@angular/cdk/drag-drop';
import { DecimalPipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Output,
  computed,
  inject,
  signal,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { HlmBadgeImports } from '@spartan-ng/helm/badge';
import { HlmButtonImports } from '@spartan-ng/helm/button';
import { HlmCardImports } from '@spartan-ng/helm/card';
import {
  defaultPercussionChannels,
  PERCUSSION_STYLE_OPTIONS,
  SERVICE_SECTION_PRESETS,
  type ClickMode,
  type GrooveIntensity,
  type LibraryEntryStatus,
  type LibrarySongEntry,
  type PercussionLockConfig,
  type PercussionMode,
  type PercussionStyle,
  type ServiceSectionKind,
  type SetlistBatchPatch,
  type SetlistItemSnapshot,
  type SetlistSection,
  type SetlistSlotKind,
  type SetlistSummary,
  type TransitionMode,
} from '../../models/engine.models';
import {
  buildNewSection,
  parseBatchBpmInput,
  parseBatchCrossfadeInput,
  resolveBatchGroovePatch,
  resolveBatchTargetSlots,
  serializeSetlistManifest,
  toggleMultiSelectSlot,
} from '../../models/setlist.helpers';
import {
  CHROMATIC_KEYS,
  SONG_KEY_OPTIONS,
  PsalmStageService,
  computeKeySemitoneShift,
  transposeKeyName,
} from '../../services/psalm-stage.service';

export interface HarmonicTransitionInfo {
  label: string;
  tone: 'smooth' | 'lift' | 'contrast';
  padBridge: string;
}

export function computeHarmonicTransition(
  current: SetlistItemSnapshot,
  next: SetlistItemSnapshot,
): HarmonicTransitionInfo {
  const alias: Record<string, string> = {
    Db: 'C#',
    'D#': 'Eb',
    Gb: 'F#',
    'G#': 'Ab',
    'A#': 'Bb',
  };
  const stripMinor = (k: string) => k.replace(/m$/, '').trim();
  const keys = CHROMATIC_KEYS as readonly string[];
  const idxA = keys.indexOf(alias[stripMinor(current.effective_key)] ?? stripMinor(current.effective_key));
  const idxB = keys.indexOf(alias[stripMinor(next.effective_key)] ?? stripMinor(next.effective_key));

  let label = `${current.effective_key} → ${next.effective_key}`;
  let tone: 'smooth' | 'lift' | 'contrast' = 'contrast';

  if (current.effective_key === 'Unknown' || next.effective_key === 'Unknown') {
    label = `${current.effective_key} → ${next.effective_key} (Key unverified)`;
    tone = 'contrast';
  } else if (idxA >= 0 && idxB >= 0) {
    const diff = (((idxB - idxA) % 12) + 12) % 12;
    const intervalNames: Record<number, { name: string; tone: 'smooth' | 'lift' | 'contrast' }> = {
      0: { name: 'Same Key', tone: 'smooth' },
      1: { name: '+1 Half-Step Lift', tone: 'lift' },
      2: { name: '+2 Whole-Step Lift', tone: 'lift' },
      5: { name: '4th / Subdominant', tone: 'smooth' },
      7: { name: '5th / Dominant', tone: 'smooth' },
      10: { name: '-2 Whole-Step Down', tone: 'smooth' },
    };
    const match = intervalNames[diff];
    if (match) {
      label = `${current.effective_key} → ${next.effective_key} (${match.name})`;
      tone = match.tone;
    } else {
      label = `${current.effective_key} → ${next.effective_key} (+${diff} st)`;
      tone = 'contrast';
    }
  }

  const padKey = next.pad_key_override ?? next.effective_key;
  const padBridge = next.ambient_pad_enabled
    ? padKey === 'Unknown'
      ? 'Pad bridge needs manual key'
      : `Pad bridge in ${padKey}`
    : 'No pad bridge';

  return { label, tone, padBridge };
}

@Component({
  selector: 'app-setlist-studio',
  standalone: true,
  imports: [
    DecimalPipe,
    FormsModule,
    CdkDropList,
    CdkDrag,
    CdkDragHandle,
    HlmButtonImports,
    HlmBadgeImports,
    HlmCardImports,
  ],
  templateUrl: './setlist-studio.component.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class SetlistStudioComponent {
  protected readonly stage = inject(PsalmStageService);
  protected readonly snap = this.stage.snapshot;
  protected readonly chromaticKeys = CHROMATIC_KEYS;
  protected readonly songKeyOptions = SONG_KEY_OPTIONS;

  @Output() readonly openInWorkbench = new EventEmitter<string>();
  @Output() readonly goLive = new EventEmitter<void>();
  @Output() readonly newInWorkbench = new EventEmitter<void>();

  readonly searchQuery = signal('');
  readonly statusFilter = signal<'All' | LibraryEntryStatus>('All');
  readonly keyFilter = signal<string>('All');
  readonly sortBy = signal<'title' | 'bpm' | 'key'>('title');
  readonly linkFolderInput = signal('');
  readonly showLibrarySettings = signal(false);
  readonly showSavedSetlists = signal(false);
  readonly showAiSetup = signal(false);
  readonly copiedAiLabel = signal<string | null>(null);
  readonly selectedStudioSlot = signal<number | null>(null);
  readonly sectionPresetOptions = SERVICE_SECTION_PRESETS;
  readonly percussionStyleOptions = PERCUSSION_STYLE_OPTIONS;
  readonly newSectionKind = signal<ServiceSectionKind>('Praise');
  readonly newSectionName = signal<string>('');
  readonly addStandaloneOpen = signal<boolean>(false);
  readonly newStandaloneKind = signal<SetlistSlotKind>('PadOnly');
  readonly newStandaloneTitle = signal<string>('Sermon Underscore');
  readonly newStandaloneKey = signal<string>('D');
  readonly newStandaloneBpm = signal<number>(72);
  readonly newStandaloneTimeSig = signal<string>('4/4');
  readonly newStandaloneSectionId = signal<string>('');
  readonly selectedStudioSlots = signal<number[]>([]);
  readonly selectionAnchorSlot = signal<number | null>(null);
  readonly batchTargetKey = signal<string>('');
  readonly batchBpm = signal<string>('');
  readonly batchClickMode = signal<string>('');
  readonly batchPadEnabled = signal<string>('');
  readonly batchCrossfadeSec = signal<string>('');
  readonly batchSectionId = signal<string>('');
  readonly batchPercussionMode = signal<string>('');
  readonly batchPercussionStyle = signal<string>('');

  readonly filteredLibrary = computed(() => {
    const q = this.searchQuery().trim().toLowerCase();
    const st = this.statusFilter();
    const key = this.keyFilter();
    const sort = this.sortBy();

    const filtered = this.stage.libraryEntries().filter((entry) => {
      if (st !== 'All' && entry.status !== st) return false;
      if (key !== 'All' && entry.original_key !== key) return false;
      if (!q) return true;
      return (
        entry.title.toLowerCase().includes(q) ||
        entry.artist.toLowerCase().includes(q) ||
        entry.original_key.toLowerCase().includes(q) ||
        entry.stem_names.some((s) => s.toLowerCase().includes(q))
      );
    });

    return [...filtered].sort((a, b) => {
      if (sort === 'bpm') return a.bpm - b.bpm;
      if (sort === 'key') return a.original_key.localeCompare(b.original_key);
      return a.title.localeCompare(b.title);
    });
  });

  readonly totalSetlistDurationSec = computed(() => {
    const entries = this.stage.libraryEntries();
    return this.snap().setlist_items.reduce((sum, item) => {
      const libMatch = entries.find((e) => e.dir === item.song_dir || e.song_id === item.song_id);
      const baseDur = libMatch?.estimated_duration_sec ?? 240;
      const ratio = item.effective_bpm > 0 ? item.base_bpm / item.effective_bpm : 1;
      return sum + baseDur * ratio;
    }, 0);
  });

  readonly selectedSlotDetail = computed(() => {
    const slot = this.selectedStudioSlot();
    const items = this.snap().setlist_items;
    if (slot === null || slot < 0 || slot >= items.length) {
      return items[0] ?? null;
    }
    return items[slot];
  });

  readonly selectedSlotLibraryEntry = computed(() => {
    const detail = this.selectedSlotDetail();
    if (!detail) return null;
    return (
      this.stage
        .libraryEntries()
        .find((e) => e.dir === detail.song_dir || e.song_id === detail.song_id) ?? null
    );
  });

  formatDuration(seconds: number): string {
    if (!Number.isFinite(seconds) || seconds <= 0) return '0:00';
    const total = Math.round(seconds);
    const mins = Math.floor(total / 60);
    const secs = total % 60;
    return `${mins}:${secs.toString().padStart(2, '0')}`;
  }

  transitionBetween(index: number): HarmonicTransitionInfo | null {
    const items = this.snap().setlist_items;
    if (index < 0 || index >= items.length - 1) return null;
    return computeHarmonicTransition(items[index], items[index + 1]);
  }

  async addSongToSetlist(entry: LibrarySongEntry, insertAt?: number): Promise<void> {
    await this.stage.sendCommand({
      type: 'AddSetlistSong',
      payload: { dir: entry.dir, insert_at: insertAt ?? null },
    });
    const newLen = this.snap().setlist_items.length;
    this.selectedStudioSlot.set(insertAt ?? Math.max(0, newLen - 1));
  }

  async duplicateSlot(item: SetlistItemSnapshot): Promise<void> {
    if (!item.song_dir) return;
    await this.stage.sendCommand({
      type: 'AddSetlistSong',
      payload: { dir: item.song_dir, insert_at: item.slot + 1 },
    });
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot + 1,
        key_shift_semitones: item.key_shift_semitones,
        bpm_override: item.bpm_override,
        transition_mode: item.transition_mode,
        crossfade_sec: item.crossfade_sec,
        ambient_pad_enabled: item.ambient_pad_enabled,
        pad_key_override: item.pad_key_override,
        pad_lock: item.pad_lock ?? null,
        click_mode: item.click_mode,
      },
    });
    this.selectedStudioSlot.set(item.slot + 1);
  }

  async onDropOnSetlist(event: CdkDragDrop<SetlistItemSnapshot[]>): Promise<void> {
    if (event.previousContainer === event.container) {
      if (event.previousIndex !== event.currentIndex) {
        await this.stage.sendCommand({
          type: 'MoveSetlistSlot',
          payload: { from: event.previousIndex, to: event.currentIndex },
        });
        this.selectedStudioSlot.set(event.currentIndex);
      }
      return;
    }

    const draggedEntry = event.item.data as LibrarySongEntry | undefined;
    if (draggedEntry) {
      await this.addSongToSetlist(draggedEntry, event.currentIndex);
    }
  }

  async adjustSlotKey(item: SetlistItemSnapshot, delta: number): Promise<void> {
    const nextShift = Math.max(-12, Math.min(12, item.key_shift_semitones + delta));
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        key_shift_semitones: nextShift,
      },
    });
  }

  async setSlotTargetKey(item: SetlistItemSnapshot, targetKey: string): Promise<void> {
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, target_key: targetKey },
      });
      return;
    }
    const diff = computeKeySemitoneShift(item.original_key, targetKey);
    if (diff === null) return;
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        key_shift_semitones: diff,
      },
    });
  }

  async setSlotCrossfadeSec(item: SetlistItemSnapshot, secRaw: number | string): Promise<void> {
    const val = Number(secRaw);
    if (!Number.isFinite(val)) return;
    const clamped = Math.max(0, Math.min(10, val));
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, crossfade_sec: clamped },
      });
      return;
    }
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        crossfade_sec: clamped,
      },
    });
  }

  async setSlotSection(item: SetlistItemSnapshot, sectionId: string): Promise<void> {
    const nextSection = sectionId || null;
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, section_id: nextSection },
      });
      return;
    }
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        section_id: nextSection,
      },
    });
  }

  async setSlotPercussionMode(item: SetlistItemSnapshot, mode: PercussionMode): Promise<void> {
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, percussion_mode: mode },
      });
      return;
    }
    const s = this.snap();
    const nextLock: PercussionLockConfig = item.percussion_lock
      ? { ...item.percussion_lock, mode }
      : {
          mode,
          style: s.percussion_style,
          intensity: s.percussion_intensity,
          gain: s.percussion_gain,
          iem_send: s.percussion_iem_send,
          follow_sections: s.percussion_follow_sections,
          room_mix: s.percussion_room_mix,
          kit_name: s.percussion_kit_name,
          kit_dir: s.percussion_kit_dir,
          custom_pattern: s.percussion_custom_pattern ?? null,
          channels: s.percussion_channels.length
            ? s.percussion_channels.map((c) => ({ ...c }))
            : defaultPercussionChannels(),
        };
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        percussion_lock: mode === 'Off' && !item.percussion_lock ? null : nextLock,
      },
    });
  }

  slotPercussionGrooveValue(item: SetlistItemSnapshot): string {
    if (item.percussion_lock?.custom_pattern) {
      return `custom:${item.percussion_lock.custom_pattern.id}`;
    }
    return `style:${item.percussion_lock?.style ?? this.snap().percussion_style}`;
  }

  async setSlotPercussionGroove(item: SetlistItemSnapshot, selected: string): Promise<void> {
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      const groovePatch = resolveBatchGroovePatch(
        selected,
        this.stage.percussionLoops(),
        this.snap().percussion_custom_pattern ?? item.percussion_lock?.custom_pattern ?? null,
      );
      if (groovePatch) {
        await this.stage.sendCommand({
          type: 'BatchUpdateSetlistSlots',
          payload: { slots: batchSlots, ...groovePatch },
        });
      }
      return;
    }
    const s = this.snap();
    const baseLock: PercussionLockConfig = item.percussion_lock
      ? {
          ...item.percussion_lock,
          channels: item.percussion_lock.channels.map((c) => ({ ...c })),
        }
      : {
          mode: 'PercussionOnly',
          style: s.percussion_style,
          intensity: s.percussion_intensity,
          gain: s.percussion_gain,
          iem_send: s.percussion_iem_send,
          follow_sections: s.percussion_follow_sections,
          room_mix: s.percussion_room_mix,
          kit_name: s.percussion_kit_name,
          kit_dir: s.percussion_kit_dir,
          custom_pattern: null,
          channels: s.percussion_channels.length
            ? s.percussion_channels.map((c) => ({ ...c }))
            : defaultPercussionChannels(),
        };

    if (selected.startsWith('custom:')) {
      const id = selected.slice('custom:'.length);
      const found =
        this.stage.percussionLoops().find((l) => l.id === id) ??
        item.percussion_lock?.custom_pattern;
      if (!found) return;
      const nextLock: PercussionLockConfig = {
        ...baseLock,
        mode: baseLock.mode === 'Off' ? 'PercussionOnly' : baseLock.mode,
        custom_pattern: { ...found, hits: found.hits.map((h) => ({ ...h })) },
      };
      await this.stage.sendCommand({
        type: 'UpdateSetlistSlotConfig',
        payload: { slot: item.slot, percussion_lock: nextLock },
      });
      return;
    }

    if (selected.startsWith('style:')) {
      const style = selected.slice('style:'.length) as PercussionStyle;
      const nextLock: PercussionLockConfig = {
        ...baseLock,
        mode: baseLock.mode === 'Off' ? 'PercussionOnly' : baseLock.mode,
        style,
        custom_pattern: null,
      };
      await this.stage.sendCommand({
        type: 'UpdateSetlistSlotConfig',
        payload: { slot: item.slot, percussion_lock: nextLock },
      });
    }
  }

  sectionForSlot(item: SetlistItemSnapshot): SetlistSection | null {
    if (!item.section_id) return null;
    return this.snap().setlist_sections.find((s) => s.id === item.section_id) ?? null;
  }

  isSectionBoundary(index: number): SetlistSection | null {
    const items = this.snap().setlist_items;
    const item = items[index];
    if (!item?.section_id) return null;
    if (index === 0 || items[index - 1]?.section_id !== item.section_id) {
      return this.sectionForSlot(item);
    }
    return null;
  }

  async addServiceSection(): Promise<void> {
    const section = buildNewSection(
      this.newSectionKind(),
      this.snap().setlist_sections,
      this.newSectionName(),
    );
    await this.stage.sendCommand({ type: 'UpsertSetlistSection', payload: section });
    this.newSectionName.set('');
  }

  async removeServiceSection(id: string): Promise<void> {
    await this.stage.sendCommand({ type: 'RemoveSetlistSection', payload: id });
  }

  async createStandaloneSlot(): Promise<void> {
    const [bpbRaw, unitRaw] = this.newStandaloneTimeSig().split('/').map(Number);
    await this.stage.sendCommand({
      type: 'AddStandaloneSetlistSlot',
      payload: {
        title: this.newStandaloneTitle().trim() || 'Standalone Slot',
        slot_kind: this.newStandaloneKind(),
        key: this.newStandaloneKey() || 'C',
        bpm: Number(this.newStandaloneBpm()) || 72,
        time_signature: [
          Number.isFinite(bpbRaw) && bpbRaw > 0 ? bpbRaw : 4,
          Number.isFinite(unitRaw) && unitRaw > 0 ? unitRaw : 4,
        ],
        section_id: this.newStandaloneSectionId() || null,
        insert_at: null,
      },
    });
    this.addStandaloneOpen.set(false);
  }

  isStudioSlotChecked(slot: number): boolean {
    return this.selectedStudioSlots().includes(slot);
  }

  onToggleStudioMultiSelect(slot: number, event?: MouseEvent): void {
    event?.stopPropagation();
    const orderedSlots = this.snap().setlist_items.map((it) => it.slot);
    const { nextSelected, nextAnchor } = toggleMultiSelectSlot(
      this.selectedStudioSlots(),
      this.selectionAnchorSlot(),
      slot,
      Boolean(event?.shiftKey),
      orderedSlots,
    );
    this.selectedStudioSlots.set(nextSelected);
    this.selectionAnchorSlot.set(nextAnchor);
  }

  async applyStudioBatchEdit(): Promise<void> {
    const slots = this.selectedStudioSlots().filter(
      (s) => s >= 0 && s < this.snap().setlist_items.length,
    );
    if (slots.length === 0) return;

    const patch: SetlistBatchPatch = { slots };
    let changed = false;
    if (this.batchTargetKey()) {
      patch.target_key = this.batchTargetKey();
      changed = true;
    }
    const parsedBpm = parseBatchBpmInput(this.batchBpm());
    if (parsedBpm?.kind === 'reset') {
      patch.bpm_override = null;
      changed = true;
    } else if (parsedBpm?.kind === 'bpm') {
      patch.bpm_override = parsedBpm.value;
      changed = true;
    }
    if (this.batchClickMode() === 'reset') {
      patch.click_mode = null;
      changed = true;
    } else if (this.batchClickMode()) {
      patch.click_mode = this.batchClickMode() as ClickMode;
      changed = true;
    }
    if (this.batchPadEnabled() === 'on') {
      patch.ambient_pad_enabled = true;
      changed = true;
    } else if (this.batchPadEnabled() === 'off') {
      patch.ambient_pad_enabled = false;
      changed = true;
    }
    const parsedXf = parseBatchCrossfadeInput(this.batchCrossfadeSec());
    if (parsedXf !== null) {
      patch.crossfade_sec = parsedXf;
      changed = true;
    }
    if (this.batchSectionId() === '__none__') {
      patch.section_id = null;
      changed = true;
    } else if (this.batchSectionId()) {
      patch.section_id = this.batchSectionId();
      changed = true;
    }
    if (this.batchPercussionMode()) {
      patch.percussion_mode = this.batchPercussionMode() as PercussionMode;
      changed = true;
    }
    const groovePatch = resolveBatchGroovePatch(
      this.batchPercussionStyle(),
      this.stage.percussionLoops(),
      this.snap().percussion_custom_pattern,
    );
    if (groovePatch) {
      Object.assign(patch, groovePatch);
      changed = true;
    }
    if (!changed) return;
    await this.stage.sendCommand({ type: 'BatchUpdateSetlistSlots', payload: patch });
  }

  async setSlotBpm(item: SetlistItemSnapshot, bpmStr: string | number): Promise<void> {
    const val = Number(bpmStr);
    if (!Number.isFinite(val) || val < 30 || val > 260) return;
    const rounded = Math.round(val * 10) / 10;
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, bpm_override: rounded },
      });
      return;
    }
    const override = Math.abs(rounded - item.base_bpm) < 0.05 ? null : rounded;
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        bpm_override: override,
      },
    });
  }

  async setSlotTransitionMode(item: SetlistItemSnapshot, mode: TransitionMode): Promise<void> {
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, transition_mode: mode },
      });
      return;
    }
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        transition_mode: mode,
      },
    });
  }

  async setSlotClickMode(item: SetlistItemSnapshot, mode: ClickMode): Promise<void> {
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, click_mode: mode },
      });
      return;
    }
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        click_mode: mode,
      },
    });
  }

  async toggleSlotPad(item: SetlistItemSnapshot): Promise<void> {
    const nextPad = !item.ambient_pad_enabled;
    const batchSlots = resolveBatchTargetSlots(
      this.selectedStudioSlots(),
      item.slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      await this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, ambient_pad_enabled: nextPad },
      });
      return;
    }
    await this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: {
        slot: item.slot,
        ambient_pad_enabled: nextPad,
      },
    });
  }

  private async dispatchStudioBatchFields(fields: Omit<SetlistBatchPatch, 'slots'>): Promise<void> {
    const slots = this.selectedStudioSlots().filter(
      (s) => s >= 0 && s < this.snap().setlist_items.length,
    );
    if (slots.length === 0) return;
    await this.stage.sendCommand({
      type: 'BatchUpdateSetlistSlots',
      payload: { slots, ...fields },
    });
  }

  onStudioBatchKeyChange(val: string): void {
    this.batchTargetKey.set(val);
    if (val) void this.dispatchStudioBatchFields({ target_key: val });
  }

  onStudioBatchClickChange(val: string): void {
    this.batchClickMode.set(val);
    if (val === 'reset') void this.dispatchStudioBatchFields({ click_mode: null });
    else if (val) void this.dispatchStudioBatchFields({ click_mode: val as ClickMode });
  }

  onStudioBatchPadChange(val: string): void {
    this.batchPadEnabled.set(val);
    if (val === 'on') void this.dispatchStudioBatchFields({ ambient_pad_enabled: true });
    else if (val === 'off') void this.dispatchStudioBatchFields({ ambient_pad_enabled: false });
  }

  onStudioBatchSectionChange(val: string): void {
    this.batchSectionId.set(val);
    if (val === '__none__') void this.dispatchStudioBatchFields({ section_id: null });
    else if (val) void this.dispatchStudioBatchFields({ section_id: val });
  }

  onStudioBatchPercChange(val: string): void {
    this.batchPercussionMode.set(val);
    if (val) void this.dispatchStudioBatchFields({ percussion_mode: val as PercussionMode });
  }

  onStudioBatchPercStyleChange(val: string): void {
    this.batchPercussionStyle.set(val);
    const groovePatch = resolveBatchGroovePatch(
      val,
      this.stage.percussionLoops(),
      this.snap().percussion_custom_pattern,
    );
    if (groovePatch) {
      void this.dispatchStudioBatchFields(groovePatch);
    }
  }

  unsavedCustomLoopPattern() {
    const custom = this.snap().percussion_custom_pattern;
    if (!custom) return null;
    return this.stage.percussionLoops().some((l) => l.id === custom.id) ? null : custom;
  }

  onStudioBatchBpmChange(val: string | number): void {
    const nextStr = String(val ?? '');
    this.batchBpm.set(nextStr);
    const parsed = parseBatchBpmInput(nextStr);
    if (parsed?.kind === 'reset') {
      void this.dispatchStudioBatchFields({ bpm_override: null });
    } else if (parsed?.kind === 'bpm') {
      void this.dispatchStudioBatchFields({ bpm_override: parsed.value });
    }
  }

  onStudioBatchCrossfadeChange(val: string | number): void {
    const nextStr = String(val ?? '');
    this.batchCrossfadeSec.set(nextStr);
    const xf = parseBatchCrossfadeInput(nextStr);
    if (xf !== null) {
      void this.dispatchStudioBatchFields({ crossfade_sec: xf });
    }
  }

  async clearSlotPadLock(item: SetlistItemSnapshot): Promise<void> {
    await this.stage.sendCommand({ type: 'ClearSlotPadLock', payload: item.slot });
  }

  async moveSlot(from: number, to: number): Promise<void> {
    if (to < 0 || to >= this.snap().setlist_items.length) return;
    await this.stage.sendCommand({
      type: 'MoveSetlistSlot',
      payload: { from, to },
    });
    this.selectedStudioSlot.set(to);
  }

  async removeSlot(slot: number): Promise<void> {
    await this.stage.sendCommand({ type: 'RemoveSetlistSlot', payload: slot });
  }

  async clearSetlist(): Promise<void> {
    await this.stage.sendCommand({ type: 'ClearSetlist' });
    this.selectedStudioSlot.set(null);
  }

  async linkNewFolderViaPicker(): Promise<void> {
    const picked = await this.stage.pickSongFolder();
    if (picked) {
      await this.stage.linkLibraryFolder(picked);
    }
  }

  async linkFolderFromInput(): Promise<void> {
    const raw = this.linkFolderInput().trim();
    if (!raw) return;
    await this.stage.linkLibraryFolder(raw);
    this.linkFolderInput.set('');
  }

  async changeLibraryRootViaPicker(): Promise<void> {
    const picked = await this.stage.pickSongFolder();
    if (picked) {
      await this.stage.saveLibraryConfig({
        ...this.stage.libraryConfig(),
        library_root: picked,
      });
    }
  }

  async changeSetlistsRootViaPicker(): Promise<void> {
    const picked = await this.stage.pickSongFolder();
    if (picked) {
      await this.stage.saveLibraryConfig({
        ...this.stage.libraryConfig(),
        setlists_root: picked,
      });
    }
  }

  toggleAiSetup(): void {
    const next = !this.showAiSetup();
    this.showAiSetup.set(next);
    if (next) {
      void this.stage.loadAiHarnessInfo();
    }
  }

  copyAiText(label: string, text: string): void {
    if (typeof navigator !== 'undefined' && navigator.clipboard) {
      void navigator.clipboard.writeText(text);
    }
    this.copiedAiLabel.set(label);
    setTimeout(() => {
      if (this.copiedAiLabel() === label) {
        this.copiedAiLabel.set(null);
      }
    }, 2000);
  }

  libraryEntryForSlot(item: SetlistItemSnapshot): LibrarySongEntry | null {
    return (
      this.stage
        .libraryEntries()
        .find((e) => e.dir === item.song_dir || e.song_id === item.song_id) ?? null
    );
  }

  slotNeedsReview(item: SetlistItemSnapshot): boolean {
    return Boolean(this.libraryEntryForSlot(item)?.provenance?.needs_review);
  }

  async loadManagedSetlist(summary: SetlistSummary): Promise<void> {
    const loaded = await this.stage.loadSetlistFromDisk(summary.path);
    if (loaded) {
      await this.stage.sendCommand({ type: 'LoadSetlist', payload: loaded });
      this.stage.dismissExternalSyncNotice();
      this.showSavedSetlists.set(false);
    }
  }

  async openSetlistFile(): Promise<void> {
    const file = await this.stage.pickOpenSetlistFile();
    if (!file) return;
    const loaded = await this.stage.loadSetlistFromDisk(file);
    if (loaded) {
      await this.stage.sendCommand({ type: 'LoadSetlist', payload: loaded });
    }
  }

  async saveSetlistFile(): Promise<void> {
    const manifest = serializeSetlistManifest(this.snap());
    const path = await this.stage.pickSaveSetlistFile(`${manifest.id}.json`);
    if (!path) return;
    await this.stage.saveSetlistToDisk(path, manifest);
  }
}

