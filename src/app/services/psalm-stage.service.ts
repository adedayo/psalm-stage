import { Injectable, OnDestroy, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import {
  type AudioDeviceInfo,
  type EngineCommand,
  type EngineSnapshot,
  type MidiBinding,
  type MidiPortInfo,
  type MidiTarget,
  type MidiTriggerBehavior,
  type RawMidiEvent,
  type AppUpdateInfo,
} from '../models/engine.models';

const DEMO_FALLBACK_SNAPSHOT: EngineSnapshot = {
  song_id: 'demo-praise',
  song_title: 'Psalm 23 Demo (Browser Preview)',
  original_key: 'G',
  current_key: 'G',
  is_playing: false,
  current_bar: 1,
  current_beat: 1,
  beats_per_bar: 4,
  active_section_id: 'intro',
  armed_section_id: null,
  loop_enabled: false,
  quantize_mode: 'NextBar',
  key_shift_semitones: 0,
  base_bpm: 72,
  current_bpm: 72,
  master_gain: 1.0,
  click_gain: 0.85,
  click_muted: false,
  routing_mode: 'SplitLeftRight',
  device_name: 'Browser Preview (Run via Tauri for Focusrite)',
  device_sample_rate: 48000,
  device_channels: 2,
  sections: [
    { id: 'intro', name: 'Intro', start_bar: 1, end_bar: 5, color: 'slate' },
    { id: 'verse', name: 'Verse 1', start_bar: 5, end_bar: 13, color: 'sky' },
    { id: 'chorus', name: 'Chorus', start_bar: 13, end_bar: 21, color: 'emerald' },
    { id: 'bridge', name: 'Bridge', start_bar: 21, end_bar: 29, color: 'amber' },
    { id: 'outro', name: 'Outro', start_bar: 29, end_bar: 33, color: 'violet' },
  ],
  stems: [
    { name: 'drums', pitched: false, is_cues: false, gain: 0.9, muted: false },
    { name: 'bass', pitched: true, is_cues: false, gain: 0.9, muted: false },
    { name: 'music', pitched: true, is_cues: false, gain: 0.9, muted: false },
  ],
};

@Injectable({ providedIn: 'root' })
export class PsalmStageService implements OnDestroy {
  readonly isTauriRuntime = signal<boolean>(
    typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window,
  );

  readonly snapshot = signal<EngineSnapshot>(DEMO_FALLBACK_SNAPSHOT);
  readonly audioDevices = signal<AudioDeviceInfo[]>([]);
  readonly midiPorts = signal<MidiPortInfo[]>([]);
  readonly connectedMidiPort = signal<string | null>(null);
  readonly midiBindings = signal<MidiBinding[]>([]);
  readonly lastRawMidi = signal<RawMidiEvent | null>(null);
  readonly midiAssignMode = signal<boolean>(false);
  readonly listeningTarget = signal<{
    target: MidiTarget;
    label: string;
    behavior: MidiTriggerBehavior;
  } | null>(null);
  readonly statusMessage = signal<string | null>(null);
  readonly APP_VERSION = '0.1.0';
  readonly availableUpdate = signal<AppUpdateInfo | null>(null);
  readonly isCheckingUpdate = signal<boolean>(false);
  readonly updateDismissed = signal<boolean>(false);

  private unlistenFns: UnlistenFn[] = [];

  constructor() {
    if (this.isTauriRuntime()) {
      void this.initTauriBridge();
    }
    // Check for new releases in the background (non-blocking)
    setTimeout(() => {
      void this.checkForUpdates();
    }, 2500);
  }

  isNewerVersion(current: string, target: string): boolean {
    const parse = (v: string) =>
      v
        .replace(/^v/i, '')
        .split('.')
        .map((part) => parseInt(part, 10) || 0);

    const [curMaj = 0, curMin = 0, curPatch = 0] = parse(current);
    const [tarMaj = 0, tarMin = 0, tarPatch = 0] = parse(target);

    if (tarMaj > curMaj) return true;
    if (tarMaj < curMaj) return false;
    if (tarMin > curMin) return true;
    if (tarMin < curMin) return false;
    return tarPatch > curPatch;
  }

  async checkForUpdates(): Promise<AppUpdateInfo | null> {
    this.isCheckingUpdate.set(true);
    try {
      const controller = new AbortController();
      const timeout = setTimeout(() => controller.abort(), 4000);
      const res = await fetch('https://api.github.com/repos/dayo-a/psalm-stage/releases/latest', {
        signal: controller.signal,
        headers: { Accept: 'application/vnd.github.v3+json' },
      });
      clearTimeout(timeout);
      if (!res.ok) return null;
      const data = (await res.json()) as {
        tag_name?: string;
        name?: string;
        html_url?: string;
        body?: string;
        published_at?: string;
      };
      if (data.tag_name && this.isNewerVersion(this.APP_VERSION, data.tag_name)) {
        const update: AppUpdateInfo = {
          currentVersion: this.APP_VERSION,
          latestVersion: data.tag_name.replace(/^v/i, ''),
          name: data.name ?? data.tag_name,
          url: data.html_url ?? 'https://github.com/dayo-a/psalm-stage/releases',
          notes: (data.body ?? '').slice(0, 300),
          publishedAt: data.published_at ?? new Date().toISOString(),
        };
        this.availableUpdate.set(update);
        return update;
      }
      return null;
    } catch {
      // Gracefully ignore network errors on stage
      return null;
    } finally {
      this.isCheckingUpdate.set(false);
    }
  }

  async openInBrowser(url: string): Promise<void> {
    if (this.isTauriRuntime()) {
      try {
        await invoke('open_in_browser', { url });
        return;
      } catch {
        // Fallback to window.open
      }
    }
    if (typeof window !== 'undefined') {
      window.open(url, '_blank', 'noopener,noreferrer');
    }
  }

  dismissUpdate(): void {
    this.updateDismissed.set(true);
  }

  private async initTauriBridge(): Promise<void> {
    try {
      const [snap, devices, ports, connected, bindings] = await Promise.all([
        invoke<EngineSnapshot>('get_snapshot'),
        invoke<AudioDeviceInfo[]>('list_audio_devices'),
        invoke<MidiPortInfo[]>('list_midi_ports'),
        invoke<string | null>('connected_midi_port'),
        invoke<MidiBinding[]>('get_midi_bindings'),
      ]);

      this.snapshot.set(snap);
      this.audioDevices.set(devices);
      this.midiPorts.set(ports);
      this.connectedMidiPort.set(connected);
      this.midiBindings.set(bindings);

      const unlistenSnap = await listen<EngineSnapshot>('engine:snapshot', (event) => {
        this.snapshot.set(event.payload);
      });

      const unlistenMidi = await listen<RawMidiEvent>('midi:raw', (event) => {
        this.lastRawMidi.set(event.payload);
        const pending = this.listeningTarget();
        if (pending && this.midiAssignMode()) {
          void this.completeMidiLearn(event.payload, pending.target, pending.behavior);
        }
      });

      const unlistenPorts = await listen<MidiPortInfo[]>('midi:ports', (event) => {
        this.midiPorts.set(event.payload);
      });

      const unlistenConnected = await listen<string | null>('midi:connected-port', (event) => {
        this.connectedMidiPort.set(event.payload);
      });

      this.unlistenFns.push(unlistenSnap, unlistenMidi, unlistenPorts, unlistenConnected);
    } catch (err) {
      this.statusMessage.set(`Audio bridge error: ${String(err)}`);
    }
  }

  formatMidiEvent(raw: RawMidiEvent | null): string {
    if (!raw) return '';
    const ch = `Ch${raw.channel}`;
    if (raw.kind === 'Note') {
      const noteNames = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
      const oct = Math.floor(raw.number / 12) - 1;
      const noteName = `${noteNames[raw.number % 12]}${oct}`;
      return `${ch} • Note ${noteName} (${raw.number}) • Vel ${raw.value}`;
    }
    if (raw.kind === 'ControlChange') {
      return `${ch} • CC ${raw.number} • Val ${raw.value}`;
    }
    if (raw.kind === 'PitchBend') {
      return `${ch} • Pitch Bend • ${raw.value}`;
    }
    return `${ch} • Prog Change ${raw.number}`;
  }

  async sendCommand(command: EngineCommand): Promise<void> {
    if (!this.isTauriRuntime()) {
      this.applyPreviewCommand(command);
      return;
    }
    try {
      await invoke('send_command', { command });
    } catch (err) {
      this.statusMessage.set(String(err));
    }
  }

  async refreshHardware(): Promise<void> {
    if (!this.isTauriRuntime()) return;
    try {
      const [devices, ports, connected] = await Promise.all([
        invoke<AudioDeviceInfo[]>('list_audio_devices'),
        invoke<MidiPortInfo[]>('list_midi_ports'),
        invoke<string | null>('connected_midi_port'),
      ]);
      this.audioDevices.set(devices);
      this.midiPorts.set(ports);
      this.connectedMidiPort.set(connected);
    } catch (err) {
      this.statusMessage.set(`Hardware scan error: ${String(err)}`);
    }
  }

  async connectMidiPort(portIndex: number): Promise<void> {
    if (!this.isTauriRuntime()) return;
    try {
      const name = await invoke<string>('connect_midi_port', { portIndex });
      this.connectedMidiPort.set(name);
      this.statusMessage.set(`MIDI Connected: ${name}`);
    } catch (err) {
      this.statusMessage.set(`MIDI Connection Error: ${String(err)}`);
    }
  }

  async connectAllMidi(): Promise<void> {
    if (!this.isTauriRuntime()) return;
    try {
      const name = await invoke<string>('connect_all_midi');
      this.connectedMidiPort.set(name);
      this.statusMessage.set(`MIDI Connected: ${name}`);
    } catch (err) {
      this.statusMessage.set(`MIDI Omni Error: ${String(err)}`);
    }
  }

  async disconnectMidiPort(): Promise<void> {
    if (!this.isTauriRuntime()) return;
    await invoke('disconnect_midi_port');
    this.connectedMidiPort.set(null);
  }

  async toggleMidiAssignMode(enabled: boolean): Promise<void> {
    this.midiAssignMode.set(enabled);
    if (!enabled) {
      this.listeningTarget.set(null);
    }
    if (this.isTauriRuntime()) {
      await invoke('set_midi_learn_mode', { enabled });
    }
  }

  startListeningForTarget(
    target: MidiTarget,
    label: string,
    behavior: MidiTriggerBehavior = 'Momentary',
  ): void {
    if (!this.midiAssignMode()) return;
    this.listeningTarget.set({ target, label, behavior });
  }

  cancelListening(): void {
    this.listeningTarget.set(null);
  }

  async deleteMidiBinding(id: string): Promise<void> {
    if (!this.isTauriRuntime()) {
      this.midiBindings.update((list) => list.filter((b) => b.id !== id));
      return;
    }
    const updated = await invoke<MidiBinding[]>('delete_midi_binding', { id });
    this.midiBindings.set(updated);
  }

  bindingLabelFor(target: MidiTarget): string | null {
    const match = this.midiBindings().find(
      (b) => JSON.stringify(b.target) === JSON.stringify(target),
    );
    if (!match) return null;
    const ch = match.channel ? `Ch${match.channel}` : 'Any';
    const kind =
      match.kind === 'Note' ? `Note ${match.number}` : match.kind === 'ControlChange' ? `CC ${match.number}` : `PC ${match.number}`;
    return `${ch} • ${kind}`;
  }

  private async completeMidiLearn(
    raw: RawMidiEvent,
    target: MidiTarget,
    behavior: MidiTriggerBehavior,
  ): Promise<void> {
    const id = `${JSON.stringify(target)}::${raw.channel}::${raw.kind}::${raw.number}`;
    const binding: MidiBinding = {
      id,
      device_name: raw.device_name,
      channel: raw.channel,
      kind: raw.kind,
      number: raw.number,
      behavior: raw.kind === 'ControlChange' && behavior === 'Continuous' ? 'Continuous' : behavior,
      target,
      invert: false,
    };
    this.listeningTarget.set(null);
    if (this.isTauriRuntime()) {
      const updated = await invoke<MidiBinding[]>('save_midi_binding', { binding });
      this.midiBindings.set(updated);
    } else {
      this.midiBindings.update((list) => [...list.filter((b) => b.id !== id), binding]);
    }
  }

  private applyPreviewCommand(command: EngineCommand): void {
    this.snapshot.update((cur) => {
      switch (command.type) {
        case 'PlayStop':
          return { ...cur, is_playing: !cur.is_playing };
        case 'Play':
          return { ...cur, is_playing: true };
        case 'Stop':
          return { ...cur, is_playing: false, armed_section_id: null };
        case 'ToggleLoop':
          return { ...cur, loop_enabled: !cur.loop_enabled };
        case 'SetQuantizeMode':
          return { ...cur, quantize_mode: command.payload };
        case 'QueueSectionById':
          return cur.is_playing
            ? { ...cur, armed_section_id: command.payload }
            : { ...cur, active_section_id: command.payload };
        case 'AdjustKeyShift':
          return {
            ...cur,
            key_shift_semitones: Math.max(-12, Math.min(12, cur.key_shift_semitones + command.payload)),
          };
        case 'AdjustBpm':
          return {
            ...cur,
            current_bpm: Math.max(30, Math.min(260, cur.current_bpm + command.payload)),
          };
        case 'SetRoutingMode':
          return { ...cur, routing_mode: command.payload };
        case 'SetMasterGain':
          return { ...cur, master_gain: command.payload };
        case 'SetClickGain':
          return { ...cur, click_gain: command.payload };
        case 'SetClickMuted':
          return { ...cur, click_muted: command.payload };
        case 'SetStemGain':
          return {
            ...cur,
            stems: cur.stems.map((s) =>
              s.name === command.payload.name ? { ...s, gain: command.payload.gain } : s,
            ),
          };
        case 'SetStemMuted':
          return {
            ...cur,
            stems: cur.stems.map((s) =>
              s.name === command.payload.name ? { ...s, muted: command.payload.muted } : s,
            ),
          };
        default:
          return cur;
      }
    });
  }

  ngOnDestroy(): void {
    for (const unlisten of this.unlistenFns) {
      unlisten();
    }
  }
}
