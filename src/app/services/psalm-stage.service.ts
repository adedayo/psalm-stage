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

  private unlistenFns: UnlistenFn[] = [];

  constructor() {
    if (this.isTauriRuntime()) {
      void this.initTauriBridge();
    }
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

      this.unlistenFns.push(unlistenSnap, unlistenMidi);
    } catch (err) {
      this.statusMessage.set(`Audio bridge error: ${String(err)}`);
    }
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
    const [devices, ports] = await Promise.all([
      invoke<AudioDeviceInfo[]>('list_audio_devices'),
      invoke<MidiPortInfo[]>('list_midi_ports'),
    ]);
    this.audioDevices.set(devices);
    this.midiPorts.set(ports);
  }

  async connectMidiPort(portIndex: number): Promise<void> {
    if (!this.isTauriRuntime()) return;
    const name = await invoke<string>('connect_midi_port', { portIndex });
    this.connectedMidiPort.set(name);
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
