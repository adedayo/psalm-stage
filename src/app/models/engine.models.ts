export type OutputRoutingMode =
  | 'SplitLeftRight'
  | 'SplitClickLeftMusicRight'
  | 'MultiOutStereo'
  | 'MultiOutClickLeftMusicRight';

export type QuantizeMode = 'NextBar' | 'EndOfSection';

export type ClickMode = 'AlwaysOn' | 'IntroOnly' | 'Off';

export type TransitionMode = 'AutoAdvance' | 'AutoPause' | 'Manual' | 'ContinueUntilStopped';

export type PadVoiceArchetype =
  | 'WarmAnalog'
  | 'BrightSynth'
  | 'EvolvingMotion'
  | 'StringsCello'
  | 'OrganPad'
  | 'GuitarSwell'
  | 'ReversePiano'
  | 'AiryChoir';

export type PadRegister = 'Low' | 'Mid' | 'High';

export type PadTonality = 'Auto' | 'OpenFifth' | 'Major' | 'Minor' | 'Melodic';

export interface PadLayerConfig {
  enabled: boolean;
  archetype: PadVoiceArchetype;
  register: PadRegister;
  tonality: PadTonality;
  gain: number;
}

export interface PadPatchSpec {
  preset_id?: string | null;
  preset_name: string;
  layers: PadLayerConfig[];
  brightness: number;
  shimmer: number;
  pan: number;
  crossfade_sec?: number | null;
}

export interface PadPreset {
  id: string;
  name: string;
  category: string;
  is_factory: boolean;
  layers: PadLayerConfig[];
  brightness: number;
  shimmer: number;
  pan: number;
  crossfade_sec?: number | null;
}

export interface PadLockConfig {
  enabled: boolean;
  gain: number;
  preset_id?: string | null;
  preset_name?: string | null;
  layers: PadLayerConfig[];
  brightness: number;
  shimmer: number;
  pan: number;
}

export type PreloadState =
  | { status: 'Unloaded' | 'Loading' | 'Ready' }
  | { status: 'Error'; error: string };

export type ServiceSectionKind =
  | 'Praise'
  | 'Worship'
  | 'Sermonic'
  | 'Sermon'
  | 'Ministry'
  | 'Offering'
  | 'Custom';

export interface SetlistSection {
  id: string;
  name: string;
  kind: ServiceSectionKind;
  color?: string | null;
  collapsed: boolean;
  note?: string | null;
}

export type SetlistSlotKind = 'SongBundle' | 'PadOnly' | 'PercussionGroove';

export type PercussionMode = 'Off' | 'Layer' | 'PercussionOnly';

export type PercussionStyle =
  | 'Worship44'
  | 'Worship68'
  | 'Afrobeat'
  | 'HighlifePraise'
  | 'Woro'
  | 'Makossa'
  | 'Amapiano'
  | 'FunkPocket'
  | 'GospelPraise'
  | 'AcousticCajon'
  | 'LatinCaribbean'
  | 'ReggaeSoca';

export type GrooveIntensity = 'Low' | 'Medium' | 'High';

export type PercussionInstrument =
  | 'Kick'
  | 'Snare'
  | 'HiHat'
  | 'HandDrums'
  | 'Tambourine'
  | 'Shaker'
  | 'Claps'
  | 'Clave'
  | 'Bell';

export type HitVariant = 'Standard' | 'Accent' | 'Soft' | 'Special';

export interface CustomPatternHit {
  step: number;
  instrument: PercussionInstrument;
  velocity: number;
  variant: HitVariant;
  min_intensity: GrooveIntensity;
}

export interface CustomPercussionPattern {
  id: string;
  name: string;
  steps_per_bar: number;
  bars_in_cycle: number;
  swing: number;
  is_compound: boolean;
  hits: CustomPatternHit[];
}

export type StudioKitProfile = 'StudioPraise' | 'MakossaLive' | 'AcousticCajon';

export interface PercussionKitInfo {
  id: string;
  name: string;
  dir: string;
  sample_count: number;
  instruments_covered: PercussionInstrument[];
}

export interface ExportLoopBundleRequest {
  title: string;
  output_dir?: string | null;
  bpm: number;
  time_signature: [number, number];
  bars: number;
  dynamic_arrangement: boolean;
  intensity: GrooveIntensity;
  room_mix: number;
  style: PercussionStyle;
  custom_pattern?: CustomPercussionPattern | null;
  kit_dir?: string | null;
}

export interface ExportLoopBundleResult {
  song_dir: string;
  title: string;
  bpm: number;
  duration_sec: number;
  stem_files: string[];
}

export interface PercussionChannelConfig {
  instrument: PercussionInstrument;
  enabled: boolean;
  gain: number;
  pan: number;
}

export interface PercussionLockConfig {
  mode: PercussionMode;
  style: PercussionStyle;
  intensity: GrooveIntensity;
  gain: number;
  iem_send: number;
  follow_sections: boolean;
  room_mix?: number;
  kit_name?: string | null;
  kit_dir?: string | null;
  custom_pattern?: CustomPercussionPattern | null;
  channels: PercussionChannelConfig[];
}

export interface ServiceSectionPreset {
  kind: ServiceSectionKind;
  label: string;
  color: string;
}

export const SERVICE_SECTION_PRESETS: ServiceSectionPreset[] = [
  { kind: 'Praise', label: 'Praise', color: '#f59e0b' },
  { kind: 'Worship', label: 'Worship', color: '#8b5cf6' },
  { kind: 'Sermonic', label: 'Sermonic', color: '#ec4899' },
  { kind: 'Sermon', label: 'Sermon', color: '#38bdf8' },
  { kind: 'Ministry', label: 'Ministry', color: '#10b981' },
  { kind: 'Offering', label: 'Offering', color: '#eab308' },
  { kind: 'Custom', label: 'Custom', color: '#64748b' },
];

export interface PercussionStyleOption {
  value: PercussionStyle;
  label: string;
  group: 'Worship' | 'African' | 'Funk & Gospel' | 'Acoustic & Island';
}

export const PERCUSSION_STYLE_OPTIONS: PercussionStyleOption[] = [
  { value: 'Worship44', label: 'Worship 4/4', group: 'Worship' },
  { value: 'Worship68', label: 'Worship 6/8', group: 'Worship' },
  { value: 'Afrobeat', label: 'Afrobeat', group: 'African' },
  { value: 'HighlifePraise', label: 'African Highlife / Praise', group: 'African' },
  { value: 'Woro', label: 'Woro / Nigerian 12/8', group: 'African' },
  { value: 'Makossa', label: 'Makossa / Soukous', group: 'African' },
  { value: 'Amapiano', label: 'Amapiano', group: 'African' },
  { value: 'FunkPocket', label: 'Funk Pocket', group: 'Funk & Gospel' },
  { value: 'GospelPraise', label: 'Gospel Praise / Shout', group: 'Funk & Gospel' },
  { value: 'AcousticCajon', label: 'Acoustic Cajon / Shaker', group: 'Acoustic & Island' },
  { value: 'LatinCaribbean', label: 'Latin / Caribbean Clave', group: 'Acoustic & Island' },
  { value: 'ReggaeSoca', label: 'Reggae / Soca', group: 'Acoustic & Island' },
];

export interface PercussionInstrumentMeta {
  instrument: PercussionInstrument;
  label: string;
  shortLabel: string;
  defaultGain: number;
  defaultPan: number;
}

export const PERCUSSION_INSTRUMENT_META: PercussionInstrumentMeta[] = [
  { instrument: 'Kick', label: 'Kick / Log Drum', shortLabel: 'Kick', defaultGain: 0.9, defaultPan: 0.0 },
  { instrument: 'Snare', label: 'Snare / Rimshot', shortLabel: 'Snare', defaultGain: 0.8, defaultPan: -0.05 },
  { instrument: 'HiHat', label: 'Hi-Hat', shortLabel: 'Hat', defaultGain: 0.65, defaultPan: 0.25 },
  { instrument: 'HandDrums', label: 'Talking Drum / Conga / Cajon', shortLabel: 'Drums', defaultGain: 0.85, defaultPan: -0.2 },
  { instrument: 'Tambourine', label: 'Tambourine', shortLabel: 'Tamb', defaultGain: 0.75, defaultPan: -0.35 },
  { instrument: 'Shaker', label: 'Shaker / Shekere', shortLabel: 'Shaker', defaultGain: 0.72, defaultPan: 0.35 },
  { instrument: 'Claps', label: 'Claps', shortLabel: 'Claps', defaultGain: 0.78, defaultPan: 0.12 },
  { instrument: 'Clave', label: 'Clave / Woodblock', shortLabel: 'Clave', defaultGain: 0.7, defaultPan: -0.15 },
  { instrument: 'Bell', label: 'Agogo / Cowbell', shortLabel: 'Bell', defaultGain: 0.68, defaultPan: 0.28 },
];

export function defaultPercussionChannels(): PercussionChannelConfig[] {
  return PERCUSSION_INSTRUMENT_META.map((m) => ({
    instrument: m.instrument,
    enabled: true,
    gain: m.defaultGain,
    pan: m.defaultPan,
  }));
}

export interface StemMixOverride {
  name: string;
  gain: number;
  muted: boolean;
}

export interface SetlistMixerConfig {
  master_gain: number;
  backing_gain?: number;
  click_gain: number;
  click_muted?: boolean | null;
}

export interface SetlistItemSnapshot {
  slot: number;
  song_id: string;
  song_dir: string | null;
  title: string;
  original_key: string;
  effective_key: string;
  base_bpm: number;
  effective_bpm: number;
  key_shift_semitones: number;
  bpm_override: number | null;
  transition_mode: TransitionMode;
  crossfade_sec: number;
  ambient_pad_enabled: boolean;
  pad_key_override: string | null;
  pad_lock?: PadLockConfig | null;
  click_mode: ClickMode | null;
  effective_click_mode: ClickMode;
  section_id?: string | null;
  slot_kind?: SetlistSlotKind;
  time_signature?: [number, number] | null;
  percussion_lock?: PercussionLockConfig | null;
  stem_mix?: StemMixOverride[];
  estimated_duration_sec?: number;
  preload_state: PreloadState;
}

export interface SetlistItem {
  song_id: string;
  song_dir?: string | null;
  title?: string | null;
  original_key?: string | null;
  base_bpm?: number | null;
  base_click_mode?: ClickMode | null;
  key_shift_semitones: number;
  bpm_override?: number | null;
  click_mode?: ClickMode | null;
  transition_mode: TransitionMode;
  crossfade_sec: number;
  ambient_pad_enabled: boolean;
  pad_key_override?: string | null;
  pad_lock?: PadLockConfig | null;
  section_id?: string | null;
  slot_kind?: SetlistSlotKind;
  time_signature?: [number, number] | null;
  percussion_lock?: PercussionLockConfig | null;
  stem_mix?: StemMixOverride[];
}

export interface SetlistManifest {
  id: string;
  name: string;
  sections?: SetlistSection[];
  items: SetlistItem[];
  mixer?: SetlistMixerConfig | null;
}

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
  start_sec?: number | null;
  end_sec?: number | null;
  color?: string | null;
}

export interface SectionDef {
  id: string;
  name: string;
  start_bar: number;
  end_bar: number;
  start_sec?: number | null;
  end_sec?: number | null;
  color?: string | null;
}

export interface StemDefDetailed {
  file: string;
  pitched: boolean;
  is_cues?: boolean | null;
  default_gain: number;
  muted?: boolean;
}

export interface ImportProvenance {
  source_kind: string;
  source_ref: string;
  original_source?: string | null;
  separation_engine: string;
  stem_preset: string;
  custom_model?: string | null;
  needs_review?: boolean;
  key_confidence?: number | null;
  review_notes?: string[];
}

export interface SongManifest {
  id: string;
  title: string;
  artist: string;
  bpm: number;
  time_signature: [number, number];
  first_downbeat_sec: number;
  original_key: string;
  provenance?: ImportProvenance | null;
  click_mode?: ClickMode;
  sections: SectionDef[];
  stems: Record<string, string | StemDefDetailed>;
}

export interface InspectedSongBundle {
  dir: string;
  has_existing_manifest: boolean;
  estimated_duration_sec: number;
  available_files: string[];
  manifest: SongManifest;
}

export interface SectionCueOptions {
  lead_in_beats: number;
  include_chime: boolean;
  sample_rate?: number | null;
  use_os_tts?: boolean | null;
}

export interface SectionCueTimingOverride {
  bpm?: number | null;
  first_downbeat_sec?: number | null;
  beats_per_bar?: number | null;
  sections?: SectionDef[] | null;
}

export interface SectionCuePlacement {
  section_id: string;
  section_name: string;
  phrase: string;
  section_start_sec: number;
  cue_start_sec: number;
}

export interface GeneratedSectionCues {
  stem_key: string;
  file_name: string;
  used_os_tts: boolean;
  placements: SectionCuePlacement[];
  duration_sec: number;
  manifest: SongManifest;
}

export interface EngineSnapshot {
  song_id: string | null;
  active_song_dir: string | null;
  song_title: string;
  original_key: string;
  current_key: string;
  is_playing: boolean;
  current_bar: number;
  current_beat: number;
  beats_per_bar: number;
  current_position_sec: number;
  total_duration_sec: number;
  active_section_id: string | null;
  armed_section_id: string | null;
  loop_enabled: boolean;
  continue_until_stopped: boolean;
  quantize_mode: QuantizeMode;
  key_shift_semitones: number;
  base_bpm: number;
  current_bpm: number;
  master_gain: number;
  backing_gain: number;
  click_gain: number;
  click_muted: boolean;
  click_mode: ClickMode;
  click_intro_end_bar: number;
  pad_enabled: boolean;
  pad_muted: boolean;
  pad_gain: number;
  pad_key: string;
  pad_is_minor: boolean;
  pad_follow_song_key: boolean;
  pad_preset_id: string | null;
  pad_preset_name: string;
  pad_layers: PadLayerConfig[];
  pad_brightness: number;
  pad_shimmer: number;
  pad_pan: number;
  pad_crossfade_sec: number;
  setlist_name: string;
  setlist_sections: SetlistSection[];
  setlist_items: SetlistItemSnapshot[];
  active_setlist_slot: number | null;
  song_crossfade_progress: number | null;
  crossfading_from_title: string | null;
  percussion_mode: PercussionMode;
  percussion_style: PercussionStyle;
  percussion_intensity: GrooveIntensity;
  percussion_follow_sections: boolean;
  percussion_gain: number;
  percussion_iem_send: number;
  percussion_room_mix: number;
  percussion_kit_name: string | null;
  percussion_kit_dir: string | null;
  percussion_custom_pattern: CustomPercussionPattern | null;
  percussion_current_step: number | null;
  percussion_audition_playing: boolean;
  percussion_audition_bpm: number;
  percussion_channels: PercussionChannelConfig[];
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
  | { type: 'KeyDial' }
  | { type: 'TempoUp' }
  | { type: 'TempoDown' }
  | { type: 'TempoDial' }
  | { type: 'NextSong' }
  | { type: 'PrevSong' }
  | { type: 'TogglePad' }
  | { type: 'ToggleClick' }
  | { type: 'PadGain' }
  | { type: 'PadBrightness' }
  | { type: 'PadShimmer' }
  | { type: 'PadCrossfade' }
  | { type: 'PadPresetNext' }
  | { type: 'PadPresetPrev' }
  | { type: 'PadKeyNext' }
  | { type: 'PadKeyPrev' }
  | { type: 'MasterGain' }
  | { type: 'BackingGain' }
  | { type: 'ClickGain' }
  | { type: 'CuesGain' }
  | { type: 'StemGain'; value: string }
  | { type: 'TogglePercussionMode' }
  | { type: 'PercussionGain' }
  | { type: 'PercussionIntensityUp' }
  | { type: 'PercussionIntensityDown' };

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

export interface SetlistBatchPatch {
  slots: number[];
  target_key?: string;
  key_shift_semitones?: number;
  bpm_override?: number | null;
  click_mode?: ClickMode | null;
  transition_mode?: TransitionMode;
  crossfade_sec?: number;
  ambient_pad_enabled?: boolean;
  pad_lock?: PadLockConfig | null;
  section_id?: string | null;
  percussion_mode?: PercussionMode;
  percussion_style?: PercussionStyle;
  percussion_intensity?: GrooveIntensity;
  percussion_custom_pattern?: CustomPercussionPattern | null;
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
  | { type: 'SeekSec'; payload: number }
  | { type: 'QueueSectionById'; payload: string }
  | { type: 'QueueSectionBySlot'; payload: number }
  | { type: 'QueueSectionByName'; payload: string }
  | { type: 'NextSection' }
  | { type: 'PrevSection' }
  | { type: 'ToggleLoop' }
  | { type: 'SetLoop'; payload: boolean }
  | { type: 'ToggleContinueTrack' }
  | { type: 'SetContinueTrack'; payload: boolean }
  | { type: 'SetQuantizeMode'; payload: QuantizeMode }
  | { type: 'SetKeyShift'; payload: number }
  | { type: 'AdjustKeyShift'; payload: number }
  | { type: 'SetBpm'; payload: number }
  | { type: 'AdjustBpm'; payload: number }
  | {
      type: 'SetSongTiming';
      payload: {
        bpm: number;
        first_downbeat_sec: number;
        beats_per_bar: number;
        click_mode?: ClickMode | null;
        sections?: SectionDef[] | null;
      };
    }
  | { type: 'SetMasterGain'; payload: number }
  | { type: 'SetBackingGain'; payload: number }
  | { type: 'SetClickGain'; payload: number }
  | { type: 'SetClickMuted'; payload: boolean }
  | { type: 'ToggleClickMuted' }
  | { type: 'SetClickMode'; payload: ClickMode }
  | { type: 'SetStemGain'; payload: { name: string; gain: number } }
  | { type: 'SetStemMuted'; payload: { name: string; muted: boolean } }
  | { type: 'SetPadEnabled'; payload: boolean }
  | { type: 'SetPadMuted'; payload: boolean }
  | { type: 'TogglePad' }
  | { type: 'SetPadGain'; payload: number }
  | { type: 'SetPadKey'; payload: string }
  | { type: 'AdjustPadKey'; payload: number }
  | { type: 'SetPadFollowSongKey'; payload: boolean }
  | { type: 'SetPadCrossfadeSec'; payload: number }
  | { type: 'SetPadPatch'; payload: PadPatchSpec }
  | { type: 'SetPadLayer'; payload: { index: number; layer: PadLayerConfig | null } }
  | { type: 'SetPadLayerGain'; payload: { index: number; gain: number } }
  | { type: 'SetPadBrightness'; payload: number }
  | { type: 'SetPadShimmer'; payload: number }
  | { type: 'SetPadPan'; payload: number }
  | { type: 'LockCurrentPadToSlot'; payload: number }
  | { type: 'ClearSlotPadLock'; payload: number }
  | { type: 'AddSetlistSong'; payload: { dir: string; insert_at?: number | null } }
  | { type: 'RemoveSetlistSlot'; payload: number }
  | { type: 'MoveSetlistSlot'; payload: { from: number; to: number } }
  | { type: 'SelectSetlistSlot'; payload: number }
  | { type: 'NextSong' }
  | { type: 'PrevSong' }
  | { type: 'SetSetlistName'; payload: string }
  | {
      type: 'UpdateSetlistSlotConfig';
      payload: {
        slot: number;
        key_shift_semitones?: number | null;
        bpm_override?: number | null | undefined;
        transition_mode?: TransitionMode | null;
        crossfade_sec?: number | null;
        ambient_pad_enabled?: boolean | null;
        pad_key_override?: string | null | undefined;
        pad_lock?: PadLockConfig | null | undefined;
        click_mode?: ClickMode | null | undefined;
        section_id?: string | null | undefined;
        percussion_lock?: PercussionLockConfig | null | undefined;
      };
    }
  | { type: 'BatchUpdateSetlistSlots'; payload: SetlistBatchPatch }
  | { type: 'UpsertSetlistSection'; payload: SetlistSection }
  | { type: 'RemoveSetlistSection'; payload: string }
  | { type: 'MoveSetlistSection'; payload: { id: string; to_index: number } }
  | { type: 'ToggleSetlistSectionCollapsed'; payload: { id: string; collapsed: boolean } }
  | {
      type: 'AddStandaloneSetlistSlot';
      payload: {
        title: string;
        slot_kind: SetlistSlotKind;
        key: string;
        bpm: number;
        time_signature: [number, number];
        section_id?: string | null;
        insert_at?: number | null;
      };
    }
  | { type: 'SetPercussionMode'; payload: PercussionMode }
  | { type: 'SetPercussionStyle'; payload: PercussionStyle }
  | { type: 'SetPercussionIntensity'; payload: GrooveIntensity }
  | { type: 'SetPercussionFollowSections'; payload: boolean }
  | { type: 'SetPercussionGain'; payload: number }
  | { type: 'SetPercussionIemSend'; payload: number }
  | { type: 'SetPercussionRoom'; payload: number }
  | { type: 'SetCustomPercussionPattern'; payload: CustomPercussionPattern | null }
  | { type: 'LoadPercussionKitDir'; payload: string | null }
  | {
      type: 'TriggerPercussionPreview';
      payload: {
        instrument: PercussionInstrument;
        variant: HitVariant;
        velocity: number;
      };
    }
  | {
      type: 'SetPercussionChannel';
      payload: {
        instrument: PercussionInstrument;
        enabled?: boolean | null;
        gain?: number | null;
        pan?: number | null;
      };
    }
  | { type: 'ResetPercussionMix' }
  | { type: 'LockCurrentPercussionToSlot'; payload: number }
  | { type: 'ClearSlotPercussionLock'; payload: number }
  | {
      type: 'StartPercussionAudition';
      payload: { bpm: number; beats_per_bar: number; with_click: boolean };
    }
  | { type: 'StopPercussionAudition' }
  | { type: 'SetPercussionAuditionBpm'; payload: number }
  | { type: 'LoadSetlist'; payload: SetlistManifest }
  | { type: 'ClearSetlist' }
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

export type WorkspaceMode = 'stage' | 'studio' | 'loops' | 'workbench';

export interface LibraryConfig {
  library_root: string;
  setlists_root?: string;
  linked_folders: string[];
  recent_setlists: string[];
}

export interface SetlistSummary {
  path: string;
  id: string;
  name: string;
  song_count: number;
  song_titles: string[];
  total_estimated_sec: number;
  modified_ms: number;
}

export interface AiHarnessInfo {
  cli_path: string;
  cli_available: boolean;
  mcp_config_json: string;
  skill_markdown: string;
  symlink_supported: boolean;
  symlink_path: string | null;
  symlink_installed: boolean;
}

export type LibraryEntryStatus = 'Ready' | 'NeedsSetup' | 'MissingStems';

export interface LibrarySongEntry {
  dir: string;
  status: LibraryEntryStatus;
  song_id: string;
  title: string;
  artist: string;
  original_key: string;
  bpm: number;
  time_signature: [number, number];
  estimated_duration_sec: number;
  section_names: string[];
  stem_names: string[];
  has_cues: boolean;
  missing_files: string[];
  provenance?: ImportProvenance | null;
}

export type SeparationEngine = 'None' | 'AudioSeparator' | 'Demucs';
export type StemPreset = 'TwoStems' | 'FourStems' | 'SixStems' | 'Custom';

export interface WorkbenchConfig {
  yt_dlp_path: string | null;
  ffmpeg_path: string | null;
  audio_separator_path: string | null;
  demucs_path: string | null;
  default_separation_engine: SeparationEngine;
  default_stem_preset: StemPreset;
  custom_uvr_model: string;
}

export interface ToolInfo {
  id: string;
  name: string;
  available: boolean;
  resolved_path: string | null;
  version: string | null;
  install_hint: string;
  purpose: string;
}

export interface ToolchainStatus {
  platform: string;
  ffmpeg: ToolInfo;
  yt_dlp: ToolInfo;
  audio_separator: ToolInfo;
  demucs: ToolInfo;
}

export interface YouTubeMetadata {
  title: string;
  uploader: string;
  duration_sec: number;
  webpage_url: string;
}

export type ImportSource =
  | { kind: 'LocalFile'; value: string }
  | { kind: 'YouTubeUrl'; value: string };

export interface ImportJobRequest {
  source: ImportSource;
  song_title: string | null;
  target_dir: string | null;
  separation_engine: SeparationEngine;
  stem_preset: StemPreset;
  custom_model: string | null;
  force_reextract?: boolean;
}

export type JobStatus = 'Running' | 'Completed' | 'Failed' | 'Cancelled';

export interface WorkbenchJobProgress {
  job_id: string;
  status: JobStatus;
  stage: string;
  progress_pct: number;
  message: string;
  log_line: string | null;
  output_dir: string | null;
}

export interface AudioAnalysisResult {
  source_file: string;
  duration_sec: number;
  sample_rate: number;
  channels: number;
  estimated_bpm: number;
  estimated_first_downbeat_sec: number;
  estimated_key?: string | null;
  key_confidence?: number;
  key_candidates?: string[];
  peaks: [number, number][];
}
