import { TestBed } from '@angular/core/testing';
import { App, UI_SESSION_STORAGE_KEY } from './app';
import { ImportWorkbenchComponent } from './components/import-workbench/import-workbench.component';
import { serializeSetlistManifest } from './models/setlist.helpers';
import {
  SetlistStudioComponent,
  computeHarmonicTransition,
} from './components/setlist-studio/setlist-studio.component';
import {
  PREVIEW_SESSION_STORAGE_KEY,
  PsalmStageService,
  transposeKeyName,
} from './services/psalm-stage.service';

describe('App', () => {
  beforeAll(() => {
    if (typeof globalThis.ResizeObserver === 'undefined') {
      globalThis.ResizeObserver = class ResizeObserver {
        observe(): void {}
        unobserve(): void {}
        disconnect(): void {}
      };
    }
  });

  beforeEach(async () => {
    window.localStorage.clear();
    await TestBed.configureTestingModule({
      imports: [App],
    }).compileComponents();
  });

  it('should create the app', () => {
    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance;
    expect(app).toBeTruthy();
  });

  it('should render title', async () => {
    const fixture = TestBed.createComponent(App);
    await fixture.whenStable();
    const compiled = fixture.nativeElement as HTMLElement;
    expect(compiled.querySelector('h1')?.textContent).toContain('PSALM STAGE');
  });

  it('should initialize with default demo praise snapshot', () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const snap = service.snapshot();

    expect(snap.song_id).toBe('demo-praise');
    expect(snap.base_bpm).toBe(72);
    expect(snap.current_bpm).toBe(72);
    expect(snap.original_key).toBe('G');
    expect(snap.is_playing).toBe(false);
    expect(snap.loop_enabled).toBe(false);
    expect(snap.sections.length).toBeGreaterThanOrEqual(4);
  });

  it('should toggle play and loop via service commands', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    expect(service.snapshot().is_playing).toBe(false);
    await service.sendCommand({ type: 'PlayStop' });
    expect(service.snapshot().is_playing).toBe(true);
    await service.sendCommand({ type: 'PlayStop' });
    expect(service.snapshot().is_playing).toBe(false);

    expect(service.snapshot().loop_enabled).toBe(false);
    await service.sendCommand({ type: 'ToggleLoop' });
    expect(service.snapshot().loop_enabled).toBe(true);
    await service.sendCommand({ type: 'ToggleLoop' });
    expect(service.snapshot().loop_enabled).toBe(false);
  });

  it('should adjust key shift and BPM within bounds', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    await service.sendCommand({ type: 'AdjustKeyShift', payload: 2 });
    expect(service.snapshot().key_shift_semitones).toBe(2);

    await service.sendCommand({ type: 'AdjustKeyShift', payload: -1 });
    expect(service.snapshot().key_shift_semitones).toBe(1);

    const baseBpm = service.snapshot().current_bpm;
    await service.sendCommand({ type: 'AdjustBpm', payload: 4 });
    expect(service.snapshot().current_bpm).toBe(baseBpm + 4);
  });

  it('should change quantize mode between NextBar and EndOfSection', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    expect(service.snapshot().quantize_mode).toBe('NextBar');
    await service.sendCommand({ type: 'SetQuantizeMode', payload: 'EndOfSection' });
    expect(service.snapshot().quantize_mode).toBe('EndOfSection');
  });

  it('should handle global keyboard shortcuts for Space and L', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    // Initial state: not playing, loop disabled
    expect(service.snapshot().is_playing).toBe(false);

    // Trigger Spacebar keydown
    window.dispatchEvent(new KeyboardEvent('keydown', { code: 'Space', bubbles: true }));
    expect(service.snapshot().is_playing).toBe(true);

    // Trigger L keydown for Loop
    expect(service.snapshot().loop_enabled).toBe(false);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'l', bubbles: true }));
    expect(service.snapshot().loop_enabled).toBe(true);
  });

  it('should correctly compare semver versions for update auto-detection', () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    expect(service.isNewerVersion('0.1.0', 'v0.2.0')).toBe(true);
    expect(service.isNewerVersion('0.1.0', '0.1.1')).toBe(true);
    expect(service.isNewerVersion('0.1.0', '1.0.0')).toBe(true);

    expect(service.isNewerVersion('0.2.0', 'v0.1.0')).toBe(false);
    expect(service.isNewerVersion('0.1.0', '0.1.0')).toBe(false);
    expect(service.isNewerVersion('0.1.1', 'v0.1.0')).toBe(false);
  });

  it('should manage setlist songs, reordering, and Next/Prev navigation', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    await service.sendCommand({ type: 'AddSetlistSong', payload: { dir: '/songs/Living Hope' } });
    await service.sendCommand({ type: 'AddSetlistSong', payload: { dir: '/songs/Gratitude' } });

    expect(service.snapshot().setlist_items.length).toBe(2);
    expect(service.snapshot().active_setlist_slot).toBe(0);
    expect(service.snapshot().setlist_items[0].title).toBe('Living Hope');

    await service.sendCommand({ type: 'NextSong' });
    expect(service.snapshot().active_setlist_slot).toBe(1);
    expect(service.snapshot().song_title).toBe('Gratitude');

    await service.sendCommand({ type: 'PrevSong' });
    expect(service.snapshot().active_setlist_slot).toBe(0);

    await service.sendCommand({ type: 'MoveSetlistSlot', payload: { from: 0, to: 1 } });
    expect(service.snapshot().active_setlist_slot).toBe(1);
    expect(service.snapshot().setlist_items[1].title).toBe('Living Hope');
  });

  it('should control ambient drone pad state, key switching, and P shortcut', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    expect(service.snapshot().pad_enabled).toBe(false);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'p', bubbles: true }));
    expect(service.snapshot().pad_enabled).toBe(true);

    await service.sendCommand({ type: 'SetPadKey', payload: 'D' });
    expect(service.snapshot().pad_key).toBe('D');
    expect(service.snapshot().pad_follow_song_key).toBe(false);

    await service.sendCommand({ type: 'SetPadFollowSongKey', payload: true });
    expect(service.snapshot().pad_follow_song_key).toBe(true);
    expect(service.snapshot().pad_key).toBe('G');
  });

  it('should open and validate song setup manifests in preview mode', async () => {
    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance as any;

    await app.openSongSetup('/worship/way-maker');
    expect(app.songEditorOpen()).toBe(true);
    expect(app.songEditorManifest()?.title).toBe('way-maker');
    expect(app.editableStems().length).toBeGreaterThan(0);

    const invalid = structuredClone(app.songEditorManifest());
    invalid.sections[0].start_bar = 10;
    invalid.sections[0].end_bar = 5;
    expect(app.validateSongManifest(invalid)).toContain('End Bar > Start Bar');

    invalid.sections[0].start_bar = 1;
    invalid.sections[0].end_bar = 5;
    expect(app.validateSongManifest(invalid)).toBeNull();
  });

  it('should switch workspace modes and enforce Live Lock on stage', () => {
    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance as any;

    expect(app.workspaceMode()).toBe('stage');
    app.setWorkspaceMode('studio');
    expect(app.workspaceMode()).toBe('studio');

    app.setWorkspaceMode('workbench');
    expect(app.workspaceMode()).toBe('workbench');

    app.onLiveLockChange(true);
    expect(app.liveLock()).toBe(true);
    expect(app.workspaceMode()).toBe('stage');

    app.setWorkspaceMode('studio');
    expect(app.workspaceMode()).toBe('stage');
  });

  it('should filter library, build setlist, duplicate slots, and compute harmonic transitions in Setlist Studio', async () => {
    const fixture = TestBed.createComponent(SetlistStudioComponent);
    const studio = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    expect(studio.filteredLibrary().length).toBeGreaterThanOrEqual(4);
    studio.searchQuery.set('gratitude');
    expect(studio.filteredLibrary().map((e) => e.title)).toEqual(['Gratitude']);

    studio.searchQuery.set('');
    const firstSong = studio.filteredLibrary()[0];
    const secondSong = studio.filteredLibrary()[1];
    await studio.addSongToSetlist(firstSong);
    await studio.addSongToSetlist(secondSong);

    expect(service.snapshot().setlist_items.length).toBe(2);
    expect(studio.totalSetlistDurationSec()).toBeGreaterThan(0);

    const item0 = service.snapshot().setlist_items[0];
    await studio.adjustSlotKey(item0, 2);
    expect(service.snapshot().setlist_items[0].key_shift_semitones).toBe(2);

    await studio.duplicateSlot(service.snapshot().setlist_items[0]);
    expect(service.snapshot().setlist_items.length).toBe(3);

    const bridge = computeHarmonicTransition(
      service.snapshot().setlist_items[0],
      service.snapshot().setlist_items[1],
    );
    expect(bridge.label).toContain('Same Key');
    expect(bridge.padBridge).toContain('Pad bridge');
  });

  it('should import track, compute waveform grid, add sections, and publish from Import Workbench', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    wb.localFilePath.set('/music/Holy Forever.mp3');
    wb.songTitleInput.set('Holy Forever');
    wb.separationEngine.set('None');

    await wb.startImportPipeline();
    expect(service.workbenchJob()?.status).toBe('Completed');

    await wb.openFolderInWorkbench('/library/Holy Forever');
    expect(wb.manifest()?.title).toBe('Holy Forever');
    expect(wb.analysis()?.peaks.length).toBe(640);
    expect(wb.barTicks().length).toBeGreaterThan(5);
    expect(wb.waveformPathD().length).toBeGreaterThan(20);

    const beforeSections = wb.manifest()!.sections.length;
    wb.appendPresetSection({ name: 'Chorus', bars: 8, color: 'emerald' });
    expect(wb.manifest()!.sections.length).toBe(beforeSections + 1);

    wb.nudgeDownbeat(0.05);
    expect(wb.manifest()!.first_downbeat_sec).toBeGreaterThan(0);

    await wb.saveToLibrary(true);
    expect(service.snapshot().setlist_items.some((s) => s.song_dir === '/library/Holy Forever')).toBe(
      true,
    );
  });

  it('should shift Beat 1 by beats, scale BPM half/double, and align Beat 1 for tapped BPM', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/Beat Align Test');
    wb.updateManifestField('bpm', 120);
    wb.updateManifestField('first_downbeat_sec', 0.25);

    wb.shiftDownbeatBeats(1);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(0.75, 3);

    wb.shiftDownbeatBeats(-1);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(0.25, 3);

    // Shifting -1 beat when < 1 beat from 0 wraps cleanly to +3 beats in 4/4 (0.25 + 1.5 = 1.75s)
    wb.shiftDownbeatBeats(-1);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(1.75, 3);

    const analyzeSpy = vi.spyOn(service, 'analyzeWaveform').mockResolvedValue({
      source_file: 'Mixdown (2 stems)',
      duration_sec: 180,
      sample_rate: 44100,
      channels: 2,
      estimated_bpm: 60,
      estimated_first_downbeat_sec: 0.92,
      estimated_key: 'G',
      key_confidence: 0.95,
      key_candidates: ['G'],
      peaks: Array.from({ length: 640 }, () => [-0.3, 0.3] as [number, number]),
    });
    const sendSpy = vi.spyOn(service, 'sendCommand');

    wb.scaleBpm(0.5);
    await Promise.resolve();
    expect(wb.manifest()!.bpm).toBe(60);
    expect(analyzeSpy).toHaveBeenCalledWith('/library/Beat Align Test', true, 640, 60, 4);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(0.92, 3);
    expect(sendSpy).toHaveBeenCalledWith({
      type: 'SetSongTiming',
      payload: {
        bpm: 60,
        first_downbeat_sec: 0.92,
        beats_per_bar: 4,
        click_mode: 'AlwaysOn',
        sections: wb.manifest()!.sections,
      },
    });
  });

  it('should display a scanning notification in the waveform area while building waveform', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    let resolveWave!: (v: any) => void;
    vi.spyOn(service, 'analyzeWaveform').mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveWave = resolve;
        }),
    );

    const openPromise = wb.openFolderInWorkbench('/library/Pending Wave');
    await Promise.resolve();
    await Promise.resolve();
    fixture.detectChanges();

    const notice = fixture.nativeElement.querySelector('[data-testid="waveform-scanning-notice"]') as HTMLElement | null;
    expect(notice).not.toBeNull();
    expect(notice?.textContent).toContain('Scanning audio stems & building waveform');

    resolveWave({
      source_file: 'Mixdown (2 stems)',
      duration_sec: 120,
      sample_rate: 44100,
      channels: 2,
      estimated_bpm: 74,
      estimated_first_downbeat_sec: 0.5,
      estimated_key: 'G',
      key_confidence: 0.95,
      key_candidates: ['G'],
      peaks: Array.from({ length: 640 }, () => [-0.4, 0.4] as [number, number]),
    });
    await openPromise;
    fixture.detectChanges();

    expect(
      fixture.nativeElement.querySelector('[data-testid="waveform-scanning-notice"]'),
    ).toBeNull();
  });

  it('should expose per-platform toolchain install guides and missing tool prompts', () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;

    wb.guidePlatform.set('windows');
    expect(wb.activePlatformGuide().youtubeCommand).toContain('winget');
    wb.guidePlatform.set('macos');
    expect(wb.activePlatformGuide().youtubeCommand).toContain('brew');
    wb.guidePlatform.set('linux');
    expect(wb.activePlatformGuide().youtubeCommand).toContain('apt');

    wb.separationEngine.set('AudioSeparator');
    expect(wb.missingSelectedTool()).toContain('audio-separator');

    wb.openToolchainGuide();
    expect(wb.showToolchainDrawer()).toBe(true);
  });

  it('should mute/uncheck stems, solo individual parts, remove vocals, and sync BPM to click', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/Holy Forever');
    wb.manifest.update((cur) =>
      cur
        ? {
          ...cur,
          stems: {
            drums: { file: 'drums.wav', pitched: false, is_cues: false, default_gain: 1.0, muted: false },
            bass: { file: 'bass.wav', pitched: true, is_cues: false, default_gain: 1.0, muted: false },
            vocals: { file: 'vocals.wav', pitched: true, is_cues: false, default_gain: 1.0, muted: false },
          },
        }
        : null,
    );
    await service.sendCommand({
      type: 'LoadSongDir',
      payload: '/library/Holy Forever',
    });

    // Vocals Off preset mutes vocals while keeping bass & drums active
    wb.applyInstrumentalOnly();
    expect(wb.editableStems().find((s) => s.key === 'vocals')?.muted).toBe(true);
    expect(wb.editableStems().find((s) => s.key === 'bass')?.muted).toBe(false);

    // Solo bass isolates only bass, then clearing solo restores prior state (vocals stays muted)
    wb.toggleStemSolo('bass');
    expect(wb.isStemSoloed('bass')).toBe(true);
    expect(wb.editableStems().find((s) => s.key === 'drums')?.muted).toBe(true);
    expect(wb.editableStems().find((s) => s.key === 'bass')?.muted).toBe(false);

    wb.toggleStemSolo('bass');
    expect(wb.isStemSoloed('bass')).toBe(false);
    expect(wb.editableStems().find((s) => s.key === 'drums')?.muted).toBe(false);
    expect(wb.editableStems().find((s) => s.key === 'vocals')?.muted).toBe(true);

    // Updating BPM in Workbench immediately synchronizes the metronome/engine timing
    wb.updateManifestField('bpm', 84.5);
    await Promise.resolve();
    expect(service.snapshot().current_bpm).toBe(84.5);
    expect(service.snapshot().base_bpm).toBe(84.5);

    // Live Stage Solo toggles stem isolation in snapshot
    const appFixture = TestBed.createComponent(App);
    const app = appFixture.componentInstance as unknown as {
      onStemSolo: (name: string) => void;
      isStageStemSoloed: (name: string) => boolean;
    };
    const firstStem = service.snapshot().stems[0];
    if (firstStem) {
      app.onStemSolo(firstStem.name);
      expect(app.isStageStemSoloed(firstStem.name)).toBe(true);
      expect(service.snapshot().stems.find((s) => s.name === firstStem.name)?.muted).toBe(false);
      app.onStemSolo(firstStem.name);
      expect(app.isStageStemSoloed(firstStem.name)).toBe(false);
    }
  });

  it('should index extracted YouTube stems into Library and skip re-extraction unless forced', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    wb.ingestTab.set('youtube');
    wb.youtubeUrl.set('https://www.youtube.com/watch?v=TestWorship11');
    wb.songTitleInput.set('Praise (Live)');
    wb.separationEngine.set('AudioSeparator');
    wb.stemPreset.set('FourStems');

    await wb.startImportPipeline();
    const indexed = service.libraryEntries().find((e) => e.title === 'Praise (Live)');
    expect(indexed).toBeDefined();
    expect(indexed?.provenance?.source_ref).toBe('youtube:TestWorship11');
    expect(indexed?.provenance?.stem_preset).toBe('FourStems');
    expect(indexed?.stem_names.length).toBe(4);

    // Pasting an equivalent youtu.be URL resolves the cached Library entry and avoids re-extracting
    wb.youtubeUrl.set('https://youtu.be/TestWorship11?t=25');
    wb.stemPreset.set('SixStems');
    expect(wb.cachedLibraryMatch()?.dir).toBe(indexed?.dir);
    expect(wb.cachedSeparationChanged()).toBe(true);

    await wb.startImportPipeline();
    expect(service.workbenchJob()?.stage).toBe('Loaded from Library Cache');
    expect(
      service.libraryEntries().find((e) => e.title === 'Praise (Live)')?.provenance?.stem_preset,
    ).toBe('FourStems');

    // Enabling forceReextract runs the new 6-stem preset and updates the Library entry
    wb.forceReextract.set(true);
    await wb.startImportPipeline();
    expect(service.workbenchJob()?.stage).toBe('Available in Library');
    const updated = service.libraryEntries().find((e) => e.title === 'Praise (Live)');
    expect(updated?.provenance?.stem_preset).toBe('SixStems');
    expect(updated?.stem_names.length).toBe(6);
  });

  it('should provide a prominent live-safe metronome mute button and M shortcut', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      liveLock: { set: (v: boolean) => void };
      onGlobalKeydown: (ev: KeyboardEvent) => void;
    };
    fixture.detectChanges();

    const btn = fixture.nativeElement.querySelector(
      '[data-testid="live-metronome-toggle"]',
    ) as HTMLButtonElement | null;
    expect(btn).toBeTruthy();
    expect(service.snapshot().click_muted).toBe(false);
    expect(btn?.textContent).toContain('Click: ON (M)');

    // Enable Live Lock — metronome mute MUST still work during live play
    app.liveLock.set(true);
    fixture.detectChanges();
    expect(btn?.disabled).toBe(false);

    btn?.click();
    await Promise.resolve();
    fixture.detectChanges();
    expect(service.snapshot().click_muted).toBe(true);
    expect(btn?.textContent).toContain('Click: MUTED (M)');

    // M shortcut toggles metronome mute back on
    app.onGlobalKeydown(new KeyboardEvent('keydown', { key: 'm' }));
    await Promise.resolve();
    fixture.detectChanges();
    expect(service.snapshot().click_muted).toBe(false);
  });

  it('should transpose minor keys and preserve Unknown honestly', () => {
    expect(transposeKeyName('G', 2)).toBe('A');
    expect(transposeKeyName('Em', 2)).toBe('F#m');
    expect(transposeKeyName('Unknown', 0)).toBe('Unknown');
    expect(transposeKeyName('Unknown', 2)).toBe('Unknown (+2)');
  });

  it('should switch left/right panning assignment and 4-channel stereo output modes', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    fixture.detectChanges();

    const btn2Ch = fixture.nativeElement.querySelector('[data-testid="routing-layout-2ch"]') as HTMLButtonElement;
    const btn4Ch = fixture.nativeElement.querySelector('[data-testid="routing-layout-4ch"]') as HTMLButtonElement;
    const btnMusicFirst = fixture.nativeElement.querySelector('[data-testid="routing-side-music-first"]') as HTMLButtonElement;
    const btnClickFirst = fixture.nativeElement.querySelector('[data-testid="routing-side-click-first"]') as HTMLButtonElement;

    expect(service.snapshot().routing_mode).toBe('SplitLeftRight');

    // Swap Left/Right in 2-ch split mode -> Click Left / Music Right
    btnClickFirst.click();
    await Promise.resolve();
    fixture.detectChanges();
    expect(service.snapshot().routing_mode).toBe('SplitClickLeftMusicRight');
    const mapClickLeft = fixture.nativeElement.querySelector('[data-testid="routing-active-map"]') as HTMLElement;
    expect(mapClickLeft.textContent).toContain('Out 1 / Left (Mono)');
    expect(mapClickLeft.textContent).toContain('Click + Cues (IEM)');

    // Switch to 4-ch stereo while keeping Click-first assignment -> MultiOutClickLeftMusicRight
    btn4Ch.click();
    await Promise.resolve();
    fixture.detectChanges();
    expect(service.snapshot().routing_mode).toBe('MultiOutClickLeftMusicRight');
    expect(fixture.nativeElement.querySelector('[data-testid="routing-4ch-fallback-notice"]')).not.toBeNull();

    // Swap back to Music 1-2 / Click 3-4 in 4-ch stereo -> MultiOutStereo
    btnMusicFirst.click();
    await Promise.resolve();
    fixture.detectChanges();
    expect(service.snapshot().routing_mode).toBe('MultiOutStereo');

    // Simulate a 4-channel device connected -> fallback notice disappears
    service.snapshot.update((cur) => ({ ...cur, device_channels: 4 }));
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('[data-testid="routing-4ch-fallback-notice"]')).toBeNull();

    // Switch back to 2-ch split -> SplitLeftRight
    btn2Ch.click();
    await Promise.resolve();
    fixture.detectChanges();
    expect(service.snapshot().routing_mode).toBe('SplitLeftRight');
  });

  it('should allow dragging playhead cursor and Bar 1 handle on the waveform and keep Tap Tempo stationary', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/Drag Test');
    fixture.detectChanges();

    const canvas = fixture.nativeElement.querySelector('[data-testid="waveform-canvas"]') as HTMLElement;
    const bar1Handle = fixture.nativeElement.querySelector('[data-testid="bar1-drag-handle"]') as HTMLButtonElement;
    const bar1Rail = fixture.nativeElement.querySelector('[data-testid="bar1-drag-rail"]') as HTMLElement;
    const floatingDock = fixture.nativeElement.querySelector('[data-testid="floating-tap-dock"]') as HTMLElement;
    const tapBtn = fixture.nativeElement.querySelector('[data-testid="tap-tempo-btn"]') as HTMLButtonElement;

    expect(canvas).toBeTruthy();
    expect(bar1Handle).toBeTruthy();
    expect(bar1Rail).toBeTruthy();
    expect(floatingDock).toBeTruthy();
    expect(floatingDock.className).toContain('fixed');
    expect(floatingDock.className).toContain('bottom-4');
    expect(floatingDock.className).toContain('z-50');
    expect(tapBtn).toBeTruthy();

    vi.spyOn(canvas, 'getBoundingClientRect').mockReturnValue({
      left: 100,
      top: 0,
      right: 500,
      bottom: 176,
      width: 400,
      height: 176,
      x: 100,
      y: 0,
      toJSON: () => ({}),
    });

    const dur = wb.effectiveDurationSec();
    const sendSpy = vi.spyOn(service, 'sendCommand');

    // 1. Drag playhead cursor from 25% -> 50% of waveform
    wb.onWaveformPointerDown({
      pointerType: 'mouse',
      button: 0,
      pointerId: 1,
      clientX: 200, // (200 - 100) / 400 = 0.25
      currentTarget: canvas,
    } as unknown as PointerEvent);
    expect(service.snapshot().current_position_sec).toBeCloseTo(dur * 0.25, 2);

    wb.onWindowPointerUp({
      clientX: 240, // (240 - 100) / 400 = 0.35 -> 84s (< 106.7s preview cap)
    } as unknown as PointerEvent);
    expect(service.snapshot().current_position_sec).toBeCloseTo(dur * 0.35, 2);

    // 2. Drag Bar 1 handle to 10% of waveform -> updates first_downbeat_sec and syncs timing
    wb.onBar1PointerDown(
      {
        pointerType: 'mouse',
        button: 0,
        pointerId: 2,
        clientX: 120,
        currentTarget: bar1Handle,
        stopPropagation: () => {},
        preventDefault: () => {},
      } as unknown as PointerEvent,
      canvas,
    );
    expect(wb.draggingTarget()).toBe('bar1');

    wb.onWindowPointerMove({ clientX: 140 } as unknown as PointerEvent); // 0.10
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(dur * 0.1, 2);

    wb.onWindowPointerUp({ clientX: 160 } as unknown as PointerEvent); // 0.15
    expect(wb.draggingTarget()).toBeNull();
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(dur * 0.15, 2);
    expect(sendSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'SetSongTiming',
        payload: expect.objectContaining({
          first_downbeat_sec: expect.closeTo(dur * 0.15, 2),
        }),
      }),
    );

    // 3. Tap Tempo pad has large stationary styling and stays locked across taps
    expect(tapBtn.className).toContain('w-full');
    expect(tapBtn.className).toContain('h-16');
    expect(tapBtn.className).toContain('shrink-0');
    expect(tapBtn.className).toContain('tabular-nums');
    const beforeRect = tapBtn.getBoundingClientRect();
    tapBtn.click();
    tapBtn.click();
    fixture.detectChanges();
    const afterRect = tapBtn.getBoundingClientRect();
    expect(afterRect.left).toBe(beforeRect.left);
    expect(afterRect.top).toBe(beforeRect.top);
    expect(afterRect.width).toBe(beforeRect.width);
    expect(afterRect.height).toBe(beforeRect.height);
  });

  it('should align Bar 1 and detect tempo from Beat 1 taps while auditioning', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/Beat 1 Tap Test');
    wb.updateManifestField('bpm', 120);
    wb.updateManifestField('ts_num', 4);
    wb.updateManifestField('first_downbeat_sec', 0.25);
    fixture.detectChanges();

    const tapBeat1Btn = fixture.nativeElement.querySelector('[data-testid="tap-beat1-btn"]') as HTMLButtonElement;
    expect(tapBeat1Btn).toBeTruthy();
    expect(tapBeat1Btn.className).toContain('w-full');
    expect(tapBeat1Btn.className).toContain('h-16');
    expect(tapBeat1Btn.className).toContain('shrink-0');
    expect(tapBeat1Btn.className).toContain('tabular-nums');
    const beat1RectBefore = tapBeat1Btn.getBoundingClientRect();

    // Stopped at 0s: shows helpful notice without zeroing existing Bar 1
    service.snapshot.update((cur) => ({ ...cur, is_playing: false, current_position_sec: 0 }));
    fixture.detectChanges();
    tapBeat1Btn.click();
    fixture.detectChanges();
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(0.25, 3);
    expect(wb.workbenchNotice()).toContain('Press Play Audition');

    const sendSpy = vi.spyOn(service, 'sendCommand');

    // Actual song is 80 BPM 4/4 (secPerBar = 3.0s) with Bar 1 at 0.45s while manifest starts at 120 BPM.
    // Tap 1 @ 3.45s: aligns phase under initial 120 BPM (secPerBar = 2.0s -> 1.45s)
    service.snapshot.update((cur) => ({ ...cur, is_playing: true, current_position_sec: 3.45 }));
    fixture.detectChanges();
    tapBeat1Btn.click();
    fixture.detectChanges();

    expect(wb.beat1TapCount()).toBe(1);
    expect(wb.manifest()!.bpm).toBe(120);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(1.45, 3);

    // Tap 2 @ 6.45s (dt = 3.0s): detects 80 BPM and realigns Bar 1 to 0.45s in one SetSongTiming call
    service.snapshot.update((cur) => ({ ...cur, is_playing: true, current_position_sec: 6.45 }));
    fixture.detectChanges();
    tapBeat1Btn.click();
    fixture.detectChanges();

    expect(wb.beat1TapCount()).toBe(2);
    expect(wb.manifest()!.bpm).toBe(80);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(0.45, 3);
    expect(sendSpy).toHaveBeenLastCalledWith(
      expect.objectContaining({
        type: 'SetSongTiming',
        payload: expect.objectContaining({
          bpm: 80,
          first_downbeat_sec: expect.closeTo(0.45, 3),
        }),
      }),
    );

    // Tap 3 skipped 1 bar @ 12.45s (dt = 6.0s = 2 bars): keeps 80 BPM and Bar 1 = 0.45s
    service.snapshot.update((cur) => ({ ...cur, is_playing: true, current_position_sec: 12.45 }));
    fixture.detectChanges();
    tapBeat1Btn.click();
    fixture.detectChanges();

    expect(wb.beat1TapCount()).toBe(3);
    expect(wb.manifest()!.bpm).toBe(80);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(0.45, 3);

    // Tapping an off-beat @ 13.95s (dt = 1.5s = 0.5 bars) resets Beat 1 buffer, keeps 80 BPM, snaps Bar 1 to 1.95s
    service.snapshot.update((cur) => ({ ...cur, is_playing: true, current_position_sec: 13.95 }));
    fixture.detectChanges();
    tapBeat1Btn.click();
    fixture.detectChanges();

    expect(wb.beat1TapCount()).toBe(1);
    expect(wb.manifest()!.bpm).toBe(80);
    expect(wb.manifest()!.first_downbeat_sec).toBeCloseTo(1.95, 3);

    const beat1RectAfter = tapBeat1Btn.getBoundingClientRect();
    expect(beat1RectAfter.left).toBe(beat1RectBefore.left);
    expect(beat1RectAfter.top).toBe(beat1RectBefore.top);
    expect(beat1RectAfter.width).toBe(beat1RectBefore.width);
    expect(beat1RectAfter.height).toBe(beat1RectBefore.height);
  });

  it('should configure per-song click mode and BPM per setlist slot and switch cleanly on stage', async () => {
    const fixture = TestBed.createComponent(SetlistStudioComponent);
    const studio = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'sunday-set',
        name: 'Sunday Set',
        items: [
          {
            song_id: 'song-a',
            song_dir: '/library/Song A',
            title: 'Song A',
            original_key: 'G',
            base_bpm: 72,
            base_click_mode: 'IntroOnly',
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
          {
            song_id: 'song-b',
            song_dir: '/library/Song B',
            title: 'Song B',
            original_key: 'C',
            base_bpm: 128,
            base_click_mode: 'AlwaysOn',
            key_shift_semitones: 0,
            transition_mode: 'AutoAdvance',
            crossfade_sec: 2.0,
            ambient_pad_enabled: false,
          },
        ],
      },
    });
    fixture.detectChanges();

    const itemsBefore = service.snapshot().setlist_items;
    expect(itemsBefore[0].effective_click_mode).toBe('IntroOnly');
    expect(itemsBefore[1].effective_click_mode).toBe('AlwaysOn');

    // Customize slot 1 to Off + 132 BPM while keeping slot 0 IntroOnly + 72 BPM
    await studio.setSlotClickMode(itemsBefore[1], 'Off');
    await studio.setSlotBpm(itemsBefore[1], 132);
    fixture.detectChanges();

    expect(service.snapshot().click_mode).toBe('IntroOnly');
    expect(service.snapshot().click_muted).toBe(false);
    expect(service.snapshot().current_bpm).toBe(72);

    await service.sendCommand({ type: 'SelectSetlistSlot', payload: 1 });
    fixture.detectChanges();
    expect(service.snapshot().click_mode).toBe('Off');
    expect(service.snapshot().click_muted).toBe(true);
    expect(service.snapshot().current_bpm).toBe(132);
  });

  it('should select IntroOnly click mode in Import Workbench and sync it to engine timing', async () => {
    const fixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = fixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/Variable Tempo Song');
    fixture.detectChanges();

    const sendSpy = vi.spyOn(service, 'sendCommand');
    const select = fixture.nativeElement.querySelector(
      '[data-testid="workbench-click-mode-select"]',
    ) as HTMLSelectElement | null;
    expect(select).toBeTruthy();

    wb.updateManifestField('click_mode', 'IntroOnly');
    fixture.detectChanges();

    expect(wb.manifest()!.click_mode).toBe('IntroOnly');
    expect(wb.introEndBar()).toBe(5);
    expect(sendSpy).toHaveBeenCalledWith(
      expect.objectContaining({
        type: 'SetSongTiming',
        payload: expect.objectContaining({
          click_mode: 'IntroOnly',
        }),
      }),
    );
    expect(service.snapshot().click_mode).toBe('IntroOnly');
  });

  it('should allow switching stage click mode to IntroOnly and show auto-off bar indicator', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    fixture.detectChanges();

    const select = fixture.nativeElement.querySelector(
      '[data-testid="stage-click-mode-select"]',
    ) as HTMLSelectElement | null;
    expect(select).toBeTruthy();

    await service.sendCommand({ type: 'SetClickMode', payload: 'IntroOnly' });
    fixture.detectChanges();

    expect(service.snapshot().click_mode).toBe('IntroOnly');
    expect((fixture.nativeElement as HTMLElement).textContent).toContain('Auto-off Bar 5');
  });

  it('should zoom waveform up to 32x, nudge playhead, rapidly annotate sections, and sync live to Stage', async () => {
    const wbFixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = wbFixture.componentInstance;
    const stageFixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/Section Annotator Test');
    wb.updateManifestField('bpm', 120); // 2.0 sec/bar, 0.5 sec/beat
    wb.updateManifestField('first_downbeat_sec', 0);
    wbFixture.detectChanges();

    // Step zoom in/out ergonomics (1x -> 2x -> 4x -> 8x -> ... -> 32x, and back out)
    expect(wb.waveformZoom()).toBe(1);
    expect(wb.beatTicks().length).toBe(0);
    wb.zoomIn();
    expect(wb.waveformZoom()).toBe(2);
    wb.setZoom(16);
    expect(wb.waveformZoom()).toBe(16);
    expect(wb.beatTicks().length).toBeGreaterThan(0);
    expect(wb.barTicks().every((t) => t.showLabel)).toBe(true);
    wb.zoomOut();
    expect(wb.waveformZoom()).toBe(8);
    wb.setZoom(32);
    expect(wb.waveformZoom()).toBe(32);

    // Seek to Bar 9 (16.0s at 120 BPM) and rapidly mark a Chorus, then custom name a Bridge at Bar 17 (32.0s)
    await service.sendCommand({ type: 'SeekSec', payload: 16.0 });
    wbFixture.detectChanges();
    expect(wb.playheadMarkBar()).toBe(9);

    wb.markSectionAtPlayhead('Chorus', 'emerald', 8);
    wbFixture.detectChanges();

    const sectionsAfterChorus = wb.manifest()!.sections;
    const chorus = sectionsAfterChorus.find((s) => s.name === 'Chorus');
    expect(chorus).toBeTruthy();
    expect(chorus!.start_bar).toBe(9);

    // Nudge playhead +1 bar (Bar 10) using precision control, then seek to Bar 17 and mark custom section
    wb.nudgePlayheadBeats(4);
    expect(wb.playheadMarkBar()).toBe(10);

    await service.sendCommand({ type: 'SeekSec', payload: 32.0 });
    wb.rapidSectionName.set('Spontaneous Worship');
    wb.markCustomSectionAtPlayhead();
    wbFixture.detectChanges();
    stageFixture.detectChanges();

    // Verify Workbench rapid annotations immediately became dynamic sections on Live Stage
    const stageSections = service.snapshot().sections;
    expect(stageSections.some((s) => s.name === 'Chorus' && s.start_bar === 9)).toBe(true);
    expect(stageSections.some((s) => s.name === 'Spontaneous Worship' && s.start_bar === 17)).toBe(
      true,
    );
    expect((stageFixture.nativeElement as HTMLElement).textContent).toContain('Spontaneous Worship');
  });

  it('drags and extends existing sections, renames inline, and deletes sections with contiguous live sync', async () => {
    const wbFixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = wbFixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/Edit Sections Test');
    wb.updateManifestField('bpm', 120); // 2.0 sec/bar
    wb.updateManifestField('first_downbeat_sec', 0);
    wbFixture.detectChanges();

    const initialSections = wb.manifest()!.sections;
    expect(initialSections.length).toBeGreaterThanOrEqual(2);
    const lastIdx = initialSections.length - 1;
    const origLastEnd = initialSections[lastIdx].end_bar;

    // 1. Right-edge handle exists on the trailing section and extends it via +4 bars & pointer drag
    const bands = wb.sectionBands();
    expect(bands[lastIdx].endHandleLeftPct).not.toBeNull();
    wb.extendSectionBars(lastIdx, 4);
    wbFixture.detectChanges();
    expect(wb.manifest()!.sections[lastIdx].end_bar).toBe(origLastEnd + 4);
    expect(service.snapshot().sections[lastIdx].end_bar).toBe(origLastEnd + 4);

    // Drag right-edge handle programmatically via onBoundaryPointerDown(..., 'end') + window pointermove/up
    const fakeCanvas = {
      getBoundingClientRect: () => ({ left: 0, width: 1000, top: 0, height: 100, right: 1000, bottom: 100 }),
      scrollWidth: 1000,
    } as unknown as HTMLElement;
    const ptrDown = new MouseEvent('pointerdown', { clientX: 500, bubbles: true }) as unknown as PointerEvent;
    wb.onBoundaryPointerDown(ptrDown, lastIdx, fakeCanvas, 'end');
    // Move to end of 240s waveform (clientX=1000 => 240s => Bar 121)
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 900 }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 900 }));
    wbFixture.detectChanges();
    expect(wb.manifest()!.sections[lastIdx].end_bar).toBeGreaterThan(origLastEnd + 4);

    // 2. Shared interior boundary drag moves section 1 start_bar and section 0 end_bar together
    const origSec0Id = wb.manifest()!.sections[0].id;
    // 10% of 240s = 24s = Bar 13 at 120 BPM
    wb.onBoundaryPointerDown(ptrDown, 1, fakeCanvas, 'start');
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: 100 }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: 100 }));
    wbFixture.detectChanges();
    expect(wb.manifest()!.sections[0].end_bar).toBe(13);
    expect(wb.manifest()!.sections[1].start_bar).toBe(13);

    // 3. Inline rename on section band preserves section ID and syncs live to Stage
    wb.startInlineBandRename(0);
    wb.editingBandName.set('Atmospheric Intro');
    wb.commitInlineBandRename();
    wbFixture.detectChanges();
    expect(wb.manifest()!.sections[0].name).toBe('Atmospheric Intro');
    expect(wb.manifest()!.sections[0].id).toBe(origSec0Id);
    expect(service.snapshot().sections[0].name).toBe('Atmospheric Intro');

    // 4. Deleting a section absorbs its span into neighbor without leaving gaps and syncs live
    const countBeforeDelete = wb.manifest()!.sections.length;
    const deletedEnd = wb.manifest()!.sections[1].end_bar;
    const deletedName = wb.manifest()!.sections[1].name;
    wb.removeSection(1);
    wbFixture.detectChanges();
    const afterDelete = wb.manifest()!.sections;
    expect(afterDelete.length).toBe(countBeforeDelete - 1);
    expect(afterDelete[0].end_bar).toBe(deletedEnd);
    expect(afterDelete.some((s) => s.name === deletedName)).toBe(false);
    expect(service.snapshot().sections.length).toBe(countBeforeDelete - 1);
    expect(service.snapshot().sections[0].end_bar).toBe(deletedEnd);
  });

  it('positions, drags, extends, and shortens sections to decimal timeline seconds without bar snapping', async () => {
    const wbFixture = TestBed.createComponent(ImportWorkbenchComponent);
    const wb = wbFixture.componentInstance;
    const stageFixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);

    await wb.openFolderInWorkbench('/library/YouTube Live Song');
    wb.updateManifestField('bpm', 120); // Bars fall on even 2.00s boundaries (0, 2, 4, ...)
    wb.updateManifestField('first_downbeat_sec', 0);
    wbFixture.detectChanges();

    const dur = wb.effectiveDurationSec();
    const fakeCanvas = {
      getBoundingClientRect: () => ({ left: 0, width: 1000, top: 0, height: 100, right: 1000, bottom: 100 }),
      scrollWidth: 1000,
    } as unknown as HTMLElement;
    const ptrDown = new MouseEvent('pointerdown', { clientX: 0, bubbles: true }) as unknown as PointerEvent;

    // 1. Drag Section 0 start rightward to a non-bar decimal timestamp (e.g. 2.34s spoken intro skip)
    const xForSec = (targetSec: number) => (targetSec / dur) * 1000;
    wb.onBoundaryPointerDown(ptrDown, 0, fakeCanvas, 'start');
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: xForSec(2.34) }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: xForSec(2.34) }));
    wbFixture.detectChanges();
    expect(wb.manifest()!.sections[0].start_sec).toBeCloseTo(2.34, 1);

    // 2. Drag interior boundary to non-bar decimal time (e.g. 13.47s — NOT snapped to Bar 7=12s or Bar 8=14s)
    wb.onBoundaryPointerDown(ptrDown, 1, fakeCanvas, 'start');
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: xForSec(13.47) }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: xForSec(13.47) }));
    wbFixture.detectChanges();
    expect(wb.manifest()!.sections[0].end_sec).toBeCloseTo(13.47, 1);
    expect(wb.manifest()!.sections[1].start_sec).toBeCloseTo(13.47, 1);
    expect(service.snapshot().sections[1].start_sec).toBeCloseTo(13.47, 1);

    // 3. Drag trailing section right edge to extend AND shorten via extendSectionSec
    const lastIdx = wb.manifest()!.sections.length - 1;
    wb.onBoundaryPointerDown(ptrDown, lastIdx, fakeCanvas, 'end');
    window.dispatchEvent(new MouseEvent('pointermove', { clientX: xForSec(31.65) }));
    window.dispatchEvent(new MouseEvent('pointerup', { clientX: xForSec(31.65) }));
    wbFixture.detectChanges();
    expect(wb.manifest()!.sections[lastIdx].end_sec).toBeCloseTo(31.65, 1);

    wb.extendSectionSec(lastIdx, 1.25);
    expect(wb.manifest()!.sections[lastIdx].end_sec).toBeCloseTo(32.9, 1);
    wb.extendSectionSec(lastIdx, -3.45);
    expect(wb.manifest()!.sections[lastIdx].end_sec).toBeCloseTo(29.45, 1);

    // 4. Marking a new section at an off-grid decimal playhead (21.38s) preserves exact decimal timestamp on Stage
    await service.sendCommand({ type: 'SeekSec', payload: 21.38 });
    wb.markSectionAtPlayhead('Spontaneous', 'amber', 4);
    wbFixture.detectChanges();
    stageFixture.detectChanges();

    const marked = service.snapshot().sections.find((s) => s.name === 'Spontaneous');
    expect(marked).toBeTruthy();
    expect(marked!.start_sec).toBeCloseTo(21.38, 2);
    expect((stageFixture.nativeElement as HTMLElement).textContent).toContain('0:21.38');
  });

  it('renders Saved Setlists, AI Harness Setup MCP JSON, Review Needed badges, and suppresses external sync under Live Lock', async () => {
    const studioFixture = TestBed.createComponent(SetlistStudioComponent);
    const studio = studioFixture.componentInstance;
    const service = TestBed.inject(PsalmStageService);

    service.managedSetlists.set([
      {
        path: '/Users/demo/Music/PsalmStage/Setlists/sunday-nov-15.json',
        id: 'sunday-nov-15',
        name: 'Sunday Nov 15',
        song_count: 3,
        song_titles: ['Gratitude', 'Goodness of God', 'Praise'],
        total_estimated_sec: 840,
        modified_ms: 1700000000000,
      },
    ]);

    const firstSong = service.libraryEntries()[0];
    service.libraryEntries.set([
      {
        ...firstSong,
        provenance: {
          source_kind: 'youtube',
          source_ref: 'youtube:abc12345678',
          separation_engine: 'None',
          stem_preset: 'FourStems',
          needs_review: true,
          key_confidence: 0.35,
          review_notes: ['Verify Bar 1 downbeat and sections in Workbench.'],
        },
      },
      ...service.libraryEntries().slice(1),
    ]);
    await studio.addSongToSetlist(service.libraryEntries()[0]);

    // 1. Saved Setlists drawer
    studio.showSavedSetlists.set(true);
    // 2. AI Harness Setup panel
    studio.toggleAiSetup();
    // 3. External sync notice
    service.externalSyncNotice.set('Setlist "Sunday Nov 15" (3 songs) updated on disk — ready to load.');
    studioFixture.detectChanges();

    const el = studioFixture.nativeElement as HTMLElement;
    expect(el.querySelector('[data-testid="saved-setlists-drawer"]')?.textContent).toContain('Sunday Nov 15');
    const aiPanel = el.querySelector('[data-testid="ai-harness-setup-panel"]');
    expect(aiPanel?.textContent).toContain('"psalm-stage"');
    expect(aiPanel?.textContent).toContain('psalm-stage-cli');
    expect(el.querySelector('[data-testid="library-review-badge"]')?.textContent).toContain('Review Needed');
    expect(el.querySelector('[data-testid="slot-review-badge"]')?.textContent).toContain('Review Needed');
    expect(el.querySelector('[data-testid="external-sync-banner"]')).toBeTruthy();

    // 4. Suppress external sync banner when Live Lock is active
    service.liveLockActive.set(true);
    studioFixture.detectChanges();
    expect(el.querySelector('[data-testid="external-sync-banner"]')).toBeFalsy();
  });

  it('should support Tonic Pad Studio layers, minor voicing, custom presets, and setlist Preset Lock vs Pass-Through', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as any;
    fixture.detectChanges();

    // Open Tonic Pad Studio modal
    const openBtn = fixture.nativeElement.querySelector('[data-testid="open-tonic-studio-btn"]') as HTMLButtonElement;
    expect(openBtn).toBeTruthy();
    openBtn.click();
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('[data-testid="tonic-pad-studio-modal"]')).toBeTruthy();

    // Select Reverse Piano preset and toggle Minor key mode
    const reversePreset = service.padPresets().find((p) => p.id === 'reverse-piano')!;
    expect(reversePreset).toBeTruthy();
    app.onSelectPadPreset(reversePreset);
    app.onSelectPadKey('A');
    app.onTogglePadMinor();
    fixture.detectChanges();

    expect(service.snapshot().pad_preset_name).toBe('Reverse Piano');
    expect(service.snapshot().pad_key).toBe('Am');
    expect(service.snapshot().pad_is_minor).toBe(true);

    // Customize Layer 2 and save custom user preset
    app.onUpdatePadLayer(1, { enabled: true, archetype: 'GuitarSwell', register: 'High', tonality: 'Minor' });
    app.onPadBrightnessSlider([0.74]);
    app.onPadShimmerSlider([0.63]);
    app.padNewPresetName.set('Late Night Prayer');
    await app.onSaveCustomPadPreset();
    fixture.detectChanges();

    const custom = service.padPresets().find((p) => p.name === 'Late Night Prayer');
    expect(custom).toBeTruthy();
    expect(custom!.is_factory).toBe(false);

    // Load a 2-slot setlist and lock current pad to slot 0, leaving slot 1 as Pass-Through
    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'tonic-set',
        name: 'Tonic Test Set',
        items: [
          {
            song_id: 'song-a',
            title: 'Song A',
            original_key: 'Am',
            base_bpm: 72,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
          {
            song_id: 'song-b',
            title: 'Song B',
            original_key: 'G',
            base_bpm: 78,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
        ],
      },
    });
    await service.sendCommand({ type: 'SetPadEnabled', payload: true });
    await service.sendCommand({ type: 'LockCurrentPadToSlot', payload: 0 });
    expect(service.snapshot().setlist_items[0].pad_lock).toBeTruthy();
    expect(service.snapshot().setlist_items[1].pad_lock).toBeNull();

    // Tweak brightness live and move to slot 1 (Pass-Through): keeps live brightness, updates key
    await service.sendCommand({ type: 'SetPadFollowSongKey', payload: true });
    await service.sendCommand({ type: 'SetPadBrightness', payload: 0.42 });
    await service.sendCommand({ type: 'SelectSetlistSlot', payload: 1 });
    expect(service.snapshot().pad_key).toBe('G');
    expect(service.snapshot().pad_brightness).toBeCloseTo(0.42, 2);

    // Move back to slot 0 (Locked): restores locked brightness 0.74 and follows song key Am
    await service.sendCommand({ type: 'SelectSetlistSlot', payload: 0 });
    expect(service.snapshot().pad_key).toBe('Am');
    expect(service.snapshot().pad_is_minor).toBe(true);
    expect(service.snapshot().pad_brightness).toBeCloseTo(0.74, 2);

    await app.onDeleteCustomPadPreset(custom!.id);
    expect(service.padPresets().some((p) => p.id === custom!.id)).toBe(false);
  });

  it('should support Live Stage drag-drop, multi-select batch edit, service sections, and liveLock guard', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      liveLock: { set: (v: boolean) => void };
      addSectionKind: { set: (k: 'Praise' | 'Worship' | 'Sermon') => void };
      onAddServiceSection: () => void;
      onToggleSlotSelection: (slot: number, ev?: MouseEvent) => void;
      selectedSetlistSlots: () => number[];
      batchTargetKey: { set: (v: string) => void };
      batchClickMode: { set: (v: string) => void };
      batchPadEnabled: { set: (v: string) => void };
      batchCrossfadeSec: { set: (v: string) => void };
      batchPercussionMode: { set: (v: string) => void };
      batchPercussionStyle: { set: (v: string) => void };
      applyBatchSetlistEdit: () => void;
      onLiveStageDrop: (ev: unknown) => void;
      onAddStandaloneSlot: () => void;
    };

    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'sunday-multi',
        name: 'Sunday Service',
        sections: [
          { id: 'sec-praise', name: 'Praise', kind: 'Praise', color: '#f59e0b', collapsed: false },
        ],
        items: [
          {
            song_id: 's1',
            title: 'Song One',
            original_key: 'C',
            base_bpm: 120,
            key_shift_semitones: 0,
            transition_mode: 'AutoAdvance',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
            section_id: 'sec-praise',
          },
          {
            song_id: 's2',
            title: 'Song Two',
            original_key: 'G',
            base_bpm: 74,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: false,
            section_id: 'sec-praise',
          },
          {
            song_id: 's3',
            title: 'Song Three',
            original_key: 'D',
            base_bpm: 68,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: false,
          },
        ],
      },
    });
    fixture.detectChanges();

    // Add a Worship section and standalone Sermon PadOnly slot
    app.addSectionKind.set('Worship');
    app.onAddServiceSection();
    expect(service.snapshot().setlist_sections.some((s) => s.kind === 'Worship')).toBe(true);

    app.onAddStandaloneSlot();
    expect(service.snapshot().setlist_items.length).toBe(4);
    expect(service.snapshot().setlist_items[3].slot_kind).toBe('PadOnly');

    // Shift+Click range select from slot 0 to slot 2 selects [0, 1, 2]
    app.onToggleSlotSelection(0);
    app.onToggleSlotSelection(2, { shiftKey: true } as MouseEvent);
    expect(app.selectedSetlistSlots()).toEqual([0, 1, 2]);
    fixture.detectChanges();

    expect(fixture.nativeElement.querySelector('[data-testid="live-batch-toolbar"]')).toBeTruthy();

    // Apply Batch Edit: Key -> D, Click -> Off, Pad -> on, Crossfade -> 4.0s, Percussion -> Layer + HighlifePraise
    app.batchTargetKey.set('D');
    app.batchClickMode.set('Off');
    app.batchPadEnabled.set('on');
    app.batchCrossfadeSec.set('4');
    app.batchPercussionMode.set('Layer');
    app.batchPercussionStyle.set('HighlifePraise');
    app.applyBatchSetlistEdit();
    await Promise.resolve();

    const items = service.snapshot().setlist_items;
    expect(items[0].effective_key).toBe('D');
    expect(items[1].effective_key).toBe('D');
    expect(items[1].effective_click_mode).toBe('Off');
    expect(items[1].ambient_pad_enabled).toBe(true);
    expect(items[1].crossfade_sec).toBe(4);
    expect(items[2].percussion_lock?.style).toBe('HighlifePraise');

    // Reorder via onLiveStageDrop (slot 0 -> slot 1 inside sec-praise)
    const praiseBucket = {
      id: 'sec-praise',
      section: service.snapshot().setlist_sections[0],
      items: service.snapshot().setlist_items.slice(0, 2),
    };
    app.onLiveStageDrop({
      item: { data: service.snapshot().setlist_items[0] },
      container: { data: praiseBucket },
      currentIndex: 1,
    });
    await Promise.resolve();
    expect(service.snapshot().setlist_items[1].song_id).toBe('s1');

    // Live Lock hides drag handles, checkboxes, and blocks batch mutations
    app.liveLock.set(true);
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('[data-testid="live-setlist-drag-handle"]')).toBeNull();
    expect(fixture.nativeElement.querySelector('[data-testid="live-setlist-checkbox"]')).toBeNull();
  });

  it('should control Percussion Track modes, African/Funk/Worship styles, and 9-instrument mixer', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      percussionMixerExpanded: { set: (v: boolean) => void };
      onPercussionMode: (m: 'Off' | 'Layer' | 'PercussionOnly') => void;
      onPercussionStyle: (s: 'Woro' | 'Afrobeat' | 'FunkPocket') => void;
      onPercussionIntensity: (i: 'Low' | 'Medium' | 'High') => void;
      onTogglePercussionInstrument: (inst: 'Clave' | 'Tambourine', enabled: boolean) => void;
      onPercussionChannelGainSlider: (inst: 'Clave', vals: number[]) => void;
      onResetPercussionMix: () => void;
    };

    fixture.detectChanges();
    expect(service.snapshot().percussion_mode).toBe('Off');
    expect(service.snapshot().percussion_channels.length).toBe(9);

    app.onPercussionMode('PercussionOnly');
    app.onPercussionStyle('Woro');
    app.onPercussionIntensity('High');
    expect(service.snapshot().percussion_mode).toBe('PercussionOnly');
    expect(service.snapshot().percussion_style).toBe('Woro');
    expect(service.snapshot().percussion_intensity).toBe('High');

    // Expand instrument mixer & tweak Clave + Tambourine
    app.percussionMixerExpanded.set(true);
    fixture.detectChanges();
    expect(fixture.nativeElement.querySelector('[data-testid="percussion-instrument-mixer"]')).toBeTruthy();

    app.onTogglePercussionInstrument('Tambourine', true);
    app.onPercussionChannelGainSlider('Clave', [1.25]);
    expect(
      service.snapshot().percussion_channels.find((c) => c.instrument === 'Tambourine')?.enabled,
    ).toBe(false);
    expect(
      service.snapshot().percussion_channels.find((c) => c.instrument === 'Clave')?.gain,
    ).toBe(1.25);

    app.onResetPercussionMix();
    expect(
      service.snapshot().percussion_channels.find((c) => c.instrument === 'Tambourine')?.enabled,
    ).toBe(true);
  });

  it('should create custom loops, generate studio WAV kits, and export 9-stem song bundles in Loop Studio', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      onTogglePercussionStudio: () => Promise<void>;
      onGenerateStudioKits: () => Promise<void>;
      onCloneStyleIntoEditor: (applyLive?: boolean) => Promise<void>;
      onToggleLoopStep: (inst: 'Kick' | 'Claps' | 'Tambourine', step: number) => void;
      onSaveCustomLoop: () => Promise<void>;
      onExportLoopBundle: () => Promise<void>;
      onPercussionRoomSlider: (vals: number[]) => void;
      percussionStudioOpen: () => boolean;
      loopExportStatus: () => string | null;
    };

    fixture.detectChanges();
    await app.onTogglePercussionStudio();
    fixture.detectChanges();
    expect(app.percussionStudioOpen()).toBe(true);
    expect(fixture.nativeElement.querySelector('[data-testid="percussion-loop-studio"]')).toBeTruthy();
    expect(fixture.nativeElement.querySelector('[data-testid="percussion-step-grid"]')).toBeTruthy();

    // Generate studio WAV kits & select Makossa Live kit
    await app.onGenerateStudioKits();
    expect(service.percussionKits().length).toBe(3);
    expect(service.snapshot().percussion_kit_name).toContain('Makossa');

    // Adjust studio room ambience
    app.onPercussionRoomSlider([0.38]);
    expect(service.snapshot().percussion_room_mix).toBeCloseTo(0.38, 2);

    // Clone preset, paint a custom Claps step, save loop
    await app.onCloneStyleIntoEditor(true);
    app.onToggleLoopStep('Claps', 6);
    await app.onSaveCustomLoop();
    expect(service.percussionLoops().length).toBe(1);
    expect(service.snapshot().percussion_custom_pattern?.hits.some((h) => h.instrument === 'Claps' && h.step === 6)).toBe(true);

    // Export 9-stem toggleable song bundle
    await app.onExportLoopBundle();
    fixture.detectChanges();
    expect(app.loopExportStatus()).toContain('9 isolated stems');
  });

  it('should propagate click off/on and row setting edits to all selected songs in a setlist', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      selectAllSetlistSlots: () => void;
      onClickMute: (muted: boolean) => void;
      onClickModeChange: (mode: 'AlwaysOn' | 'IntroOnly' | 'Off') => void;
      cycleSlotClickMode: (slot: number, current: 'AlwaysOn' | 'IntroOnly' | 'Off') => void;
      onBatchClickModeChange: (val: string) => void;
    };

    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'click-batch',
        name: 'Click Batch Set',
        items: [
          {
            song_id: 's1',
            title: 'Song 1',
            original_key: 'G',
            base_bpm: 72,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
          {
            song_id: 's2',
            title: 'Song 2',
            original_key: 'C',
            base_bpm: 80,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
          {
            song_id: 's3',
            title: 'Song 3',
            original_key: 'D',
            base_bpm: 84,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
        ],
      },
    });
    fixture.detectChanges();

    // 1) Select All -> turn click OFF via header Click Mute button
    app.selectAllSetlistSlots();
    app.onClickMute(true);
    await Promise.resolve();
    expect(service.snapshot().setlist_items.every((it) => it.effective_click_mode === 'Off')).toBe(
      true,
    );

    // 2) Turn click back AlwaysOn via header Click Mode dropdown while all are selected
    app.onClickModeChange('AlwaysOn');
    await Promise.resolve();
    expect(
      service.snapshot().setlist_items.every((it) => it.effective_click_mode === 'AlwaysOn'),
    ).toBe(true);

    // 3) Cycle click on a single selected song row -> applies IntroOnly, then Off across all selected slots
    app.cycleSlotClickMode(1, 'AlwaysOn');
    await Promise.resolve();
    expect(
      service.snapshot().setlist_items.every((it) => it.effective_click_mode === 'IntroOnly'),
    ).toBe(true);

    app.cycleSlotClickMode(1, 'IntroOnly');
    await Promise.resolve();
    expect(service.snapshot().setlist_items.every((it) => it.effective_click_mode === 'Off')).toBe(
      true,
    );

    // 4) Immediate Batch dropdown apply (without needing a separate Apply click)
    app.onBatchClickModeChange('AlwaysOn');
    await Promise.resolve();
    expect(
      service.snapshot().setlist_items.every((it) => it.effective_click_mode === 'AlwaysOn'),
    ).toBe(true);

    app.onBatchClickModeChange('Off');
    await Promise.resolve();
    expect(service.snapshot().setlist_items.every((it) => it.effective_click_mode === 'Off')).toBe(
      true,
    );
  });

  it('should immediately propagate BPM changes to all selected setlist songs in Live Stage and Studio', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      selectAllSetlistSlots: () => void;
      onBatchBpmChange: (val: string | number) => void;
      onBpmAdjust: (delta: number) => void;
      onHeaderBpmChange: (val: string | number) => void;
    };

    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'bpm-batch',
        name: 'BPM Batch Set',
        items: [
          {
            song_id: 's1',
            title: 'Song 1',
            original_key: 'G',
            base_bpm: 72,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
          {
            song_id: 's2',
            title: 'Song 2',
            original_key: 'C',
            base_bpm: 80,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
          {
            song_id: 's3',
            title: 'Song 3',
            original_key: 'D',
            base_bpm: 94,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
        ],
      },
    });
    fixture.detectChanges();

    // Select all songs, partial keystrokes '1' and '10' do nothing, '100' updates all immediately
    app.selectAllSetlistSlots();
    app.onBatchBpmChange('1');
    app.onBatchBpmChange('10');
    await Promise.resolve();
    expect(service.snapshot().setlist_items.map((it) => it.effective_bpm)).toEqual([72, 80, 94]);

    app.onBatchBpmChange('100');
    await Promise.resolve();
    expect(service.snapshot().setlist_items.every((it) => it.effective_bpm === 100)).toBe(true);
    expect(service.snapshot().current_bpm).toBe(100);

    // Header +1 BPM while all selected moves every selected song from 100 -> 101
    app.onBpmAdjust(1);
    await Promise.resolve();
    expect(service.snapshot().setlist_items.every((it) => it.effective_bpm === 101)).toBe(true);

    // Header direct BPM input sets all selected songs immediately
    app.onHeaderBpmChange(112);
    await Promise.resolve();
    expect(service.snapshot().setlist_items.every((it) => it.effective_bpm === 112)).toBe(true);

    // Setlist Studio setSlotBpm and onStudioBatchBpmChange propagate across checked slots
    const studioFixture = TestBed.createComponent(SetlistStudioComponent);
    const studio = studioFixture.componentInstance;
    studio.selectedStudioSlots.set([0, 1, 2]);
    await studio.setSlotBpm(service.snapshot().setlist_items[0], 100);
    expect(service.snapshot().setlist_items.every((it) => it.effective_bpm === 100)).toBe(true);

    studio.onStudioBatchBpmChange('96');
    await Promise.resolve();
    expect(service.snapshot().setlist_items.every((it) => it.effective_bpm === 96)).toBe(true);
  });

  it('should keep Dynamic Song Sections top-left, Sunday Setlist top-right, full-card song click, and hide setup under Live Lock', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      liveLock: { set: (v: boolean) => void; (): boolean };
    };

    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'live-ux-set',
        name: 'Live UX Set',
        items: [
          {
            song_id: 's1',
            title: 'Very Long Worship Song Title That Must Stay Unoccluded In Live Performance Mode',
            original_key: 'G',
            base_bpm: 72,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
          {
            song_id: 's2',
            title: 'Second Praise Anthem Full Title',
            original_key: 'A',
            base_bpm: 128,
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: false,
          },
        ],
      },
    });
    fixture.detectChanges();

    const host = fixture.nativeElement as HTMLElement;
    const primaryCol = host.querySelector('[data-testid="live-stage-primary-col"]') as HTMLElement;
    const setlistCol = host.querySelector('[data-testid="live-stage-setlist-col"]') as HTMLElement;
    expect(primaryCol).not.toBeNull();
    expect(setlistCol).not.toBeNull();
    expect(primaryCol.textContent).toContain('Dynamic Song Sections');
    expect(primaryCol.textContent).toContain('Tonic Pad Player');
    expect(primaryCol.textContent).toContain('Percussion Track');
    expect(setlistCol.textContent).toContain('Sunday Setlist');
    // Setup cards visible when unlocked
    expect(setlistCol.textContent).toContain('Song Library & Bundle Setup');
    expect(setlistCol.textContent).toContain('Audio Output Interface');

    // Engage Live Lock: setup cards hide, full row becomes a single clickable button with full unoccluded title
    app.liveLock.set(true);
    fixture.detectChanges();

    expect(setlistCol.textContent).not.toContain('Song Library & Bundle Setup');
    expect(setlistCol.textContent).not.toContain('Audio Output Interface');
    expect(setlistCol.textContent).not.toContain('MIDI Controller & Keyboard');

    const rows = Array.from(host.querySelectorAll('[data-testid="live-setlist-row"]')) as HTMLElement[];
    expect(rows.length).toBe(2);
    // In Live Lock mode each row has exactly 1 button covering the whole row (no disabled inner buttons blocking clicks)
    for (const row of rows) {
      const buttons = row.querySelectorAll('button');
      expect(buttons.length).toBe(1);
      expect(buttons[0].getAttribute('data-testid')).toBe('live-setlist-select-btn');
      expect(buttons[0].disabled).toBe(false);
    }

    const titles = Array.from(host.querySelectorAll('[data-testid="live-setlist-title"]'));
    expect(titles[0].textContent).toContain('Very Long Worship Song Title');
    expect(titles[1].textContent).toContain('Second Praise Anthem Full Title');

    const nowPlayingBanner = host.querySelector('[data-testid="dynamic-sections-now-playing"]') as HTMLElement;
    const nowPlayingTitle = host.querySelector('[data-testid="dynamic-sections-song-title"]') as HTMLElement;
    expect(nowPlayingBanner).not.toBeNull();
    expect(nowPlayingTitle.textContent).toContain('Very Long Worship Song Title');

    // Clicking anywhere on the second song card switches active song immediately under Live Lock
    const secondBtn = rows[1].querySelector('[data-testid="live-setlist-select-btn"]') as HTMLButtonElement;
    secondBtn.click();
    await Promise.resolve();
    fixture.detectChanges();
    expect(service.snapshot().active_setlist_slot).toBe(1);
    expect(nowPlayingTitle.textContent).toContain('Second Praise Anthem Full Title');
    expect(nowPlayingBanner.textContent).toContain('Song 2 / 2');

    // Starting playback clearly marks Playing in the Dynamic Song Sections now-playing banner
    await service.sendCommand({ type: 'Play' });
    fixture.detectChanges();
    expect(nowPlayingBanner.textContent).toContain('Playing');
  });

  it('should persist preview setlist and stage configurations across service reload', async () => {
    const service = TestBed.inject(PsalmStageService);
    await service.sendCommand({ type: 'SetSetlistName', payload: 'Sunday Night Vigil' });
    await service.sendCommand({
      type: 'UpsertSetlistSection',
      payload: {
        id: 'sec-praise',
        name: 'High Praise',
        kind: 'Praise',
        collapsed: false,
      },
    });
    await service.sendCommand({
      type: 'AddStandaloneSetlistSlot',
      payload: {
        title: 'Opening Prayer Pad',
        slot_kind: 'PadOnly',
        key: 'D',
        bpm: 70,
        time_signature: [4, 4],
        section_id: 'sec-praise',
      },
    });
    await service.sendCommand({
      type: 'AddStandaloneSetlistSlot',
      payload: {
        title: 'Afrobeat Praise Groove',
        slot_kind: 'PercussionGroove',
        key: 'G',
        bpm: 112,
        time_signature: [4, 4],
        section_id: 'sec-praise',
      },
    });
    await service.sendCommand({ type: 'SelectSetlistSlot', payload: 1 });
    await service.sendCommand({ type: 'SetRoutingMode', payload: 'SplitClickLeftMusicRight' });
    await service.sendCommand({ type: 'SetPercussionStyle', payload: 'Afrobeat' });
    await service.sendCommand({ type: 'SetClickMuted', payload: true });
    await service.sendCommand({ type: 'SetPadEnabled', payload: true });
    await service.sendCommand({ type: 'Play' });

    const stored = window.localStorage.getItem(PREVIEW_SESSION_STORAGE_KEY);
    expect(stored).toBeTruthy();

    // Simulate an app reload with a fresh TestBed & PsalmStageService instance
    TestBed.resetTestingModule();
    await TestBed.configureTestingModule({ imports: [App] }).compileComponents();
    const reloaded = TestBed.inject(PsalmStageService);
    const snap = reloaded.snapshot();

    expect(snap.setlist_name).toBe('Sunday Night Vigil');
    expect(snap.setlist_sections.map((s) => s.name)).toContain('High Praise');
    expect(snap.setlist_items.length).toBe(2);
    expect(snap.setlist_items[1].title).toBe('Afrobeat Praise Groove');
    expect(snap.active_setlist_slot).toBe(1);
    expect(snap.routing_mode).toBe('SplitClickLeftMusicRight');
    expect(snap.percussion_style).toBe('Afrobeat');
    expect(snap.click_muted).toBe(true);
    // Safety reset on reload: transport stopped and tonic pad disengaged
    expect(snap.is_playing).toBe(false);
    expect(snap.pad_enabled).toBe(false);
  });

  it('should persist UI workspace preferences across App reload', async () => {
    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance as unknown as {
      workspaceMode: { set: (m: 'stage' | 'studio' | 'workbench') => void; (): string };
      customFolderPath: { set: (p: string) => void; (): string };
      selectedMidiPortIndex: { set: (i: number) => void; (): number };
      percussionMixerExpanded: { set: (v: boolean) => void; (): boolean };
      padStudioOpen: { set: (v: boolean) => void; (): boolean };
    };
    app.workspaceMode.set('studio');
    app.customFolderPath.set('/Music/Sunday/Rehearsal');
    app.selectedMidiPortIndex.set(2);
    app.percussionMixerExpanded.set(true);
    app.padStudioOpen.set(true);
    fixture.detectChanges();
    await fixture.whenStable();

    expect(window.localStorage.getItem(UI_SESSION_STORAGE_KEY)).toContain('studio');

    fixture.destroy();
    TestBed.resetTestingModule();
    await TestBed.configureTestingModule({ imports: [App] }).compileComponents();
    const nextFixture = TestBed.createComponent(App);
    const nextApp = nextFixture.componentInstance as unknown as typeof app;

    expect(nextApp.workspaceMode()).toBe('studio');
    expect(nextApp.customFolderPath()).toBe('/Music/Sunday/Rehearsal');
    expect(nextApp.selectedMidiPortIndex()).toBe(2);
    expect(nextApp.percussionMixerExpanded()).toBe(true);
    expect(nextApp.padStudioOpen()).toBe(true);
  });

  it('switches to dedicated Loop Studio workspace, plays loop audition, and authors 4-bar variations with turnaround fill', async () => {
    const service = TestBed.inject(PsalmStageService);
    const fixture = TestBed.createComponent(App);
    fixture.detectChanges();
    await fixture.whenStable();

    // Switch via top navigation Loop Studio button
    const loopsTab = fixture.nativeElement.querySelector(
      '[data-testid="workspace-tab-loops"]',
    ) as HTMLButtonElement;
    expect(loopsTab).toBeTruthy();
    loopsTab.click();
    fixture.detectChanges();
    await fixture.whenStable();

    expect(
      fixture.nativeElement.querySelector('[data-testid="dedicated-loop-studio"]'),
    ).toBeTruthy();

    // Play loop directly from Loop Studio independent transport
    const playBtn = fixture.nativeElement.querySelector(
      '[data-testid="loop-studio-play-btn"]',
    ) as HTMLButtonElement;
    expect(playBtn).toBeTruthy();
    playBtn.click();
    fixture.detectChanges();
    expect(service.snapshot().percussion_audition_playing).toBe(true);

    // Adjust BPM live while loop is auditioning
    const bpmInput = fixture.nativeElement.querySelector(
      '[data-testid="loop-studio-bpm-input"]',
    ) as HTMLInputElement;
    bpmInput.value = '126';
    bpmInput.dispatchEvent(new Event('input'));
    bpmInput.dispatchEvent(new Event('change'));
    fixture.detectChanges();

    // Expand cycle length to 4 Bars
    const bars4Btn = fixture.nativeElement.querySelector(
      '[data-testid="loop-cycle-bars-4"]',
    ) as HTMLButtonElement;
    bars4Btn.click();
    fixture.detectChanges();

    expect(service.snapshot().percussion_custom_pattern?.bars_in_cycle).toBe(4);
    expect(
      fixture.nativeElement.querySelector('[data-testid="loop-bar-tab-3"]'),
    ).toBeTruthy();

    // Switch to Bar 4 tab, apply turnaround fill variation on last beat of Bar 4 (steps 60..63)
    const bar4Tab = fixture.nativeElement.querySelector(
      '[data-testid="loop-bar-tab-3"]',
    ) as HTMLButtonElement;
    bar4Tab.click();
    fixture.detectChanges();

    const turnaroundBtn = fixture.nativeElement.querySelector(
      '[data-testid="loop-turnaround-fill-btn"]',
    ) as HTMLButtonElement;
    turnaroundBtn.click();
    fixture.detectChanges();

    const pat = service.snapshot().percussion_custom_pattern;
    expect(
      pat?.hits.some((h) => h.step === 63 && h.instrument === 'Snare' && h.variant === 'Accent'),
    ).toBe(true);

    // Stop audition and return to Live Stage via "Use on Live Stage"
    playBtn.click();
    fixture.detectChanges();
    expect(service.snapshot().percussion_audition_playing).toBe(false);

    const useLiveBtn = fixture.nativeElement.querySelector(
      '[data-testid="loop-use-on-stage-btn"]',
    ) as HTMLButtonElement;
    useLiveBtn.click();
    fixture.detectChanges();
    await fixture.whenStable();

    expect(service.snapshot().percussion_mode).not.toBe('Off');
    expect(
      fixture.nativeElement.querySelector('[data-testid="dynamic-sections-card"]'),
    ).toBeTruthy();
  });

  it('syncs custom loop tempo to live setlist tempo and exposes saved loops inside Stage Style & Setlist Studio', async () => {
    const service = TestBed.inject(PsalmStageService);
    await service.sendCommand({
      type: 'AddStandaloneSetlistSlot',
      payload: {
        title: 'Sunday Worship 94',
        slot_kind: 'PercussionGroove',
        key: 'G',
        bpm: 94,
        time_signature: [4, 4],
      },
    });
    const addedSlot = service.snapshot().setlist_items.findIndex((i) => i.title === 'Sunday Worship 94');
    await service.sendCommand({ type: 'SelectSetlistSlot', payload: addedSlot });
    expect(service.snapshot().current_bpm).toBe(94);
    expect(service.snapshot().percussion_audition_bpm).toBe(94);

    // Save a custom loop in Loop Studio and confirm audition plays at live setlist tempo (94 BPM)
    const fixture = TestBed.createComponent(App);
    fixture.detectChanges();
    await fixture.whenStable();

    (
      fixture.nativeElement.querySelector('[data-testid="workspace-tab-loops"]') as HTMLButtonElement
    ).click();
    fixture.detectChanges();
    await fixture.whenStable();

    const bpmInput = fixture.nativeElement.querySelector(
      '[data-testid="loop-studio-bpm-input"]',
    ) as HTMLInputElement;
    expect(Number(bpmInput.value)).toBe(94);

    const titleInput = fixture.nativeElement.querySelector(
      '[data-testid="loop-studio-title-input"]',
    ) as HTMLInputElement;
    titleInput.value = 'Sunday Groove 94';
    titleInput.dispatchEvent(new Event('input'));
    fixture.detectChanges();

    (
      fixture.nativeElement.querySelector('[data-testid="loop-studio-save-btn"]') as HTMLButtonElement
    ).click();
    fixture.detectChanges();
    await fixture.whenStable();

    // Adjusting BPM in live stage updates loop audition BPM in lockstep
    (
      fixture.nativeElement.querySelector('[data-testid="loop-studio-play-btn"]') as HTMLButtonElement
    ).click();
    fixture.detectChanges();
    await service.sendCommand({ type: 'AdjustBpm', payload: 4 });
    fixture.detectChanges();
    await fixture.whenStable();
    fixture.detectChanges();
    expect(service.snapshot().current_bpm).toBe(98);
    expect(service.snapshot().percussion_audition_bpm).toBe(98);
    expect(Number(bpmInput.value)).toBe(98);

    // Return to Live Stage and verify the saved loop appears inside Style / Custom Loop dropdown
    (
      fixture.nativeElement.querySelector('[data-testid="loop-use-on-stage-btn"]') as HTMLButtonElement
    ).click();
    fixture.detectChanges();
    await fixture.whenStable();

    const stageStyleSelect = fixture.nativeElement.querySelector(
      '[data-testid="percussion-style-select"]',
    ) as HTMLSelectElement;
    const customOpt = Array.from(stageStyleSelect.options).find((o) =>
      o.textContent?.includes('Sunday Groove 94'),
    );
    expect(customOpt).toBeTruthy();

    // Also verify it appears in Setlist Studio per-slot Style / Custom Loop picker
    const studioTab = Array.from(
      fixture.nativeElement.querySelectorAll('nav button'),
    ).find((b) => (b as HTMLElement).textContent?.includes('Setlist Studio')) as HTMLButtonElement;
    studioTab.click();
    fixture.detectChanges();
    await fixture.whenStable();

    const slotStyleSelect = fixture.nativeElement.querySelector(
      '[data-testid="slot-percussion-style-select"]',
    ) as HTMLSelectElement;
    const slotCustomOpt = Array.from(slotStyleSelect.options).find((o) =>
      o.textContent?.includes('Sunday Groove 94'),
    );
    expect(slotCustomOpt).toBeTruthy();
  });

  it('rescales hit steps proportionally onto beat columns when switching Loop Studio grid meter', async () => {
    const service = TestBed.inject(PsalmStageService);
    // Seed a 2-bar, 16-step pattern with kicks directly on beats 1..4 of bar 1 and beat 1 of bar 2
    // before entering Loop Studio so its draft initializes directly from this pattern.
    await service.sendCommand({
      type: 'SetCustomPercussionPattern',
      payload: {
        id: 'beat-align-test',
        name: 'Beat Align Test',
        steps_per_bar: 16,
        bars_in_cycle: 2,
        swing: 0.0,
        is_compound: false,
        hits: [
          { step: 0, instrument: 'Kick', variant: 'Standard', velocity: 0.9, min_intensity: 'Low' },
          { step: 4, instrument: 'Kick', variant: 'Standard', velocity: 0.9, min_intensity: 'Low' },
          { step: 8, instrument: 'Kick', variant: 'Standard', velocity: 0.9, min_intensity: 'Low' },
          { step: 12, instrument: 'Kick', variant: 'Standard', velocity: 0.9, min_intensity: 'Low' },
          { step: 16, instrument: 'Kick', variant: 'Standard', velocity: 0.9, min_intensity: 'Low' },
        ],
      },
    });

    const fixture = TestBed.createComponent(App);
    fixture.detectChanges();
    await fixture.whenStable();

    (
      fixture.nativeElement.querySelector('[data-testid="workspace-tab-loops"]') as HTMLButtonElement
    ).click();
    fixture.detectChanges();
    await fixture.whenStable();

    const gridSelect = fixture.nativeElement.querySelector(
      '[data-testid="loop-studio-grid-select"]',
    ) as HTMLSelectElement;
    const opt12 = Array.from(gridSelect.options).find((o) =>
      o.textContent?.includes('12 Steps'),
    );
    expect(opt12).toBeTruthy();
    gridSelect.value = opt12!.value;
    gridSelect.dispatchEvent(new Event('change'));
    fixture.detectChanges();
    await fixture.whenStable();

    const rescaled = service.snapshot().percussion_custom_pattern;
    expect(rescaled?.steps_per_bar).toBe(12);
    expect(rescaled?.is_compound).toBe(true);
    const kickSteps = rescaled?.hits
      .filter((h) => h.instrument === 'Kick')
      .map((h) => h.step)
      .sort((a, b) => a - b);
    expect(kickSteps).toEqual([0, 3, 6, 9, 12]);
  });

  it('exposes latest saved custom loops in Live Stage & Setlist Studio multi-select style batch controls', async () => {
    const service = TestBed.inject(PsalmStageService);
    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'batch-custom-style-set',
        name: 'Batch Custom Style Set',
        items: [
          {
            song_id: 's1',
            song_dir: '/tmp/s1',
            title: 'Song One',
            original_key: 'G',
            base_bpm: 112,
            key_shift_semitones: 0,
            bpm_override: null,
            transition_mode: 'Manual',
            crossfade_sec: 1.5,
            ambient_pad_enabled: false,
            pad_key_override: null,
            click_mode: null,
            section_id: null,
          },
          {
            song_id: 's2',
            song_dir: '/tmp/s2',
            title: 'Song Two',
            original_key: 'A',
            base_bpm: 116,
            key_shift_semitones: 0,
            bpm_override: null,
            transition_mode: 'Manual',
            crossfade_sec: 1.5,
            ambient_pad_enabled: false,
            pad_key_override: null,
            click_mode: null,
            section_id: null,
          },
          {
            song_id: 's3',
            song_dir: '/tmp/s3',
            title: 'Song Three',
            original_key: 'C',
            base_bpm: 120,
            key_shift_semitones: 0,
            bpm_override: null,
            transition_mode: 'Manual',
            crossfade_sec: 1.5,
            ambient_pad_enabled: false,
            pad_key_override: null,
            click_mode: null,
            section_id: null,
          },
        ],
      },
    });

    await service.savePercussionLoop({
      id: 'alujo-latest',
      name: 'Alujo Latest',
      steps_per_bar: 16,
      bars_in_cycle: 2,
      swing: 0.04,
      is_compound: false,
      hits: [{ step: 0, instrument: 'Kick', variant: 'Standard', velocity: 0.9, min_intensity: 'Low' }],
    });

    const fixture = TestBed.createComponent(App);
    const app = fixture.componentInstance as unknown as {
      onToggleSlotSelection: (slot: number, event?: MouseEvent) => void;
      onBatchPercussionStyleChange: (val: string) => void;
    };
    fixture.detectChanges();
    await fixture.whenStable();

    // Check slots 0 and 1 on Live Stage so the batch edit toolbar renders
    app.onToggleSlotSelection(0);
    app.onToggleSlotSelection(1);
    fixture.detectChanges();
    await fixture.whenStable();

    const liveBatchStyleSelect = fixture.nativeElement.querySelector(
      '[data-testid="live-batch-perc-style"]',
    ) as HTMLSelectElement;
    expect(liveBatchStyleSelect).toBeTruthy();
    const customOpt = Array.from(liveBatchStyleSelect.options).find((o) =>
      o.textContent?.includes('Alujo Latest'),
    );
    expect(customOpt).toBeTruthy();

    // Apply latest custom loop across checked slots [0, 1]
    app.onBatchPercussionStyleChange('custom:alujo-latest');
    await Promise.resolve();
    fixture.detectChanges();

    expect(service.snapshot().setlist_items[0].percussion_lock?.custom_pattern?.id).toBe('alujo-latest');
    expect(service.snapshot().setlist_items[1].percussion_lock?.custom_pattern?.id).toBe('alujo-latest');
    expect(service.snapshot().setlist_items[2].percussion_lock).toBeNull();

    // Switch checked slots back to built-in style clears custom_pattern cleanly
    app.onBatchPercussionStyleChange('Woro');
    await Promise.resolve();
    expect(service.snapshot().setlist_items[0].percussion_lock?.style).toBe('Woro');
    expect(service.snapshot().setlist_items[0].percussion_lock?.custom_pattern).toBeNull();
    expect(service.snapshot().setlist_items[1].percussion_lock?.style).toBe('Woro');
    expect(service.snapshot().setlist_items[1].percussion_lock?.custom_pattern).toBeNull();

    // Verify Setlist Studio multi-select style selector also lists and applies 'Alujo Latest'
    const studioTab = Array.from(
      fixture.nativeElement.querySelectorAll('nav button'),
    ).find((b) => (b as HTMLElement).textContent?.includes('Setlist Studio')) as HTMLButtonElement;
    studioTab.click();
    fixture.detectChanges();
    await fixture.whenStable();

    const checkboxes = fixture.nativeElement.querySelectorAll(
      '[aria-label="Select slot for batch edit"]',
    ) as NodeListOf<HTMLInputElement>;
    checkboxes[0].click();
    checkboxes[2].click();
    fixture.detectChanges();
    await fixture.whenStable();

    const studioBatchStyleSelect = fixture.nativeElement.querySelector(
      '[data-testid="studio-batch-perc-style"]',
    ) as HTMLSelectElement;
    expect(studioBatchStyleSelect).toBeTruthy();
    const studioCustomOpt = Array.from(studioBatchStyleSelect.options).find((o) =>
      o.textContent?.includes('Alujo Latest'),
    );
    expect(studioCustomOpt).toBeTruthy();

    studioBatchStyleSelect.value = studioCustomOpt!.value;
    studioBatchStyleSelect.dispatchEvent(new Event('change'));
    fixture.detectChanges();
    await fixture.whenStable();

    expect(service.snapshot().setlist_items[0].percussion_lock?.custom_pattern?.id).toBe('alujo-latest');
    expect(service.snapshot().setlist_items[2].percussion_lock?.custom_pattern?.id).toBe('alujo-latest');
  });

  it('turns off the ambient drone pad when stopping playback', async () => {
    const service = TestBed.inject(PsalmStageService);
    await service.sendCommand({ type: 'SetPadEnabled', payload: true });
    await service.sendCommand({ type: 'Play' });
    expect(service.snapshot().is_playing).toBe(true);
    expect(service.snapshot().pad_enabled).toBe(true);

    await service.sendCommand({ type: 'PlayStop' });
    expect(service.snapshot().is_playing).toBe(false);
    expect(service.snapshot().pad_enabled).toBe(false);

    await service.sendCommand({ type: 'SetPadEnabled', payload: true });
    await service.sendCommand({ type: 'Stop' });
    expect(service.snapshot().is_playing).toBe(false);
    expect(service.snapshot().pad_enabled).toBe(false);
  });

  it('serializes and restores Stem & IEM Mixer settings with the setlist', async () => {
    const service = TestBed.inject(PsalmStageService);
    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'mix-test',
        name: 'Mixer Set',
        items: [
          {
            song_id: 's1',
            song_dir: '/songs/s1',
            title: 'Song One',
            original_key: 'G',
            base_bpm: 74,
            base_click_mode: 'AlwaysOn',
            key_shift_semitones: 0,
            transition_mode: 'AutoPause',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
        ],
      },
    });

    await service.sendCommand({ type: 'SetMasterGain', payload: 0.72 });
    await service.sendCommand({ type: 'SetClickGain', payload: 0.61 });
    await service.sendCommand({ type: 'SetClickMuted', payload: true });
    await service.sendCommand({ type: 'SetStemGain', payload: { name: 'drums', gain: 0.45 } });
    await service.sendCommand({ type: 'SetStemMuted', payload: { name: 'drums', muted: true } });
    await service.sendCommand({ type: 'SetStemGain', payload: { name: 'music', gain: 1.12 } });

    const savedManifest = serializeSetlistManifest(service.snapshot());
    expect(savedManifest.mixer).toEqual({
      master_gain: 0.72,
      click_gain: 0.61,
      click_muted: true,
    });
    const savedDrums = savedManifest.items[0].stem_mix?.find((s) => s.name === 'drums');
    expect(savedDrums).toEqual({ name: 'drums', gain: 0.45, muted: true });
    const savedMusic = savedManifest.items[0].stem_mix?.find((s) => s.name === 'music');
    expect(savedMusic).toEqual({ name: 'music', gain: 1.12, muted: false });

    // Mutate mixer away then reload the saved setlist
    await service.sendCommand({ type: 'SetMasterGain', payload: 1.0 });
    await service.sendCommand({ type: 'SetClickGain', payload: 0.85 });
    await service.sendCommand({ type: 'SetClickMuted', payload: false });
    await service.sendCommand({ type: 'SetStemGain', payload: { name: 'drums', gain: 1.0 } });
    await service.sendCommand({ type: 'SetStemMuted', payload: { name: 'drums', muted: false } });
    await service.sendCommand({ type: 'SetStemGain', payload: { name: 'music', gain: 0.9 } });

    await service.sendCommand({ type: 'LoadSetlist', payload: savedManifest });
    const restored = service.snapshot();
    expect(restored.master_gain).toBeCloseTo(0.72, 2);
    expect(restored.click_gain).toBeCloseTo(0.61, 2);
    expect(restored.click_muted).toBe(true);
    const restoredDrums = restored.stems.find((s) => s.name === 'drums');
    expect(restoredDrums?.gain).toBeCloseTo(0.45, 2);
    expect(restoredDrums?.muted).toBe(true);
    const restoredMusic = restored.stems.find((s) => s.name === 'music');
    expect(restoredMusic?.gain).toBeCloseTo(1.12, 2);
  });

  it('should toggle continue-until-stopped transport & per-slot mode and engage screen keep-awake in live lock mode', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    const app = fixture.componentInstance as unknown as {
      onLiveLockChange: (locked: boolean) => void;
      onToggleContinueTrack: () => void;
      cycleTransitionMode: (slot: number, current: string) => void;
    };
    fixture.detectChanges();

    expect(service.snapshot().continue_until_stopped).toBe(false);
    app.onToggleContinueTrack();
    fixture.detectChanges();
    expect(service.snapshot().continue_until_stopped).toBe(true);
    const continueBtn = fixture.nativeElement.querySelector(
      '[data-testid="live-continue-track-toggle"]',
    ) as HTMLButtonElement | null;
    expect(continueBtn?.textContent).toContain('CONTINUING UNTIL STOP');

    await service.sendCommand({
      type: 'LoadSetlist',
      payload: {
        id: 'continue-slot-setlist',
        name: 'Continue Slot Test',
        sections: [],
        items: [
          {
            song_id: 'song-1',
            song_dir: '/songs/song-1',
            title: 'Song 1',
            original_key: 'G',
            base_bpm: 120,
            key_shift_semitones: 0,
            transition_mode: 'AutoAdvance',
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
          },
        ],
      },
    });
    app.cycleTransitionMode(0, 'AutoAdvance');
    fixture.detectChanges();
    expect(service.snapshot().setlist_items[0].transition_mode).toBe('ContinueUntilStopped');
    const slotBtn = fixture.nativeElement.querySelector(
      '[data-testid="slot-transition-mode-btn"]',
    ) as HTMLButtonElement | null;
    expect(slotBtn?.textContent).toContain('Continue');

    const releaseSpy = vi.fn().mockResolvedValue(undefined);
    const requestSpy = vi.fn().mockResolvedValue({ release: releaseSpy });
    const origWakeLock = (navigator as unknown as { wakeLock?: unknown }).wakeLock;
    Object.defineProperty(navigator, 'wakeLock', {
      configurable: true,
      value: { request: requestSpy },
    });

    try {
      app.onLiveLockChange(true);
      await Promise.resolve();
      fixture.detectChanges();
      expect(requestSpy).toHaveBeenCalledWith('screen');
      expect(service.screenWakeLockActive()).toBe(true);
      expect(
        fixture.nativeElement.querySelector('[data-testid="live-wake-lock-badge"]')?.textContent,
      ).toContain('Screen Awake');

      app.onLiveLockChange(false);
      await Promise.resolve();
      fixture.detectChanges();
      expect(releaseSpy).toHaveBeenCalled();
      expect(service.screenWakeLockActive()).toBe(false);
      expect(
        fixture.nativeElement.querySelector('[data-testid="live-wake-lock-badge"]'),
      ).toBeNull();
    } finally {
      Object.defineProperty(navigator, 'wakeLock', {
        configurable: true,
        value: origWakeLock,
      });
    }
  });

  it('discovers newly connected audio output devices like Focusrite and switches immediately', async () => {
    const fixture = TestBed.createComponent(App);
    const service = TestBed.inject(PsalmStageService);
    fixture.detectChanges();

    const builtin = {
      name: 'MacBook Pro Speakers',
      max_channels: 2,
      default_sample_rate: 48000,
      is_default: true,
    };
    service.applyAudioDevices([builtin], false);
    fixture.detectChanges();
    expect(service.newlyDetectedOutput()).toBeNull();

    const focusrite = {
      name: 'Focusrite Scarlett 4i4 USB',
      max_channels: 4,
      default_sample_rate: 48000,
      is_default: false,
    };
    service.applyAudioDevices([builtin, focusrite], false);
    fixture.detectChanges();

    expect(service.newlyDetectedOutput()?.name).toBe('Focusrite Scarlett 4i4 USB');
    const banner = fixture.nativeElement.querySelector(
      '[data-testid="audio-hotplug-banner"]',
    ) as HTMLElement | null;
    expect(banner?.textContent).toContain('Focusrite Scarlett 4i4 USB');

    const useBtn = fixture.nativeElement.querySelector(
      '[data-testid="use-detected-output-btn"]',
    ) as HTMLButtonElement | null;
    useBtn?.click();
    await Promise.resolve();
    fixture.detectChanges();

    expect(service.snapshot().device_name).toBe('Focusrite Scarlett 4i4 USB');
    expect(service.snapshot().device_channels).toBe(4);
    expect(service.newlyDetectedOutput()).toBeNull();
  });
});
