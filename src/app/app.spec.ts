import { TestBed } from '@angular/core/testing';
import { App } from './app';
import { PsalmStageService } from './services/psalm-stage.service';

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
});
