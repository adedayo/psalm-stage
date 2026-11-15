import {
  CdkDrag,
  CdkDragDrop,
  CdkDragHandle,
  CdkDropList,
  CdkDropListGroup,
} from '@angular/cdk/drag-drop';
import { DecimalPipe, UpperCasePipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  HostListener,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { HlmBadgeImports } from '@spartan-ng/helm/badge';
import { HlmButtonImports } from '@spartan-ng/helm/button';
import { HlmCardImports } from '@spartan-ng/helm/card';
import { HlmSliderImports } from '@spartan-ng/helm/slider';
import { HlmSwitchImports } from '@spartan-ng/helm/switch';
import {
  PERCUSSION_INSTRUMENT_META,
  PERCUSSION_STYLE_OPTIONS,
  SERVICE_SECTION_PRESETS,
  type ClickMode,
  type CustomPatternHit,
  type CustomPercussionPattern,
  type GrooveIntensity,
  type HitVariant,
  type MidiTarget,
  type MidiTriggerBehavior,
  type OutputRoutingMode,
  type PadLayerConfig,
  type PadPreset,
  type PadRegister,
  type PadTonality,
  type PadVoiceArchetype,
  type PercussionInstrument,
  type PercussionMode,
  type PercussionStyle,
  type QuantizeMode,
  type RawMidiEvent,
  type ServiceSectionKind,
  type SetlistBatchPatch,
  type SetlistItemSnapshot,
  type SetlistSection,
  type SetlistSlotKind,
  type SongManifest,
  type StemDefDetailed,
  type StudioKitProfile,
  type TransitionMode,
  type WorkspaceMode,
} from './models/engine.models';
import {
  buildNewSection,
  buildSetlistBuckets,
  computeCrossBucketMove,
  parseBatchBpmInput,
  parseBatchCrossfadeInput,
  resolveBatchGroovePatch,
  resolveBatchTargetSlots,
  serializeSetlistManifest,
  toggleMultiSelectSlot,
  type SetlistBucket,
} from './models/setlist.helpers';
import { ImportWorkbenchComponent } from './components/import-workbench/import-workbench.component';
import { LoopStudioComponent } from './components/loop-studio/loop-studio.component';
import { SetlistStudioComponent } from './components/setlist-studio/setlist-studio.component';
import { expandPatternBars, rescalePatternGrid, sanitizeLoopId } from './models/loop-pattern.helpers';
import { PsalmStageService, SONG_KEY_OPTIONS } from './services/psalm-stage.service';

export const UI_SESSION_STORAGE_KEY = 'psalm-stage.ui-session.v1';

interface UiSessionPrefs {
  workspaceMode: WorkspaceMode;
  workbenchTargetDir: string | null;
  customFolderPath: string;
  selectedMidiPortIndex: number;
  padStudioOpen: boolean;
  percussionMixerExpanded: boolean;
}

const DEFAULT_UI_SESSION: UiSessionPrefs = {
  workspaceMode: 'stage',
  workbenchTargetDir: null,
  customFolderPath: '',
  selectedMidiPortIndex: -1,
  padStudioOpen: false,
  percussionMixerExpanded: false,
};

function loadUiSession(): UiSessionPrefs {
  if (typeof window === 'undefined' || !window.localStorage) {
    return DEFAULT_UI_SESSION;
  }
  try {
    const raw = window.localStorage.getItem(UI_SESSION_STORAGE_KEY);
    if (!raw) return DEFAULT_UI_SESSION;
    const parsed = JSON.parse(raw) as Partial<UiSessionPrefs> | null;
    if (!parsed || typeof parsed !== 'object') return DEFAULT_UI_SESSION;
    const mode = parsed.workspaceMode;
    const workspaceMode: WorkspaceMode =
      mode === 'stage' || mode === 'studio' || mode === 'loops' || mode === 'workbench'
        ? mode
        : 'stage';
    return {
      workspaceMode,
      workbenchTargetDir:
        typeof parsed.workbenchTargetDir === 'string' ? parsed.workbenchTargetDir : null,
      customFolderPath: typeof parsed.customFolderPath === 'string' ? parsed.customFolderPath : '',
      selectedMidiPortIndex:
        typeof parsed.selectedMidiPortIndex === 'number' &&
        Number.isFinite(parsed.selectedMidiPortIndex)
          ? parsed.selectedMidiPortIndex
          : -1,
      padStudioOpen: Boolean(parsed.padStudioOpen),
      percussionMixerExpanded: Boolean(parsed.percussionMixerExpanded),
    };
  } catch {
    return DEFAULT_UI_SESSION;
  }
}

function saveUiSession(prefs: UiSessionPrefs): void {
  if (typeof window === 'undefined' || !window.localStorage) return;
  try {
    window.localStorage.setItem(UI_SESSION_STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Ignore storage quota / private mode errors gracefully.
  }
}

export interface EditableStemEntry {
  key: string;
  file: string;
  pitched: boolean;
  is_cues: boolean;
  default_gain: number;
}

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [
    DecimalPipe,
    UpperCasePipe,
    FormsModule,
    CdkDropListGroup,
    CdkDropList,
    CdkDrag,
    CdkDragHandle,
    HlmButtonImports,
    HlmBadgeImports,
    HlmCardImports,
    HlmSliderImports,
    HlmSwitchImports,
    SetlistStudioComponent,
    LoopStudioComponent,
    ImportWorkbenchComponent,
  ],
  templateUrl: './app.html',
  styleUrl: './app.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  private readonly initialUiSession = loadUiSession();
  protected readonly stage = inject(PsalmStageService);
  protected readonly snap = this.stage.snapshot;
  protected readonly workspaceMode = signal<WorkspaceMode>(this.initialUiSession.workspaceMode);
  protected readonly workbenchTargetDir = signal<string | null>(
    this.initialUiSession.workbenchTargetDir,
  );
  protected readonly liveLock = signal(false);
  protected readonly customFolderPath = signal(this.initialUiSession.customFolderPath);
  protected readonly soloedStageStems = signal<string[]>([]);
  private preSoloStageMuted: Record<string, boolean> | null = null;
  protected readonly selectedMidiPortIndex = signal<number>(
    this.initialUiSession.selectedMidiPortIndex,
  );
  protected readonly chromaticKeys = [
    'C',
    'C#',
    'D',
    'Eb',
    'E',
    'F',
    'F#',
    'G',
    'Ab',
    'A',
    'Bb',
    'B',
  ];
  protected readonly songKeyOptions = SONG_KEY_OPTIONS;

  // Feature #16: Tonic Pad Studio state
  protected readonly padStudioOpen = signal(this.initialUiSession.padStudioOpen);
  protected readonly padPresetFilter = signal<'all' | 'factory' | 'custom'>('all');
  protected readonly padNewPresetName = signal('');
  protected readonly padStudioNotice = signal<string | null>(null);
  protected readonly padArchetypeOptions: { id: PadVoiceArchetype; label: string }[] = [
    { id: 'WarmAnalog', label: 'Warm Analog' },
    { id: 'BrightSynth', label: 'Bright Synth' },
    { id: 'EvolvingMotion', label: 'Evolving Motion' },
    { id: 'StringsCello', label: 'Strings / Cello' },
    { id: 'OrganPad', label: 'Warm Organ' },
    { id: 'GuitarSwell', label: 'Guitar Swell' },
    { id: 'ReversePiano', label: 'Reverse Piano' },
    { id: 'AiryChoir', label: 'Airy Choir' },
  ];
  protected readonly padRegisterOptions: { id: PadRegister; label: string }[] = [
    { id: 'Low', label: 'Low' },
    { id: 'Mid', label: 'Mid' },
    { id: 'High', label: 'High' },
  ];
  protected readonly padTonalityOptions: { id: PadTonality; label: string }[] = [
    { id: 'Auto', label: 'Auto (Follow Key)' },
    { id: 'OpenFifth', label: 'Open 5th' },
    { id: 'Major', label: 'Major' },
    { id: 'Minor', label: 'Minor' },
    { id: 'Melodic', label: 'Melodic (9th)' },
  ];
  protected readonly filteredPadPresets = computed<PadPreset[]>(() => {
    const filter = this.padPresetFilter();
    const list = this.stage.padPresets();
    if (filter === 'factory') return list.filter((p) => p.is_factory);
    if (filter === 'custom') return list.filter((p) => !p.is_factory);
    return list;
  });
  protected readonly activeSetlistItem = computed(() => {
    const s = this.snap();
    return s.active_setlist_slot !== null ? (s.setlist_items[s.active_setlist_slot] ?? null) : null;
  });

  // SPEC-017 / SPEC-018 / SPEC-019: Live Stage Setlist Sections, Multi-Select Batch & Percussion Rack
  protected readonly sectionPresetOptions = SERVICE_SECTION_PRESETS;
  protected readonly percussionStyleOptions = PERCUSSION_STYLE_OPTIONS;
  protected readonly percussionInstrumentMeta = PERCUSSION_INSTRUMENT_META;
  protected readonly setlistBuckets = computed<SetlistBucket[]>(() =>
    buildSetlistBuckets(this.snap().setlist_sections, this.snap().setlist_items),
  );
  protected readonly selectedSetlistSlots = signal<number[]>([]);
  protected readonly selectionAnchorSlot = signal<number | null>(null);
  protected readonly batchTargetKey = signal<string>('');
  protected readonly batchBpm = signal<string>('');
  protected readonly batchClickMode = signal<string>('');
  protected readonly batchPadEnabled = signal<string>('');
  protected readonly batchTransitionMode = signal<string>('');
  protected readonly batchCrossfadeSec = signal<string>('');
  protected readonly batchSectionId = signal<string>('');
  protected readonly batchPercussionMode = signal<string>('');
  protected readonly batchPercussionStyle = signal<string>('');
  protected readonly batchPercussionIntensity = signal<string>('');
  protected readonly addSectionKind = signal<ServiceSectionKind>('Praise');
  protected readonly addSectionName = signal<string>('');
  protected readonly addSlotDrawerOpen = signal<boolean>(false);
  protected readonly newStandaloneTitle = signal<string>('Sermon Underscore');
  protected readonly newStandaloneKind = signal<SetlistSlotKind>('PadOnly');
  protected readonly newStandaloneKey = signal<string>('D');
  protected readonly newStandaloneBpm = signal<number>(72);
  protected readonly newStandaloneTimeSig = signal<string>('4/4');
  protected readonly newStandaloneSectionId = signal<string>('');
  protected readonly percussionMixerExpanded = signal<boolean>(
    this.initialUiSession.percussionMixerExpanded,
  );
  protected readonly percussionStudioOpen = signal<boolean>(false);
  protected readonly loopDraft = signal<CustomPercussionPattern>({
    id: 'sunday-makossa',
    name: 'Sunday Makossa Loop',
    steps_per_bar: 16,
    bars_in_cycle: 1,
    swing: 0.03,
    is_compound: false,
    hits: [],
  });
  protected readonly loopPaintIntensity = signal<GrooveIntensity>('Low');
  protected readonly loopPaintVariant = signal<HitVariant>('Standard');
  protected readonly loopExportTitle = signal<string>('Sunday Makossa Groove');
  protected readonly loopExportBars = signal<number>(16);
  protected readonly loopExportDynamic = signal<boolean>(true);
  protected readonly loopExportBusy = signal<boolean>(false);
  protected readonly loopKitBusy = signal<boolean>(false);
  protected readonly loopExportStatus = signal<string | null>(null);

  constructor() {
    effect(() => {
      saveUiSession({
        workspaceMode: this.workspaceMode(),
        workbenchTargetDir: this.workbenchTargetDir(),
        customFolderPath: this.customFolderPath(),
        selectedMidiPortIndex: this.selectedMidiPortIndex(),
        padStudioOpen: this.padStudioOpen(),
        percussionMixerExpanded: this.percussionMixerExpanded(),
      });
    });
  }

  // Feature #2: In-App Song Setup & Section Marker Editor state
  protected readonly songEditorOpen = signal(false);
  protected readonly songEditorDir = signal('');
  protected readonly songEditorHasManifest = signal(false);
  protected readonly songEditorDurationSec = signal(0);
  protected readonly songEditorAvailableFiles = signal<string[]>([]);
  protected readonly songEditorManifest = signal<SongManifest | null>(null);
  protected readonly songEditorAddToSetlist = signal(false);
  protected readonly songEditorError = signal<string | null>(null);

  protected readonly editableStems = computed<EditableStemEntry[]>(() => {
    const m = this.songEditorManifest();
    if (!m) return [];
    return Object.entries(m.stems).map(([key, def]) => {
      if (typeof def === 'string') {
        const lower = key.toLowerCase();
        return {
          key,
          file: def,
          pitched: ![
            'drums',
            'percussion',
            'cues',
            'click',
            'kick',
            'snare',
            'hihat',
            'hand_drums',
            'tambourine',
            'shaker',
            'claps',
            'clave',
            'bell',
          ].includes(lower),
          is_cues: lower === 'cues' || lower === 'cue' || lower === 'guide',
          default_gain: 1.0,
        };
      }
      return {
        key,
        file: def.file,
        pitched: def.pitched,
        is_cues: def.is_cues ?? key.toLowerCase() === 'cues',
        default_gain: def.default_gain ?? 1.0,
      };
    });
  });

  protected readonly beatDots = computed(() => {
    const count = Math.max(1, this.snap().beats_per_bar);
    return Array.from({ length: count }, (_, idx) => idx + 1);
  });

  protected readonly isMultiOutRouting = computed(() => {
    const mode = this.snap().routing_mode;
    return mode === 'MultiOutStereo' || mode === 'MultiOutClickLeftMusicRight';
  });

  protected readonly isClickFirstRouting = computed(() => {
    const mode = this.snap().routing_mode;
    return mode === 'SplitClickLeftMusicRight' || mode === 'MultiOutClickLeftMusicRight';
  });

  protected readonly shouldShowUpdateBanner = computed(() => {
    const update = this.stage.availableUpdate();
    const dismissed = this.stage.updateDismissed();
    const isPlaying = this.snap().is_playing;
    return update !== null && !dismissed && !isPlaying;
  });

  @HostListener('window:keydown', ['$event'])
  protected onGlobalKeydown(ev: KeyboardEvent): void {
    const tag = (ev.target as HTMLElement | null)?.tagName;
    if (
      tag === 'INPUT' ||
      tag === 'SELECT' ||
      tag === 'TEXTAREA' ||
      this.songEditorOpen() ||
      this.workspaceMode() !== 'stage'
    ) {
      return;
    }
    if (ev.code === 'Space') {
      ev.preventDefault();
      void this.stage.sendCommand({ type: 'PlayStop' });
    } else if (ev.key === 'l' || ev.key === 'L') {
      ev.preventDefault();
      void this.stage.sendCommand({ type: 'ToggleLoop' });
    } else if (ev.key === 'p' || ev.key === 'P') {
      ev.preventDefault();
      void this.stage.sendCommand({ type: 'TogglePad' });
    } else if (ev.key === 'm' || ev.key === 'M') {
      ev.preventDefault();
      void this.stage.sendCommand({ type: 'ToggleClickMuted' });
    } else if (/^[1-9]$/.test(ev.key)) {
      const slot = Number(ev.key) - 1;
      if (slot < this.snap().sections.length) {
        ev.preventDefault();
        void this.stage.sendCommand({ type: 'QueueSectionBySlot', payload: slot });
      }
    }
  }

  protected setWorkspaceMode(mode: WorkspaceMode): void {
    if (this.liveLock() && mode !== 'stage') {
      return;
    }
    this.workspaceMode.set(mode);
  }

  protected onLiveLockChange(locked: boolean): void {
    this.liveLock.set(locked);
    this.stage.liveLockActive.set(locked);
    void this.stage.setLiveKeepAwake(locked);
    if (locked) {
      this.workspaceMode.set('stage');
    }
  }

  protected openSongDirInWorkbench(dir: string): void {
    if (this.liveLock()) return;
    this.workbenchTargetDir.set(dir);
    this.workspaceMode.set('workbench');
  }

  protected onPlayStop(): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget({ type: 'PlayStop' }, 'Play / Stop', 'Momentary');
      return;
    }
    void this.stage.sendCommand({ type: 'PlayStop' });
  }

  protected onToggleLoop(): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget({ type: 'LoopToggle' }, 'Loop Toggle', 'Toggle');
      return;
    }
    void this.stage.sendCommand({ type: 'ToggleLoop' });
  }

  protected onToggleContinueTrack(): void {
    void this.stage.sendCommand({ type: 'ToggleContinueTrack' });
  }

  protected onSectionTap(sectionId: string, sectionName: string, slotIndex: number): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget(
        { type: 'SectionByName', value: sectionName },
        `Section: ${sectionName}`,
        'Momentary',
      );
      return;
    }
    void this.stage.sendCommand({ type: 'QueueSectionById', payload: sectionId });
    void slotIndex;
  }

  protected onKeyAdjust(delta: number): void {
    if (this.stage.midiAssignMode()) {
      const target: MidiTarget = delta > 0 ? { type: 'KeyUp' } : { type: 'KeyDown' };
      this.stage.startListeningForTarget(target, delta > 0 ? 'Key +1' : 'Key -1', 'Momentary');
      return;
    }
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'AdjustKeyShift', payload: delta });
  }

  protected onBpmAdjust(delta: number): void {
    if (this.stage.midiAssignMode()) {
      const target: MidiTarget = delta > 0 ? { type: 'TempoUp' } : { type: 'TempoDown' };
      this.stage.startListeningForTarget(
        target,
        delta > 0 ? 'Tempo +1 BPM' : 'Tempo -1 BPM',
        'Momentary',
      );
      return;
    }
    if (this.liveLock()) return;
    const selected = this.validSelectedSlots();
    if (selected.length > 0) {
      const nextBpm = Math.max(
        30,
        Math.min(260, Math.round((this.snap().current_bpm + delta) * 10) / 10),
      );
      this.applyBpmToSelectedAndCurrent(nextBpm, selected);
      return;
    }
    void this.stage.sendCommand({ type: 'AdjustBpm', payload: delta });
  }

  protected onHeaderBpmChange(val: string | number): void {
    if (this.liveLock()) return;
    const parsed = parseBatchBpmInput(val);
    if (parsed?.kind !== 'bpm') return;
    const selected = this.validSelectedSlots();
    if (selected.length > 0) {
      this.applyBpmToSelectedAndCurrent(parsed.value, selected);
      return;
    }
    void this.stage.sendCommand({ type: 'SetBpm', payload: parsed.value });
  }

  private applyBpmToSelectedAndCurrent(bpm: number, selected: number[]): void {
    this.dispatchBatchToSelected({ bpm_override: bpm });
    const activeSlot = this.snap().active_setlist_slot;
    if (activeSlot === null || !selected.includes(activeSlot)) {
      void this.stage.sendCommand({ type: 'SetBpm', payload: bpm });
    }
  }

  protected onQuantizeChange(mode: QuantizeMode): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'SetQuantizeMode', payload: mode });
  }

  protected onRoutingChange(mode: OutputRoutingMode): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'SetRoutingMode', payload: mode });
  }

  protected onRoutingLayoutChange(multiOut: boolean): void {
    this.onRoutingChange(this.resolveRoutingMode(multiOut, this.isClickFirstRouting()));
  }

  protected onRoutingSideChange(clickFirst: boolean): void {
    this.onRoutingChange(this.resolveRoutingMode(this.isMultiOutRouting(), clickFirst));
  }

  private resolveRoutingMode(multiOut: boolean, clickFirst: boolean): OutputRoutingMode {
    if (multiOut) {
      return clickFirst ? 'MultiOutClickLeftMusicRight' : 'MultiOutStereo';
    }
    return clickFirst ? 'SplitClickLeftMusicRight' : 'SplitLeftRight';
  }

  protected onDeviceSelect(valueOrEvent: string | Event): void {
    if (this.liveLock()) return;
    const name =
      typeof valueOrEvent === 'string'
        ? valueOrEvent
        : (valueOrEvent.target as HTMLSelectElement).value;
    if (name) {
      void this.stage.useDetectedOutput(name);
    }
  }

  protected onMasterSlider(values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetMasterGain', payload: val });
    }
  }

  protected onClickSlider(values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetClickGain', payload: val });
    }
  }

  protected onClickMute(muted: boolean): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget({ type: 'ToggleClick' }, 'Mute / Unmute Metronome', 'Toggle');
      return;
    }
    void this.stage.sendCommand({ type: 'SetClickMuted', payload: muted });
    if (!this.liveLock()) {
      this.dispatchBatchToSelected({ click_mode: muted ? 'Off' : 'AlwaysOn' });
    }
  }

  protected onClickModeChange(mode: ClickMode): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'SetClickMode', payload: mode });
    this.dispatchBatchToSelected({ click_mode: mode });
  }

  protected cycleSlotClickMode(slot: number, current: ClickMode): void {
    if (this.liveLock()) return;
    const next: ClickMode =
      current === 'AlwaysOn' ? 'IntroOnly' : current === 'IntroOnly' ? 'Off' : 'AlwaysOn';
    const batchSlots = resolveBatchTargetSlots(
      this.selectedSetlistSlots(),
      slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      void this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, click_mode: next },
      });
      return;
    }
    void this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: { slot, click_mode: next },
    });
  }

  protected onStemSlider(name: string, values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetStemGain', payload: { name, gain: val } });
    }
  }

  protected onStemMute(name: string, muted: boolean): void {
    this.soloedStageStems.set([]);
    this.preSoloStageMuted = null;
    void this.stage.sendCommand({ type: 'SetStemMuted', payload: { name, muted } });
  }

  protected isStageStemSoloed(name: string): boolean {
    return this.soloedStageStems().includes(name);
  }

  protected onStemSolo(name: string): void {
    const stems = this.snap().stems;
    if (stems.length === 0) return;

    const currentSolo = this.soloedStageStems();
    if (currentSolo.length === 0 && !this.preSoloStageMuted) {
      const snapshot: Record<string, boolean> = {};
      for (const st of stems) {
        snapshot[st.name] = st.muted;
      }
      this.preSoloStageMuted = snapshot;
    }

    const nextSolo = currentSolo.includes(name)
      ? currentSolo.filter((k) => k !== name)
      : [...currentSolo, name];
    this.soloedStageStems.set(nextSolo);

    if (nextSolo.length === 0) {
      const restore = this.preSoloStageMuted ?? {};
      this.preSoloStageMuted = null;
      for (const st of stems) {
        void this.stage.sendCommand({
          type: 'SetStemMuted',
          payload: { name: st.name, muted: restore[st.name] ?? false },
        });
      }
      return;
    }

    for (const st of stems) {
      void this.stage.sendCommand({
        type: 'SetStemMuted',
        payload: { name: st.name, muted: !nextSolo.includes(st.name) },
      });
    }
  }

  protected mapMidiFader(target: MidiTarget, label: string, behavior: MidiTriggerBehavior): void {
    if (!this.stage.midiAssignMode()) return;
    this.stage.startListeningForTarget(target, label, behavior);
  }

  // --- Feature #3: Ambient Drone Pad Handlers ---

  protected onTogglePad(): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget({ type: 'TogglePad' }, 'Toggle Ambient Pad', 'Toggle');
      return;
    }
    const nextPadEnabled = !this.snap().pad_enabled;
    void this.stage.sendCommand({ type: 'TogglePad' });
    if (!this.liveLock()) {
      this.dispatchBatchToSelected({ ambient_pad_enabled: nextPadEnabled });
    }
  }

  protected onPadMute(muted: boolean): void {
    void this.stage.sendCommand({ type: 'SetPadMuted', payload: muted });
  }

  protected onPadSlider(values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetPadGain', payload: val });
    }
  }

  protected onSelectPadKey(key: string): void {
    const label = this.snap().pad_is_minor ? `${key}m` : key;
    void this.stage.sendCommand({ type: 'SetPadKey', payload: label });
  }

  protected onTogglePadMinor(): void {
    const s = this.snap();
    const root = s.pad_key.endsWith('m') && !s.pad_key.toLowerCase().endsWith('maj')
      ? s.pad_key.slice(0, -1)
      : s.pad_key;
    const next = s.pad_is_minor ? root : `${root}m`;
    void this.stage.sendCommand({ type: 'SetPadKey', payload: next });
  }

  protected isPadKeyRootSelected(key: string): boolean {
    const cur = this.snap().pad_key;
    const curRoot = cur.endsWith('m') && !cur.toLowerCase().endsWith('maj') ? cur.slice(0, -1) : cur;
    return curRoot === key;
  }

  protected onTogglePadFollow(follow: boolean): void {
    void this.stage.sendCommand({ type: 'SetPadFollowSongKey', payload: follow });
  }

  protected openPadStudio(): void {
    void this.stage.loadPadPresets();
    this.padStudioNotice.set(null);
    this.padStudioOpen.set(true);
  }

  protected closePadStudio(): void {
    this.padStudioOpen.set(false);
    this.padStudioNotice.set(null);
  }

  protected onPadBrightnessSlider(values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetPadBrightness', payload: val });
    }
  }

  protected onPadShimmerSlider(values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetPadShimmer', payload: val });
    }
  }

  protected onPadPanSlider(values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetPadPan', payload: val });
    }
  }

  protected onPadCrossfadeSlider(values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetPadCrossfadeSec', payload: val });
    }
  }

  protected onSelectPadPreset(preset: PadPreset): void {
    void this.stage.sendCommand({
      type: 'SetPadPatch',
      payload: {
        preset_id: preset.id,
        preset_name: preset.name,
        layers: preset.layers,
        brightness: preset.brightness,
        shimmer: preset.shimmer,
        pan: preset.pan,
        crossfade_sec: preset.crossfade_sec ?? null,
      },
    });
  }

  protected onUpdatePadLayer(index: number, patch: Partial<PadLayerConfig>): void {
    const cur = this.snap().pad_layers[index] ?? {
      enabled: false,
      archetype: 'WarmAnalog' as const,
      register: 'Mid' as const,
      tonality: 'Auto' as const,
      gain: 0.75,
    };
    void this.stage.sendCommand({
      type: 'SetPadLayer',
      payload: { index, layer: { ...cur, ...patch } },
    });
  }

  protected onPadLayerGain(index: number, values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({
        type: 'SetPadLayerGain',
        payload: { index, gain: val },
      });
    }
  }

  protected async onSaveCustomPadPreset(): Promise<void> {
    const name = this.padNewPresetName().trim();
    if (!name) {
      this.padStudioNotice.set('Enter a preset name first.');
      return;
    }
    const s = this.snap();
    const activeLayers = s.pad_layers.filter((l) => l.enabled);
    if (activeLayers.length === 0) {
      this.padStudioNotice.set('Enable at least one pad layer before saving.');
      return;
    }
    await this.stage.saveUserPadPreset({
      id: '',
      name,
      category: 'Custom',
      is_factory: false,
      layers: s.pad_layers,
      brightness: s.pad_brightness,
      shimmer: s.pad_shimmer,
      pan: s.pad_pan,
      crossfade_sec: s.pad_crossfade_sec,
    });
    this.padNewPresetName.set('');
    this.padStudioNotice.set(`Saved preset "${name}".`);
  }

  protected async onDeleteCustomPadPreset(id: string): Promise<void> {
    await this.stage.deleteUserPadPreset(id);
  }

  protected onToggleActiveSlotPadLock(): void {
    const slot = this.snap().active_setlist_slot;
    if (slot === null) return;
    const item = this.activeSetlistItem();
    if (item?.pad_lock) {
      void this.stage.sendCommand({ type: 'ClearSlotPadLock', payload: slot });
    } else {
      void this.stage.sendCommand({ type: 'LockCurrentPadToSlot', payload: slot });
    }
  }

  // --- Feature #1: Setlist Handlers ---

  protected async browseAndAddSongToSetlist(): Promise<void> {
    if (this.liveLock()) return;
    const dir = await this.stage.pickSongFolder();
    if (!dir) return;
    await this.addOrSetupSongFolder(dir, true);
  }

  protected async addTypedPathToSetlist(): Promise<void> {
    const dir = this.customFolderPath().trim();
    if (!dir || this.liveLock()) return;
    await this.addOrSetupSongFolder(dir, true);
  }

  private async addOrSetupSongFolder(dir: string, addToSetlist: boolean): Promise<void> {
    const inspected = await this.stage.inspectSongFolder(dir);
    if (!inspected) return;

    if (inspected.has_existing_manifest) {
      if (addToSetlist) {
        void this.stage.sendCommand({ type: 'AddSetlistSong', payload: { dir, insert_at: null } });
      } else {
        this.customFolderPath.set(dir);
        void this.stage.sendCommand({ type: 'LoadSongDir', payload: dir });
      }
      return;
    }

    // No manifest.json yet: open the Song Setup Builder pre-populated with auto-classified stems
    this.openEditorWithInspected(inspected, addToSetlist);
  }

  protected onPrevSong(): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget({ type: 'PrevSong' }, 'Previous Setlist Song', 'Momentary');
      return;
    }
    void this.stage.sendCommand({ type: 'PrevSong' });
  }

  protected onNextSong(): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget({ type: 'NextSong' }, 'Next Setlist Song', 'Momentary');
      return;
    }
    void this.stage.sendCommand({ type: 'NextSong' });
  }

  protected onSelectSetlistSlot(slot: number): void {
    void this.stage.sendCommand({ type: 'SelectSetlistSlot', payload: slot });
  }

  protected onMoveSetlistSlot(from: number, to: number): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'MoveSetlistSlot', payload: { from, to } });
  }

  protected onRemoveSetlistSlot(slot: number): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'RemoveSetlistSlot', payload: slot });
  }

  protected formatTransitionMode(mode: TransitionMode): string {
    return mode === 'ContinueUntilStopped' ? 'Continue' : mode;
  }

  protected cycleTransitionMode(slot: number, current: TransitionMode): void {
    if (this.liveLock()) return;
    const order: TransitionMode[] = ['AutoPause', 'AutoAdvance', 'ContinueUntilStopped', 'Manual'];
    const idx = order.indexOf(current);
    const next = order[(idx >= 0 ? idx + 1 : 0) % order.length];
    const batchSlots = resolveBatchTargetSlots(
      this.selectedSetlistSlots(),
      slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      void this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, transition_mode: next },
      });
      return;
    }
    void this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: { slot, transition_mode: next },
    });
  }

  protected toggleSlotPadEnabled(slot: number, current: boolean): void {
    if (this.liveLock()) return;
    const nextPad = !current;
    const batchSlots = resolveBatchTargetSlots(
      this.selectedSetlistSlots(),
      slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      void this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, ambient_pad_enabled: nextPad },
      });
      return;
    }
    void this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: { slot, ambient_pad_enabled: nextPad },
    });
  }

  private validSelectedSlots(): number[] {
    const len = this.snap().setlist_items.length;
    return this.selectedSetlistSlots().filter((s) => s >= 0 && s < len);
  }

  private dispatchBatchToSelected(fields: Omit<SetlistBatchPatch, 'slots'>): void {
    if (this.liveLock()) return;
    const slots = this.validSelectedSlots();
    if (slots.length === 0) return;
    void this.stage.sendCommand({
      type: 'BatchUpdateSetlistSlots',
      payload: { slots, ...fields },
    });
  }

  protected onBatchTargetKeyChange(val: string): void {
    this.batchTargetKey.set(val);
    if (val) this.dispatchBatchToSelected({ target_key: val });
  }

  protected onBatchClickModeChange(val: string): void {
    this.batchClickMode.set(val);
    if (val === 'reset') {
      this.dispatchBatchToSelected({ click_mode: null });
    } else if (val === 'AlwaysOn' || val === 'IntroOnly' || val === 'Off') {
      this.dispatchBatchToSelected({ click_mode: val as ClickMode });
    }
  }

  protected onBatchPadEnabledChange(val: string): void {
    this.batchPadEnabled.set(val);
    if (val === 'on') this.dispatchBatchToSelected({ ambient_pad_enabled: true });
    else if (val === 'off') this.dispatchBatchToSelected({ ambient_pad_enabled: false });
  }

  protected onBatchTransitionModeChange(val: string): void {
    this.batchTransitionMode.set(val);
    if (
      val === 'AutoPause' ||
      val === 'AutoAdvance' ||
      val === 'ContinueUntilStopped' ||
      val === 'Manual'
    ) {
      this.dispatchBatchToSelected({ transition_mode: val as TransitionMode });
    }
  }

  protected onBatchSectionIdChange(val: string): void {
    this.batchSectionId.set(val);
    if (val === '__none__') this.dispatchBatchToSelected({ section_id: null });
    else if (val) this.dispatchBatchToSelected({ section_id: val });
  }

  protected onBatchPercussionModeChange(val: string): void {
    this.batchPercussionMode.set(val);
    if (val === 'Off' || val === 'Layer' || val === 'PercussionOnly') {
      this.dispatchBatchToSelected({ percussion_mode: val as PercussionMode });
    }
  }

  protected onBatchPercussionStyleChange(val: string): void {
    this.batchPercussionStyle.set(val);
    const groovePatch = resolveBatchGroovePatch(
      val,
      this.stage.percussionLoops(),
      this.snap().percussion_custom_pattern,
    );
    if (groovePatch) {
      this.dispatchBatchToSelected(groovePatch);
    }
  }

  protected onBatchPercussionIntensityChange(val: string): void {
    this.batchPercussionIntensity.set(val);
    if (val === 'Low' || val === 'Medium' || val === 'High') {
      this.dispatchBatchToSelected({ percussion_intensity: val as GrooveIntensity });
    }
  }

  protected onBatchBpmChange(val: string | number): void {
    const nextStr = String(val ?? '');
    this.batchBpm.set(nextStr);
    const parsed = parseBatchBpmInput(nextStr);
    if (parsed?.kind === 'reset') {
      this.dispatchBatchToSelected({ bpm_override: null });
    } else if (parsed?.kind === 'bpm') {
      this.dispatchBatchToSelected({ bpm_override: parsed.value });
    }
  }

  protected onBatchCrossfadeChange(val: string | number): void {
    const nextStr = String(val ?? '');
    this.batchCrossfadeSec.set(nextStr);
    const xf = parseBatchCrossfadeInput(nextStr);
    if (xf !== null) {
      this.dispatchBatchToSelected({ crossfade_sec: xf });
    }
  }

  protected async saveSetlistToFile(): Promise<void> {
    if (this.liveLock()) return;
    const manifest = serializeSetlistManifest(this.snap());
    const path = await this.stage.pickSaveSetlistFile(`${manifest.id}.json`);
    if (path) {
      await this.stage.saveSetlistToDisk(path, manifest);
    }
  }

  protected onLiveStageDrop(
    event: CdkDragDrop<SetlistBucket, SetlistBucket, SetlistItemSnapshot>,
  ): void {
    if (this.liveLock()) return;
    const item = event.item.data;
    const targetBucket = event.container.data;
    if (!item || !targetBucket) return;

    const { toSlot, sectionId } = computeCrossBucketMove(
      item.slot,
      targetBucket,
      event.currentIndex,
      this.snap().setlist_items,
    );
    const prevSectionId = item.section_id ?? null;
    if (sectionId !== prevSectionId) {
      void this.stage.sendCommand({
        type: 'UpdateSetlistSlotConfig',
        payload: { slot: item.slot, section_id: sectionId },
      });
    }
    if (toSlot !== item.slot) {
      void this.stage.sendCommand({
        type: 'MoveSetlistSlot',
        payload: { from: item.slot, to: toSlot },
      });
    }
    this.clearSelectedSlots();
  }

  protected isSlotSelected(slot: number): boolean {
    return this.selectedSetlistSlots().includes(slot);
  }

  protected onToggleSlotSelection(slot: number, mouseEvent?: MouseEvent): void {
    if (this.liveLock()) return;
    const orderedSlots = this.snap().setlist_items.map((it) => it.slot);
    const { nextSelected, nextAnchor } = toggleMultiSelectSlot(
      this.selectedSetlistSlots(),
      this.selectionAnchorSlot(),
      slot,
      Boolean(mouseEvent?.shiftKey),
      orderedSlots,
    );
    this.selectedSetlistSlots.set(nextSelected);
    this.selectionAnchorSlot.set(nextAnchor);
  }

  protected selectAllSetlistSlots(): void {
    if (this.liveLock()) return;
    this.selectedSetlistSlots.set(this.snap().setlist_items.map((it) => it.slot));
  }

  protected selectSectionSlots(sectionId: string | null): void {
    if (this.liveLock()) return;
    const matching = this.snap()
      .setlist_items.filter((it) => (it.section_id ?? null) === sectionId)
      .map((it) => it.slot);
    this.selectedSetlistSlots.set(matching);
  }

  protected clearSelectedSlots(): void {
    this.selectedSetlistSlots.set([]);
    this.selectionAnchorSlot.set(null);
  }

  protected applyBatchSetlistEdit(): void {
    if (this.liveLock()) return;
    const slots = this.selectedSetlistSlots().filter(
      (s) => s >= 0 && s < this.snap().setlist_items.length,
    );
    if (slots.length === 0) return;

    const patch: SetlistBatchPatch = { slots };
    let hasChange = false;

    const targetKey = this.batchTargetKey();
    if (targetKey) {
      patch.target_key = targetKey;
      hasChange = true;
    }
    const parsedBpm = parseBatchBpmInput(this.batchBpm());
    if (parsedBpm?.kind === 'reset') {
      patch.bpm_override = null;
      hasChange = true;
    } else if (parsedBpm?.kind === 'bpm') {
      patch.bpm_override = parsedBpm.value;
      hasChange = true;
    }
    const clickVal = this.batchClickMode();
    if (clickVal === 'reset') {
      patch.click_mode = null;
      hasChange = true;
    } else if (clickVal === 'AlwaysOn' || clickVal === 'IntroOnly' || clickVal === 'Off') {
      patch.click_mode = clickVal as ClickMode;
      hasChange = true;
    }
    const padVal = this.batchPadEnabled();
    if (padVal === 'on') {
      patch.ambient_pad_enabled = true;
      hasChange = true;
    } else if (padVal === 'off') {
      patch.ambient_pad_enabled = false;
      hasChange = true;
    }
    const transVal = this.batchTransitionMode();
    if (
      transVal === 'AutoPause' ||
      transVal === 'AutoAdvance' ||
      transVal === 'ContinueUntilStopped' ||
      transVal === 'Manual'
    ) {
      patch.transition_mode = transVal as TransitionMode;
      hasChange = true;
    }
    const parsedXf = parseBatchCrossfadeInput(this.batchCrossfadeSec());
    if (parsedXf !== null) {
      patch.crossfade_sec = parsedXf;
      hasChange = true;
    }
    const secVal = this.batchSectionId();
    if (secVal === '__none__') {
      patch.section_id = null;
      hasChange = true;
    } else if (secVal !== '') {
      patch.section_id = secVal;
      hasChange = true;
    }
    const pMode = this.batchPercussionMode();
    if (pMode === 'Off' || pMode === 'Layer' || pMode === 'PercussionOnly') {
      patch.percussion_mode = pMode as PercussionMode;
      hasChange = true;
    }
    const groovePatch = resolveBatchGroovePatch(
      this.batchPercussionStyle(),
      this.stage.percussionLoops(),
      this.snap().percussion_custom_pattern,
    );
    if (groovePatch) {
      Object.assign(patch, groovePatch);
      hasChange = true;
    }
    const pIntensity = this.batchPercussionIntensity();
    if (pIntensity === 'Low' || pIntensity === 'Medium' || pIntensity === 'High') {
      patch.percussion_intensity = pIntensity as GrooveIntensity;
      hasChange = true;
    }

    if (!hasChange) return;
    void this.stage.sendCommand({ type: 'BatchUpdateSetlistSlots', payload: patch });
  }

  protected onAddServiceSection(): void {
    if (this.liveLock()) return;
    const section = buildNewSection(
      this.addSectionKind(),
      this.snap().setlist_sections,
      this.addSectionName(),
    );
    void this.stage.sendCommand({ type: 'UpsertSetlistSection', payload: section });
    this.addSectionName.set('');
  }

  protected onToggleSectionCollapsed(sec: SetlistSection): void {
    void this.stage.sendCommand({
      type: 'ToggleSetlistSectionCollapsed',
      payload: { id: sec.id, collapsed: !sec.collapsed },
    });
  }

  protected onRemoveServiceSection(sectionId: string): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'RemoveSetlistSection', payload: sectionId });
  }

  protected onAssignSlotSection(slot: number, sectionId: string): void {
    if (this.liveLock()) return;
    const nextSection = sectionId || null;
    const batchSlots = resolveBatchTargetSlots(
      this.selectedSetlistSlots(),
      slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      void this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, section_id: nextSection },
      });
      return;
    }
    void this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: { slot, section_id: nextSection },
    });
  }

  protected onSetSlotCrossfadeSec(slot: number, secRaw: number | string): void {
    if (this.liveLock()) return;
    const val = Number(secRaw);
    if (!Number.isFinite(val)) return;
    const clamped = Math.max(0, Math.min(10, val));
    const batchSlots = resolveBatchTargetSlots(
      this.selectedSetlistSlots(),
      slot,
      this.snap().setlist_items.length,
    );
    if (batchSlots) {
      void this.stage.sendCommand({
        type: 'BatchUpdateSetlistSlots',
        payload: { slots: batchSlots, crossfade_sec: clamped },
      });
      return;
    }
    void this.stage.sendCommand({
      type: 'UpdateSetlistSlotConfig',
      payload: { slot, crossfade_sec: clamped },
    });
  }

  protected onAddStandaloneSlot(): void {
    if (this.liveLock()) return;
    const [bpbRaw, unitRaw] = this.newStandaloneTimeSig().split('/').map(Number);
    const timeSig: [number, number] = [
      Number.isFinite(bpbRaw) && bpbRaw > 0 ? bpbRaw : 4,
      Number.isFinite(unitRaw) && unitRaw > 0 ? unitRaw : 4,
    ];
    void this.stage.sendCommand({
      type: 'AddStandaloneSetlistSlot',
      payload: {
        title: this.newStandaloneTitle().trim() || 'Standalone Slot',
        slot_kind: this.newStandaloneKind(),
        key: this.newStandaloneKey() || 'C',
        bpm: Number(this.newStandaloneBpm()) || 72,
        time_signature: timeSig,
        section_id: this.newStandaloneSectionId() || null,
        insert_at: null,
      },
    });
    this.addSlotDrawerOpen.set(false);
  }

  // --- SPEC-019: Percussion Track & Groove Rack handlers ---

  protected onPercussionMode(mode: PercussionMode): void {
    if (this.stage.midiAssignMode()) {
      this.stage.startListeningForTarget(
        { type: 'TogglePercussionMode' },
        'Cycle Percussion Mode',
        'Momentary',
      );
      return;
    }
    void this.stage.sendCommand({ type: 'SetPercussionMode', payload: mode });
    this.dispatchBatchToSelected({ percussion_mode: mode });
  }

  protected onPercussionStyle(style: PercussionStyle): void {
    void this.stage.sendCommand({ type: 'SetPercussionStyle', payload: style });
    this.dispatchBatchToSelected({
      percussion_style: style,
      percussion_custom_pattern: null,
    });
  }

  protected activePercussionStyleSelectValue(): string {
    const custom = this.snap().percussion_custom_pattern;
    if (custom) {
      return `custom:${custom.id}`;
    }
    return `style:${this.snap().percussion_style}`;
  }

  protected unsavedCustomLoopPattern(): CustomPercussionPattern | null {
    const custom = this.snap().percussion_custom_pattern;
    if (!custom) return null;
    return this.stage.percussionLoops().some((l) => l.id === custom.id) ? null : custom;
  }

  protected onPercussionStyleOrLoopSelect(selected: string): void {
    if (selected.startsWith('custom:')) {
      const id = selected.slice('custom:'.length);
      const found =
        this.stage.percussionLoops().find((l) => l.id === id) ??
        this.snap().percussion_custom_pattern;
      if (!found) return;
      const copy: CustomPercussionPattern = {
        ...found,
        hits: found.hits.map((h) => ({ ...h })),
      };
      this.loopDraft.set(copy);
      void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: copy });
      if (this.snap().percussion_mode === 'Off') {
        void this.stage.sendCommand({ type: 'SetPercussionMode', payload: 'PercussionOnly' });
      }
      this.dispatchBatchToSelected({ percussion_custom_pattern: copy });
      return;
    }
    if (selected.startsWith('style:')) {
      this.onPercussionStyle(selected.slice('style:'.length) as PercussionStyle);
    }
  }

  protected onPercussionIntensity(intensity: GrooveIntensity): void {
    void this.stage.sendCommand({ type: 'SetPercussionIntensity', payload: intensity });
    this.dispatchBatchToSelected({ percussion_intensity: intensity });
  }

  protected onPercussionFollowSections(follow: boolean): void {
    void this.stage.sendCommand({ type: 'SetPercussionFollowSections', payload: follow });
  }

  protected onPercussionGainSlider(values: number[]): void {
    if (values.length > 0) {
      void this.stage.sendCommand({ type: 'SetPercussionGain', payload: values[0] });
    }
  }

  protected onPercussionIemSlider(values: number[]): void {
    if (values.length > 0) {
      void this.stage.sendCommand({ type: 'SetPercussionIemSend', payload: values[0] });
    }
  }

  protected onTogglePercussionInstrument(
    instrument: PercussionInstrument,
    currentlyEnabled: boolean,
  ): void {
    void this.stage.sendCommand({
      type: 'SetPercussionChannel',
      payload: { instrument, enabled: !currentlyEnabled },
    });
  }

  protected onPercussionChannelGainSlider(
    instrument: PercussionInstrument,
    values: number[],
  ): void {
    if (values.length > 0) {
      void this.stage.sendCommand({
        type: 'SetPercussionChannel',
        payload: { instrument, gain: values[0] },
      });
    }
  }

  protected onResetPercussionMix(): void {
    void this.stage.sendCommand({ type: 'ResetPercussionMix' });
  }

  protected onPercussionRoomSlider(values: number[]): void {
    if (values.length > 0) {
      void this.stage.sendCommand({ type: 'SetPercussionRoom', payload: values[0] });
    }
  }

  protected onSelectPercussionKitDir(dir: string): void {
    const clean = dir.trim();
    void this.stage.sendCommand({
      type: 'LoadPercussionKitDir',
      payload: clean.length > 0 ? clean : null,
    });
  }

  protected async onGenerateStudioKits(): Promise<void> {
    if (this.loopKitBusy()) return;
    this.loopKitBusy.set(true);
    try {
      const profiles: StudioKitProfile[] = ['StudioPraise', 'MakossaLive', 'AcousticCajon'];
      let preferredDir: string | null = null;
      for (const p of profiles) {
        const res = await this.stage.generateStudioPercussionKit(p);
        if (p === 'MakossaLive' && res) {
          preferredDir = res.dir;
        }
      }
      if (preferredDir) {
        await this.stage.sendCommand({ type: 'LoadPercussionKitDir', payload: preferredDir });
      }
    } finally {
      this.loopKitBusy.set(false);
    }
  }

  protected async onTogglePercussionStudio(): Promise<void> {
    const next = !this.percussionStudioOpen();
    this.percussionStudioOpen.set(next);
    if (next) {
      const curCustom = this.snap().percussion_custom_pattern;
      if (curCustom) {
        this.loopDraft.set({ ...curCustom, hits: curCustom.hits.map((h) => ({ ...h })) });
      } else {
        await this.onCloneStyleIntoEditor(false);
      }
    }
  }

  protected async onCloneStyleIntoEditor(applyLive = true): Promise<void> {
    const style = this.snap().percussion_style;
    const label =
      PERCUSSION_STYLE_OPTIONS.find((o) => o.value === style)?.label ?? String(style);
    const id = `custom-${style.toLowerCase()}`;
    const cloned = await this.stage.cloneStyleToCustomPattern(
      style,
      id,
      `Custom ${label}`,
    );
    this.loopDraft.set(cloned);
    if (applyLive) {
      await this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: cloned });
    }
  }

  protected onNewBlankLoop(stepsPerBar = 16, barsInCycle = 1): void {
    const blank: CustomPercussionPattern = {
      id: `custom-loop-${Date.now().toString().slice(-4)}`,
      name: 'My Custom Worship Loop',
      steps_per_bar: stepsPerBar,
      bars_in_cycle: barsInCycle,
      swing: 0,
      is_compound: stepsPerBar === 12 || stepsPerBar === 24,
      hits: [],
    };
    this.loopDraft.set(blank);
    void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: blank });
  }

  protected onUseBuiltInPresetPattern(): void {
    void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: null });
  }

  protected onSelectSavedLoop(loopId: string): void {
    if (!loopId) {
      this.onUseBuiltInPresetPattern();
      return;
    }
    const found = this.stage.percussionLoops().find((l) => l.id === loopId);
    if (!found) return;
    const copy: CustomPercussionPattern = {
      ...found,
      hits: found.hits.map((h) => ({ ...h })),
    };
    this.loopDraft.set(copy);
    void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: copy });
  }

  protected async onSaveCustomLoop(): Promise<void> {
    const cur = this.loopDraft();
    const cleanName = cur.name.trim() || 'Custom Worship Loop';
    const updated: CustomPercussionPattern = { ...cur, id: sanitizeLoopId(cleanName), name: cleanName };
    this.loopDraft.set(updated);
    await this.stage.savePercussionLoop(updated);
    await this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: updated });
  }

  protected async onDeleteCustomLoop(id: string): Promise<void> {
    await this.stage.deletePercussionLoop(id);
    if (this.snap().percussion_custom_pattern?.id === id) {
      await this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: null });
    }
  }

  protected onTogglePercussionAudition(): void {
    const s = this.snap();
    if (s.percussion_audition_playing) {
      void this.stage.sendCommand({ type: 'StopPercussionAudition' });
      return;
    }
    void this.stage.sendCommand({
      type: 'StartPercussionAudition',
      payload: {
        bpm: Math.round(s.current_bpm || 112),
        beats_per_bar: Math.max(1, s.beats_per_bar || 4),
        with_click: false,
      },
    });
  }

  protected onLoopDraftNameChange(name: string): void {
    this.loopDraft.update((d) => ({ ...d, name }));
  }

  protected onLoopDraftBarsChange(bars: number): void {
    this.loopDraft.update((d) => {
      const next = expandPatternBars(d, bars);
      void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: next });
      return next;
    });
  }

  protected onLoopDraftGridChange(stepsPerBar: number): void {
    this.loopDraft.update((d) => {
      const next = rescalePatternGrid(d, stepsPerBar);
      void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: next });
      return next;
    });
  }

  protected onLoopDraftSwingSlider(values: number[]): void {
    if (values.length === 0) return;
    const swing = Math.max(0, Math.min(0.35, values[0]));
    this.loopDraft.update((d) => {
      const next = { ...d, swing };
      void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: next });
      return next;
    });
  }

  protected loopStepIndices(): number[] {
    const d = this.loopDraft();
    const total = Math.max(4, Math.min(96, d.steps_per_bar * d.bars_in_cycle));
    return Array.from({ length: total }, (_, i) => i);
  }

  protected loopStepHit(
    instrument: PercussionInstrument,
    step: number,
  ): CustomPatternHit | undefined {
    return this.loopDraft().hits.find(
      (h) => h.instrument === instrument && h.step === step,
    );
  }

  protected onToggleLoopStep(instrument: PercussionInstrument, step: number): void {
    const paintIntensity = this.loopPaintIntensity();
    const paintVariant = this.loopPaintVariant();
    let added = false;
    this.loopDraft.update((d) => {
      const existing = d.hits.find(
        (h) => h.instrument === instrument && h.step === step,
      );
      let nextHits: CustomPatternHit[];
      if (
        existing &&
        existing.min_intensity === paintIntensity &&
        existing.variant === paintVariant
      ) {
        nextHits = d.hits.filter(
          (h) => !(h.instrument === instrument && h.step === step),
        );
      } else {
        added = true;
        const vel =
          paintVariant === 'Accent'
            ? 0.96
            : paintVariant === 'Soft'
              ? 0.62
              : 0.84;
        const newHit: CustomPatternHit = {
          step,
          instrument,
          velocity: vel,
          variant: paintVariant,
          min_intensity: paintIntensity,
        };
        nextHits = [
          ...d.hits.filter((h) => !(h.instrument === instrument && h.step === step)),
          newHit,
        ];
      }
      const next = { ...d, hits: nextHits };
      void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: next });
      return next;
    });
    if (added) {
      void this.stage.sendCommand({
        type: 'TriggerPercussionPreview',
        payload: {
          instrument,
          variant: paintVariant,
          velocity: paintVariant === 'Accent' ? 0.95 : 0.85,
        },
      });
    }
  }

  protected onAuditionInstrument(instrument: PercussionInstrument): void {
    const variant: HitVariant =
      instrument === 'HandDrums' ? 'Special' : 'Standard';
    void this.stage.sendCommand({
      type: 'TriggerPercussionPreview',
      payload: { instrument, variant, velocity: 0.9 },
    });
  }

  protected async onExportLoopBundle(): Promise<void> {
    if (this.loopExportBusy()) return;
    this.loopExportBusy.set(true);
    this.loopExportStatus.set(null);
    try {
      const s = this.snap();
      const useCustom = s.percussion_custom_pattern ?? null;
      const spb = useCustom?.steps_per_bar ?? (s.percussion_style === 'Woro' || s.percussion_style === 'Worship68' ? 12 : 16);
      const ts: [number, number] = spb === 12 || spb === 24 ? [6, 8] : [4, 4];
      const title =
        this.loopExportTitle().trim() || useCustom?.name || 'Sunday Percussion Loop';
      const res = await this.stage.exportPercussionLoopBundle({
        title,
        bpm: s.current_bpm,
        time_signature: ts,
        bars: this.loopExportBars(),
        dynamic_arrangement: this.loopExportDynamic(),
        intensity: s.percussion_intensity,
        room_mix: s.percussion_room_mix,
        style: s.percussion_style,
        custom_pattern: useCustom,
        kit_dir: s.percussion_kit_dir,
      });
      if (res) {
        this.loopExportStatus.set(
          `Saved "${res.title}" (${res.stem_files.length} isolated stems) to ${res.song_dir}`,
        );
      }
    } finally {
      this.loopExportBusy.set(false);
    }
  }

  protected onToggleActiveSlotPercussionLock(): void {
    const slot = this.snap().active_setlist_slot;
    if (slot === null) return;
    const item = this.activeSetlistItem();
    if (item?.percussion_lock) {
      void this.stage.sendCommand({ type: 'ClearSlotPercussionLock', payload: slot });
    } else {
      void this.stage.sendCommand({ type: 'LockCurrentPercussionToSlot', payload: slot });
    }
  }

  protected percussionInstrumentShortLabel(instrument: PercussionInstrument): string {
    return (
      PERCUSSION_INSTRUMENT_META.find((m) => m.instrument === instrument)?.shortLabel ?? instrument
    );
  }

  protected percussionInstrumentFullLabel(instrument: PercussionInstrument): string {
    return PERCUSSION_INSTRUMENT_META.find((m) => m.instrument === instrument)?.label ?? instrument;
  }

  protected async openSetlistFromFile(): Promise<void> {
    if (this.liveLock()) return;
    const path = await this.stage.pickOpenSetlistFile();
    if (!path) return;
    const setlist = await this.stage.loadSetlistFromDisk(path);
    if (setlist) {
      void this.stage.sendCommand({ type: 'LoadSetlist', payload: setlist });
    }
  }

  // --- Feature #2: Native Folder Picker & Song Setup Editor ---

  protected async browseAndLoadSong(): Promise<void> {
    if (this.liveLock()) return;
    const dir = await this.stage.pickSongFolder();
    if (!dir) return;
    await this.addOrSetupSongFolder(dir, false);
  }

  protected async loadFolder(): Promise<void> {
    const dir = this.customFolderPath().trim();
    if (!dir || this.liveLock()) return;
    await this.addOrSetupSongFolder(dir, false);
  }

  protected async openSongSetup(dirOverride?: string | null): Promise<void> {
    if (this.liveLock()) return;
    let dir = dirOverride ?? this.snap().active_song_dir ?? this.customFolderPath().trim();
    if (!dir) {
      const picked = await this.stage.pickSongFolder();
      if (!picked) return;
      dir = picked;
    }
    const inspected = await this.stage.inspectSongFolder(dir);
    if (inspected) {
      this.openEditorWithInspected(inspected, false);
    }
  }

  private openEditorWithInspected(
    inspected: NonNullable<Awaited<ReturnType<PsalmStageService['inspectSongFolder']>>>,
    addToSetlistAfterSave: boolean,
  ): void {
    this.songEditorDir.set(inspected.dir);
    this.songEditorHasManifest.set(inspected.has_existing_manifest);
    this.songEditorDurationSec.set(inspected.estimated_duration_sec);
    this.songEditorAvailableFiles.set(inspected.available_files);
    this.songEditorManifest.set(structuredClone(inspected.manifest));
    this.songEditorAddToSetlist.set(addToSetlistAfterSave);
    this.songEditorError.set(null);
    this.songEditorOpen.set(true);
  }

  protected closeSongEditor(): void {
    this.songEditorOpen.set(false);
    this.songEditorError.set(null);
  }

  protected updateEditorField(
    field: 'title' | 'artist' | 'bpm' | 'first_downbeat_sec' | 'original_key' | 'ts_num' | 'ts_den',
    rawValue: string | number,
  ): void {
    this.songEditorManifest.update((m) => {
      if (!m) return m;
      if (field === 'title') {
        const title = String(rawValue);
        const slug = title.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '');
        return { ...m, title, id: slug || m.id };
      }
      if (field === 'artist') return { ...m, artist: String(rawValue) };
      if (field === 'bpm') return { ...m, bpm: Number(rawValue) || 120 };
      if (field === 'first_downbeat_sec') {
        return { ...m, first_downbeat_sec: Math.max(0, Number(rawValue) || 0) };
      }
      if (field === 'original_key') return { ...m, original_key: String(rawValue) };
      if (field === 'ts_num') {
        return { ...m, time_signature: [Math.max(1, Number(rawValue) || 4), m.time_signature[1]] };
      }
      if (field === 'ts_den') {
        return { ...m, time_signature: [m.time_signature[0], Math.max(1, Number(rawValue) || 4)] };
      }
      return m;
    });
  }

  protected nudgeEditorBeat(beats: number): void {
    this.songEditorManifest.update((m) => {
      if (!m) return m;
      const spb = 60 / Math.max(30, m.bpm || 120);
      const bpb = Math.max(1, m.time_signature[0] || 4);
      const cur = m.first_downbeat_sec ?? 0;
      let next = cur + beats * spb;
      if (next < -0.002 && beats < 0) {
        next = cur + (bpb + beats) * spb;
      }
      return { ...m, first_downbeat_sec: Math.max(0, Math.round(next * 1000) / 1000) };
    });
  }

  protected updateEditorStem(
    stemKey: string,
    patch: Partial<StemDefDetailed>,
  ): void {
    this.songEditorManifest.update((m) => {
      if (!m) return m;
      const existing = m.stems[stemKey];
      const base: StemDefDetailed =
        typeof existing === 'string'
          ? { file: existing, pitched: true, is_cues: false, default_gain: 1.0 }
          : { ...(existing ?? { file: '', pitched: true, is_cues: false, default_gain: 1.0 }) };
      return {
        ...m,
        stems: {
          ...m.stems,
          [stemKey]: { ...base, ...patch },
        },
      };
    });
  }

  protected addEditorSection(): void {
    this.songEditorManifest.update((m) => {
      if (!m) return m;
      const lastEnd = m.sections.length > 0 ? m.sections[m.sections.length - 1].end_bar : 1;
      const num = m.sections.length + 1;
      return {
        ...m,
        sections: [
          ...m.sections,
          {
            id: `section-${num}-${Date.now().toString().slice(-3)}`,
            name: `Section ${num}`,
            start_bar: lastEnd,
            end_bar: lastEnd + 8,
            color: 'sky',
          },
        ],
      };
    });
  }

  protected removeEditorSection(index: number): void {
    this.songEditorManifest.update((m) => {
      if (!m || m.sections.length <= 1) return m;
      return { ...m, sections: m.sections.filter((_, i) => i !== index) };
    });
  }

  protected updateEditorSection(
    index: number,
    patch: Partial<{ name: string; start_bar: number; end_bar: number }>,
  ): void {
    this.songEditorManifest.update((m) => {
      if (!m) return m;
      return {
        ...m,
        sections: m.sections.map((sec, i) => {
          if (i !== index) return sec;
          const nextName = patch.name !== undefined ? patch.name : sec.name;
          return {
            ...sec,
            ...patch,
            id:
              patch.name !== undefined
                ? nextName.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || sec.id
                : sec.id,
          };
        }),
      };
    });
  }

  protected validateSongManifest(manifest: SongManifest): string | null {
    if (!manifest.title.trim()) return 'Song title is required.';
    if (manifest.bpm < 20 || manifest.bpm > 280) return 'BPM must be between 20 and 280.';
    if (manifest.sections.length === 0) return 'At least one section is required.';
    for (const sec of manifest.sections) {
      if (!sec.name.trim()) return 'All sections must have a name.';
      if (sec.start_bar < 1 || sec.end_bar <= sec.start_bar) {
        return `Section "${sec.name}" must have End Bar > Start Bar (1-based).`;
      }
    }
    if (Object.keys(manifest.stems).length === 0) return 'At least one stem is required.';
    return null;
  }

  protected async saveSongEditorManifest(): Promise<void> {
    const manifest = this.songEditorManifest();
    const dir = this.songEditorDir();
    if (!manifest || !dir) return;

    const validationError = this.validateSongManifest(manifest);
    if (validationError) {
      this.songEditorError.set(validationError);
      return;
    }

    const saved = await this.stage.saveSongManifest(dir, manifest, true);
    if (!saved) {
      this.songEditorError.set(this.stage.statusMessage() ?? 'Failed to save manifest.json');
      return;
    }

    if (this.songEditorAddToSetlist()) {
      void this.stage.sendCommand({ type: 'AddSetlistSong', payload: { dir, insert_at: null } });
    } else {
      this.customFolderPath.set(dir);
    }
    this.songEditorOpen.set(false);
  }

  protected loadDemo(): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'LoadDemoSong' });
  }

  protected connectSelectedMidi(): void {
    void this.stage.connectMidiPort(Number(this.selectedMidiPortIndex()));
  }

  protected formatMidiEvent(raw: RawMidiEvent | null): string {
    return this.stage.formatMidiEvent(raw);
  }

  protected onDownloadUpdate(): void {
    const update = this.stage.availableUpdate();
    if (update) {
      void this.stage.openInBrowser(update.url);
    }
  }

  protected onDismissUpdate(): void {
    this.stage.dismissUpdate();
  }

  protected onCheckUpdatesManually(): void {
    void this.stage.checkForUpdates();
  }

  protected formatTarget(target: MidiTarget): string {
    switch (target.type) {
      case 'PlayStop':
        return 'Play / Stop';
      case 'LoopToggle':
        return 'Loop Toggle';
      case 'SectionByName':
        return `Section: ${target.value}`;
      case 'SectionBySlot':
        return `Section Slot #${target.value + 1}`;
      case 'KeyUp':
        return 'Key +1';
      case 'KeyDown':
        return 'Key -1';
      case 'TempoUp':
        return 'Tempo +1';
      case 'TempoDown':
        return 'Tempo -1';
      case 'NextSong':
        return 'Next Setlist Song';
      case 'PrevSong':
        return 'Prev Setlist Song';
      case 'TogglePad':
        return 'Toggle Ambient Pad';
      case 'ToggleClick':
        return 'Mute / Unmute Metronome';
      case 'PadGain':
        return 'Ambient Pad Volume';
      case 'PadBrightness':
        return 'Pad Brightness';
      case 'PadShimmer':
        return 'Pad Shimmer';
      case 'PadCrossfade':
        return 'Pad Crossfade Time';
      case 'PadPresetNext':
        return 'Next Pad Preset';
      case 'PadPresetPrev':
        return 'Prev Pad Preset';
      case 'PadKeyNext':
        return 'Pad Key +1';
      case 'PadKeyPrev':
        return 'Pad Key -1';
      case 'MasterGain':
        return 'Master Volume';
      case 'ClickGain':
        return 'Click Volume';
      case 'CuesGain':
        return 'Cues Volume';
      case 'StemGain':
        return `Stem: ${target.value}`;
      case 'TogglePercussionMode':
        return 'Cycle Percussion Mode';
      case 'PercussionGain':
        return 'Percussion Groove Volume';
      case 'PercussionIntensityUp':
        return 'Percussion Intensity +';
      case 'PercussionIntensityDown':
        return 'Percussion Intensity -';
      default:
        return target.type;
    }
  }

  protected formatTimePrecise(sec: number): string {
    if (!Number.isFinite(sec) || sec < 0) return '0:00.00';
    const mins = Math.floor(sec / 60);
    const rem = sec % 60;
    return `${mins}:${rem.toFixed(2).padStart(5, '0')}`;
  }
}
