export type OutputRoutingMode =
  | 'SplitLeftRight'
  | 'SplitClickLeftMusicRight'
  | 'MultiOutStereo';

export type QuantizeMode = 'NextBar' | 'EndOfSection';

export interface StemSnapshot {
  name: string;
  pitched: boolean;
  is_cues: boolean;
  gain: number;
  muted: boolean;
}

export interface SectionSnapshot {
  id: string;
  name: string;
  start_bar: number;
  end_bar: number;
  color?: string | null;
}

export interface EngineSnapshot {
  song_id: string | null;
  song_title: string;
  original_key: string;
  current_key: string;
  is_playing: boolean;
  current_bar: number;
  current_beat: number;
  beats_per_bar: number;
  active_section_id: string | null;
  armed_section_id: string | null;
  loop_enabled: boolean;
  quantize_mode: QuantizeMode;
  key_shift_semitones: number;
  base_bpm: number;
  current_bpm: number;
  master_gain: number;
  click_gain: number;
  click_muted: boolean;
  routing_mode: OutputRoutingMode;
  device_name: string;
  device_sample_rate: number;
  device_channels: number;
  sections: SectionSnapshot[];
  stems: StemSnapshot[];
}

export interface AudioDeviceInfo {
  name: string;
  max_channels: number;
  default_sample_rate: number;
  is_default: boolean;
}

export interface MidiPortInfo {
  index: number;
  name: string;
}

export type MidiMessageKind = 'Note' | 'ControlChange' | 'ProgramChange' | 'PitchBend';
export type MidiTriggerBehavior = 'Momentary' | 'Toggle' | 'Continuous';

export type MidiTarget =
  | { type: 'PlayStop' }
  | { type: 'Play' }
  | { type: 'Stop' }
  | { type: 'LoopToggle' }
  | { type: 'SectionBySlot'; value: number }
  | { type: 'SectionByName'; value: string }
  | { type: 'NextSection' }
  | { type: 'PrevSection' }
  | { type: 'KeyUp' }
  | { type: 'KeyDown' }
  | { type: 'TempoUp' }
  | { type: 'TempoDown' }
  | { type: 'MasterGain' }
  | { type: 'ClickGain' }
  | { type: 'CuesGain' }
  | { type: 'StemGain'; value: string };

export interface MidiBinding {
  id: string;
  device_name: string | null;
  channel: number | null;
  kind: MidiMessageKind;
  number: number;
  behavior: MidiTriggerBehavior;
  target: MidiTarget;
  invert: boolean;
}

export interface RawMidiEvent {
  device_name: string;
  channel: number;
  kind: MidiMessageKind;
  number: number;
  value: number;
}

export type EngineCommand =
  | { type: 'LoadDemoSong' }
  | { type: 'LoadSongDir'; payload: string }
  | { type: 'Play' }
  | { type: 'Stop' }
  | { type: 'PlayStop' }
  | { type: 'QueueSectionById'; payload: string }
  | { type: 'QueueSectionBySlot'; payload: number }
  | { type: 'QueueSectionByName'; payload: string }
  | { type: 'NextSection' }
  | { type: 'PrevSection' }
  | { type: 'ToggleLoop' }
  | { type: 'SetLoop'; payload: boolean }
  | { type: 'SetQuantizeMode'; payload: QuantizeMode }
  | { type: 'SetKeyShift'; payload: number }
  | { type: 'AdjustKeyShift'; payload: number }
  | { type: 'SetBpm'; payload: number }
  | { type: 'AdjustBpm'; payload: number }
  | { type: 'SetMasterGain'; payload: number }
  | { type: 'SetClickGain'; payload: number }
  | { type: 'SetClickMuted'; payload: boolean }
  | { type: 'SetStemGain'; payload: { name: string; gain: number } }
  | { type: 'SetStemMuted'; payload: { name: string; muted: boolean } }
  | { type: 'SetRoutingMode'; payload: OutputRoutingMode }
  | { type: 'SelectOutputDevice'; payload: string };

export interface AppUpdateInfo {
  currentVersion: string;
  latestVersion: string;
  name: string;
  url: string;
  notes: string;
  publishedAt: string;
}
