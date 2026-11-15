import { DecimalPipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  EventEmitter,
  Output,
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
  type CustomPatternHit,
  type CustomPercussionPattern,
  type GrooveIntensity,
  type HitVariant,
  type PercussionInstrument,
  type PercussionStyle,
  type StudioKitProfile,
} from '../../models/engine.models';
import {
  applyBarGrooveQuickFill,
  applyTurnaroundFillToBar,
  clearBarHits,
  cloneCustomPattern,
  copyBarHits,
  copyBarOneToAllBars,
  expandPatternBars,
  normalizePattern,
  rescalePatternGrid,
  sanitizeLoopId,
  type BarQuickPreset,
} from '../../models/loop-pattern.helpers';
import { PsalmStageService } from '../../services/psalm-stage.service';

export type BarViewScope = 'all' | 0 | 1 | 2 | 3;

@Component({
  selector: 'app-loop-studio',
  standalone: true,
  imports: [
    DecimalPipe,
    FormsModule,
    HlmButtonImports,
    HlmBadgeImports,
    HlmCardImports,
    HlmSliderImports,
    HlmSwitchImports,
  ],
  templateUrl: './loop-studio.component.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class LoopStudioComponent {
  @Output() readonly goToStage = new EventEmitter<void>();

  protected readonly stage = inject(PsalmStageService);
  protected readonly snap = this.stage.snapshot;

  protected readonly instrumentMeta = PERCUSSION_INSTRUMENT_META;
  protected readonly styleOptions = PERCUSSION_STYLE_OPTIONS;

  protected readonly draft = signal<CustomPercussionPattern>({
    id: 'sunday-multi-bar-groove',
    name: 'Sunday Multi-Bar Groove',
    steps_per_bar: 16,
    bars_in_cycle: 2,
    swing: 0.04,
    is_compound: false,
    hits: [],
  });

  protected readonly barScope = signal<BarViewScope>(0);
  protected readonly followSetlistBpm = signal<boolean>(true);
  protected readonly auditionBpm = signal<number>(120);
  protected readonly auditionClick = signal<boolean>(false);
  protected readonly selectedStylePreset = signal<PercussionStyle>('Afrobeat');
  protected readonly paintIntensity = signal<GrooveIntensity>('Low');
  protected readonly paintVariant = signal<HitVariant>('Standard');
  protected readonly paintVelocity = signal<number>(0.86);

  protected readonly soloedInstruments = signal<PercussionInstrument[]>([]);
  private preSoloEnabled: Partial<Record<PercussionInstrument, boolean>> | null = null;

  protected readonly kitBusy = signal<boolean>(false);
  protected readonly saveStatus = signal<string | null>(null);
  protected readonly exportTitle = signal<string>('Sunday Worship Groove');
  protected readonly exportBars = signal<number>(16);
  protected readonly exportDynamic = signal<boolean>(true);
  protected readonly exportBusy = signal<boolean>(false);
  protected readonly exportStatus = signal<string | null>(null);

  private tapTimestamps: number[] = [];
  private initializedFromEngine = false;

  constructor() {
    effect(() => {
      const s = this.snap();
      if (!this.initializedFromEngine) {
        this.initializedFromEngine = true;
        this.selectedStylePreset.set(s.percussion_style);
        if (s.percussion_custom_pattern) {
          this.draft.set(cloneCustomPattern(s.percussion_custom_pattern));
        } else {
          void this.onLoadStylePreset(s.percussion_style, 2, false);
        }
      }
      if (this.followSetlistBpm()) {
        const setlistBpm = Math.max(40, Math.min(240, Math.round(s.current_bpm || 120)));
        if (this.auditionBpm() !== setlistBpm) {
          this.auditionBpm.set(setlistBpm);
        }
      }
    });
  }

  protected readonly visibleBars = computed<number[]>(() => {
    const bars = this.draft().bars_in_cycle;
    const scope = this.barScope();
    if (scope === 'all') {
      return Array.from({ length: bars }, (_, i) => i);
    }
    return [Math.min(scope, Math.max(0, bars - 1))];
  });

  protected readonly barIndices = computed<number[]>(() =>
    Array.from({ length: this.draft().bars_in_cycle }, (_, i) => i),
  );

  protected readonly activeFocusedBar = computed<number>(() => {
    const scope = this.barScope();
    if (scope === 'all') return 0;
    return Math.min(scope, Math.max(0, this.draft().bars_in_cycle - 1));
  });

  protected readonly playingBarIndex = computed<number | null>(() => {
    const step = this.snap().percussion_current_step;
    if (step === null) return null;
    return Math.floor(step / Math.max(1, this.draft().steps_per_bar));
  });

  protected readonly visibleStepColumns = computed<{ bar: number; localStep: number; globalStep: number; beatStart: boolean; barStart: boolean }[]>(() => {
    const d = this.draft();
    const spb = d.steps_per_bar;
    const beatSubdiv = spb === 12 ? 3 : spb === 24 ? 6 : 4;
    const cols: {
      bar: number;
      localStep: number;
      globalStep: number;
      beatStart: boolean;
      barStart: boolean;
    }[] = [];
    for (const bar of this.visibleBars()) {
      for (let local = 0; local < spb; local++) {
        cols.push({
          bar,
          localStep: local,
          globalStep: bar * spb + local,
          beatStart: local % beatSubdiv === 0,
          barStart: local === 0,
        });
      }
    }
    return cols;
  });

  protected readonly totalHitsCount = computed<number>(() => this.draft().hits.length);

  protected readonly hitLookup = computed<Map<string, CustomPatternHit>>(() => {
    const map = new Map<string, CustomPatternHit>();
    for (const h of this.draft().hits) {
      map.set(`${h.instrument}:${h.step}`, h);
    }
    return map;
  });

  protected getHit(instrument: PercussionInstrument, step: number): CustomPatternHit | undefined {
    return this.hitLookup().get(`${instrument}:${step}`);
  }

  protected getChannelConfig(instrument: PercussionInstrument) {
    return (
      this.snap().percussion_channels.find((c) => c.instrument === instrument) ?? {
        instrument,
        enabled: true,
        gain: 0.85,
        pan: 0,
      }
    );
  }

  protected isSoloed(instrument: PercussionInstrument): boolean {
    return this.soloedInstruments().includes(instrument);
  }

  protected onToggleAuditionPlay(): void {
    const s = this.snap();
    if (s.percussion_audition_playing) {
      void this.stage.sendCommand({ type: 'StopPercussionAudition' });
      return;
    }
    // Ensure custom draft is pushed to the engine before auditioning
    const pat = normalizePattern(this.draft());
    this.draft.set(pat);
    void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: pat });
    const bpm = this.followSetlistBpm()
      ? Math.max(40, Math.min(240, s.current_bpm || this.auditionBpm()))
      : this.auditionBpm();
    const beatsPerBar = Math.max(1, s.beats_per_bar || 4);
    void this.stage.sendCommand({
      type: 'StartPercussionAudition',
      payload: {
        bpm,
        beats_per_bar: beatsPerBar,
        with_click: this.auditionClick(),
      },
    });
  }

  protected onSyncToSetlistBpm(): void {
    this.followSetlistBpm.set(true);
    const setlistBpm = Math.max(40, Math.min(240, Math.round(this.snap().current_bpm || 120)));
    this.auditionBpm.set(setlistBpm);
    if (this.snap().percussion_audition_playing) {
      void this.stage.sendCommand({ type: 'SetPercussionAuditionBpm', payload: setlistBpm });
    }
  }

  protected onBpmChange(nextBpm: number): void {
    const clamped = Math.max(40, Math.min(240, Math.round(Number(nextBpm) || 120)));
    const stageBpm = Math.max(40, Math.min(240, Math.round(this.snap().current_bpm || 120)));
    this.followSetlistBpm.set(clamped === stageBpm);
    this.auditionBpm.set(clamped);
    if (this.snap().percussion_audition_playing) {
      void this.stage.sendCommand({ type: 'SetPercussionAuditionBpm', payload: clamped });
    }
  }

  protected onToggleAuditionClick(checked: boolean): void {
    this.auditionClick.set(checked);
    if (this.snap().percussion_audition_playing) {
      void this.stage.sendCommand({
        type: 'StartPercussionAudition',
        payload: {
          bpm: this.auditionBpm(),
          beats_per_bar: Math.max(1, this.snap().beats_per_bar || 4),
          with_click: checked,
        },
      });
    }
  }

  protected onTapTempo(): void {
    const now = performance.now();
    if (this.tapTimestamps.length > 0 && now - this.tapTimestamps[this.tapTimestamps.length - 1] > 2000) {
      this.tapTimestamps = [];
    }
    this.tapTimestamps.push(now);
    if (this.tapTimestamps.length > 6) {
      this.tapTimestamps.shift();
    }
    if (this.tapTimestamps.length >= 2) {
      const diffs: number[] = [];
      for (let i = 1; i < this.tapTimestamps.length; i++) {
        diffs.push(this.tapTimestamps[i] - this.tapTimestamps[i - 1]);
      }
      const avgMs = diffs.reduce((a, b) => a + b, 0) / diffs.length;
      if (avgMs > 200 && avgMs < 1500) {
        this.onBpmChange(Math.round(60000 / avgMs));
      }
    }
  }

  protected onSetBarsInCycle(targetBars: 1 | 2 | 4): void {
    const next = expandPatternBars(this.draft(), targetBars);
    this.applyDraft(next);
    const scope = this.barScope();
    if (scope !== 'all' && scope >= targetBars) {
      this.barScope.set(0);
    }
  }

  protected onSetSubdivision(stepsPerBar: number): void {
    const next = rescalePatternGrid(this.draft(), stepsPerBar);
    this.applyDraft(next);
  }

  protected onSetIntensity(level: GrooveIntensity): void {
    void this.stage.sendCommand({ type: 'SetPercussionIntensity', payload: level });
  }

  protected onSwingSlider(values: number[]): void {
    if (values.length === 0) return;
    const next = normalizePattern({ ...this.draft(), swing: values[0] });
    this.applyDraft(next);
  }

  protected onRoomSlider(values: number[]): void {
    if (values.length === 0) return;
    void this.stage.sendCommand({ type: 'SetPercussionRoom', payload: values[0] });
  }

  protected onMasterGainSlider(values: number[]): void {
    if (values.length === 0) return;
    void this.stage.sendCommand({ type: 'SetPercussionGain', payload: values[0] });
  }

  protected onSelectKitDir(dir: string): void {
    const clean = dir.trim();
    void this.stage.sendCommand({
      type: 'LoadPercussionKitDir',
      payload: clean.length > 0 ? clean : null,
    });
  }

  protected async onGenerateStudioKits(): Promise<void> {
    if (this.kitBusy()) return;
    this.kitBusy.set(true);
    try {
      const profiles: StudioKitProfile[] = ['StudioPraise', 'MakossaLive', 'AcousticCajon'];
      let preferredDir: string | null = null;
      for (const p of profiles) {
        const res = await this.stage.generateStudioPercussionKit(p);
        if (p === 'MakossaLive' && res) preferredDir = res.dir;
      }
      if (preferredDir) {
        await this.stage.sendCommand({ type: 'LoadPercussionKitDir', payload: preferredDir });
      }
    } finally {
      this.kitBusy.set(false);
    }
  }

  protected async onLoadStylePreset(
    style: PercussionStyle,
    targetBars?: 1 | 2 | 4,
    applyChannelStyle = true,
  ): Promise<void> {
    this.selectedStylePreset.set(style);
    if (applyChannelStyle) {
      void this.stage.sendCommand({ type: 'SetPercussionStyle', payload: style });
    }
    const label = PERCUSSION_STYLE_OPTIONS.find((o) => o.value === style)?.label ?? String(style);
    const bars = targetBars ?? (Math.max(1, Math.min(4, this.draft().bars_in_cycle)) as 1 | 2 | 4);
    const cloned = await this.stage.cloneStyleToCustomPattern(
      style,
      `custom-${style.toLowerCase()}-${bars}bar`,
      `${label} (${bars}B)`,
    );
    const expanded = expandPatternBars(cloned, bars);
    this.applyDraft(expanded);
  }

  protected onNewBlankPattern(bars: 1 | 2 | 4 = 2): void {
    const cur = this.draft();
    const blank: CustomPercussionPattern = {
      id: `worship-loop-${Date.now().toString().slice(-4)}`,
      name: `Custom ${bars}-Bar Groove`,
      steps_per_bar: cur.steps_per_bar,
      bars_in_cycle: bars,
      swing: cur.swing,
      is_compound: cur.is_compound,
      hits: [],
    };
    this.barScope.set(0);
    this.applyDraft(blank);
  }

  protected onLoadSavedLoop(loopId: string): void {
    const found = this.stage.percussionLoops().find((l) => l.id === loopId);
    if (!found) return;
    this.barScope.set(0);
    this.exportTitle.set(found.name);
    this.applyDraft(cloneCustomPattern(found));
  }

  protected async onSaveLoop(): Promise<void> {
    const cur = this.draft();
    const cleanName = cur.name.trim() || 'Custom Worship Loop';
    const next = normalizePattern({
      ...cur,
      id: sanitizeLoopId(cleanName),
      name: cleanName,
    });
    this.draft.set(next);
    this.exportTitle.set(cleanName);
    await this.stage.savePercussionLoop(next);
    await this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: next });
    this.saveStatus.set(
      `Saved "${next.name}" (${next.bars_in_cycle}B) — now selectable under Style on Live Stage & Setlist Studio`,
    );
  }

  protected async onDeleteLoop(loopId: string): Promise<void> {
    await this.stage.deletePercussionLoop(loopId);
    if (this.snap().percussion_custom_pattern?.id === loopId) {
      await this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: null });
    }
    this.saveStatus.set('Deleted custom loop');
  }

  protected onDraftNameChange(name: string): void {
    this.draft.update((d) => ({ ...d, name }));
  }

  protected onCopyBar1ToAll(): void {
    this.applyDraft(copyBarOneToAllBars(this.draft()));
  }

  protected onCopyPrevBar(): void {
    const target = this.activeFocusedBar();
    if (target <= 0) return;
    this.applyDraft(copyBarHits(this.draft(), target - 1, target));
  }

  protected onAddTurnaroundFill(): void {
    const target = this.activeFocusedBar();
    this.applyDraft(applyTurnaroundFillToBar(this.draft(), target));
  }

  protected onClearFocusedBar(): void {
    const target = this.activeFocusedBar();
    this.applyDraft(clearBarHits(this.draft(), target));
  }

  protected onQuickBarLayer(preset: BarQuickPreset): void {
    const target = this.activeFocusedBar();
    this.applyDraft(applyBarGrooveQuickFill(this.draft(), target, preset));
  }

  protected onToggleStep(instrument: PercussionInstrument, step: number): void {
    const paintIntensity = this.paintIntensity();
    const paintVariant = this.paintVariant();
    const paintVel = this.paintVelocity();
    let added = false;

    const cur = this.draft();
    const existing = cur.hits.find((h) => h.instrument === instrument && h.step === step);
    let nextHits: CustomPatternHit[];
    if (
      existing &&
      existing.min_intensity === paintIntensity &&
      existing.variant === paintVariant
    ) {
      nextHits = cur.hits.filter((h) => !(h.instrument === instrument && h.step === step));
    } else {
      added = true;
      const newHit: CustomPatternHit = {
        step,
        instrument,
        velocity:
          paintVariant === 'Soft'
            ? Math.min(0.68, paintVel)
            : paintVariant === 'Accent'
              ? Math.max(0.92, paintVel)
              : paintVel,
        variant: paintVariant,
        min_intensity: paintIntensity,
      };
      nextHits = [
        ...cur.hits.filter((h) => !(h.instrument === instrument && h.step === step)),
        newHit,
      ];
    }

    this.applyDraft(normalizePattern({ ...cur, hits: nextHits }));

    if (added) {
      void this.stage.sendCommand({
        type: 'TriggerPercussionPreview',
        payload: {
          instrument,
          variant: paintVariant,
          velocity: paintVel,
        },
      });
    }
  }

  protected onAuditionInstrument(instrument: PercussionInstrument): void {
    void this.stage.sendCommand({
      type: 'TriggerPercussionPreview',
      payload: {
        instrument,
        variant: instrument === 'HandDrums' ? 'Special' : 'Standard',
        velocity: 0.9,
      },
    });
  }

  protected onToggleMute(instrument: PercussionInstrument): void {
    const ch = this.getChannelConfig(instrument);
    void this.stage.sendCommand({
      type: 'SetPercussionChannel',
      payload: { instrument, enabled: !ch.enabled },
    });
  }

  protected onToggleSolo(instrument: PercussionInstrument): void {
    const current = this.soloedInstruments();
    const channels = this.snap().percussion_channels;

    if (current.length === 0) {
      const saved: Partial<Record<PercussionInstrument, boolean>> = {};
      for (const ch of channels) saved[ch.instrument] = ch.enabled;
      this.preSoloEnabled = saved;
    }

    const next = current.includes(instrument)
      ? current.filter((x) => x !== instrument)
      : [...current, instrument];
    this.soloedInstruments.set(next);

    if (next.length === 0) {
      const restore = this.preSoloEnabled;
      this.preSoloEnabled = null;
      for (const meta of this.instrumentMeta) {
        void this.stage.sendCommand({
          type: 'SetPercussionChannel',
          payload: {
            instrument: meta.instrument,
            enabled: restore?.[meta.instrument] ?? true,
          },
        });
      }
      return;
    }

    for (const meta of this.instrumentMeta) {
      void this.stage.sendCommand({
        type: 'SetPercussionChannel',
        payload: {
          instrument: meta.instrument,
          enabled: next.includes(meta.instrument),
        },
      });
    }
  }

  protected onChannelGainSlider(instrument: PercussionInstrument, values: number[]): void {
    if (values.length === 0) return;
    void this.stage.sendCommand({
      type: 'SetPercussionChannel',
      payload: { instrument, gain: values[0] },
    });
  }

  protected onResetMix(): void {
    this.soloedInstruments.set([]);
    this.preSoloEnabled = null;
    void this.stage.sendCommand({ type: 'ResetPercussionMix' });
  }

  protected async onUseOnLiveStage(): Promise<void> {
    const pat = normalizePattern(this.draft());
    this.draft.set(pat);
    const cmds = [
      this.stage.sendCommand({ type: 'StopPercussionAudition' }),
      this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: pat }),
    ];
    if (this.snap().percussion_mode === 'Off') {
      cmds.push(this.stage.sendCommand({ type: 'SetPercussionMode', payload: 'PercussionOnly' }));
    }
    this.goToStage.emit();
    await Promise.all(cmds);
  }

  protected async onExportSongBundle(): Promise<void> {
    if (this.exportBusy()) return;
    this.exportBusy.set(true);
    this.exportStatus.set(null);
    try {
      const s = this.snap();
      const pat = normalizePattern(this.draft());
      const ts: [number, number] = pat.is_compound ? [6, 8] : [4, 4];
      const title = this.exportTitle().trim() || pat.name || 'Sunday Percussion Groove';
      const res = await this.stage.exportPercussionLoopBundle({
        title,
        bpm: this.auditionBpm(),
        time_signature: ts,
        bars: this.exportBars(),
        dynamic_arrangement: this.exportDynamic(),
        intensity: s.percussion_intensity,
        room_mix: s.percussion_room_mix,
        style: s.percussion_style,
        custom_pattern: pat,
        kit_dir: s.percussion_kit_dir,
      });
      if (res) {
        this.exportStatus.set(
          `Exported "${res.title}" (${res.stem_files.length} isolated WAV stems) to ${res.song_dir}`,
        );
      }
    } finally {
      this.exportBusy.set(false);
    }
  }

  protected stepPadClasses(
    hit: CustomPatternHit | undefined,
    isCurrentPlayhead: boolean,
    beatStart: boolean,
  ): string {
    const borderClass = beatStart ? 'border-l-2 border-l-zinc-500/80 ' : '';
    const ringClass = isCurrentPlayhead ? 'ring-2 ring-white/90 z-10 ' : '';

    if (!hit) {
      return `${borderClass}${ringClass}border border-zinc-800/90 bg-zinc-950/90 hover:bg-zinc-800/80 hover:border-zinc-600`;
    }

    if (hit.variant === 'Accent') {
      return `${borderClass}${ringClass}border border-amber-300 bg-amber-500/90 text-zinc-950 shadow-sm`;
    }
    if (hit.variant === 'Special') {
      return `${borderClass}${ringClass}border border-cyan-300 bg-cyan-500/85 text-zinc-950 shadow-sm`;
    }
    if (hit.variant === 'Soft') {
      return `${borderClass}${ringClass}border border-emerald-500/50 bg-emerald-500/35 text-emerald-100`;
    }
    if (hit.min_intensity === 'High') {
      return `${borderClass}${ringClass}border border-rose-400 bg-rose-500/75 text-white`;
    }
    if (hit.min_intensity === 'Medium') {
      return `${borderClass}${ringClass}border border-sky-400 bg-sky-500/75 text-zinc-950`;
    }
    return `${borderClass}${ringClass}border border-emerald-400 bg-emerald-500/80 text-zinc-950`;
  }

  private applyDraft(next: CustomPercussionPattern): void {
    const normalized = normalizePattern(next);
    this.draft.set(normalized);
    void this.stage.sendCommand({ type: 'SetCustomPercussionPattern', payload: normalized });
  }
}
