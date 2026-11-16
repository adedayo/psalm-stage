import { Injectable, OnDestroy, signal } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { open, save } from '@tauri-apps/plugin-dialog';
import {
  type AiHarnessInfo,
  type AppUpdateInfo,
  type AudioAnalysisResult,
  type AudioDeviceInfo,
  type CustomPercussionPattern,
  defaultPercussionChannels,
  type EngineCommand,
  type EngineSnapshot,
  type ExportLoopBundleRequest,
  type ExportLoopBundleResult,
  type GeneratedSectionCues,
  type GrooveIntensity,
  type ImportJobRequest,
  type ImportProvenance,
  type InspectedSongBundle,
  type LibraryConfig,
  type LibrarySongEntry,
  type MidiBinding,
  type MidiPortInfo,
  type MidiTarget,
  type MidiTriggerBehavior,
  type PadLayerConfig,
  type PadLockConfig,
  type PadPreset,
  type PercussionKitInfo,
  type PercussionLockConfig,
  type PercussionMode,
  type PercussionStyle,
  type RawMidiEvent,
  type SectionCueOptions,
  type SectionCuePlacement,
  type SectionCueTimingOverride,
  type SetlistManifest,
  type SetlistSection,
  type SetlistSummary,
  type SongManifest,
  type StudioKitProfile,
  type ToolchainStatus,
  type WorkbenchConfig,
  type WorkbenchJobProgress,
  type YouTubeMetadata,
} from '../models/engine.models';
import { serializeSetlistManifest } from '../models/setlist.helpers';

export function canonicalizeSourceRef(kind: 'youtube' | 'local_file', raw: string): string {
  const trimmed = raw.trim();
  if (kind === 'youtube') {
    const vMatch = /[?&]v=([A-Za-z0-9_-]+)/.exec(trimmed);
    if (vMatch?.[1]) return `youtube:${vMatch[1]}`;
    const shortMatch = /(?:youtu\.be\/|\/shorts\/|\/embed\/|\/live\/)([A-Za-z0-9_-]+)/i.exec(trimmed);
    if (shortMatch?.[1]) return `youtube:${shortMatch[1]}`;
    return `youtube:${trimmed.replace(/\/+$/, '').toLowerCase()}`;
  }
  return `file:${trimmed}`;
}

export const CHROMATIC_KEYS = [
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
] as const;

export const SONG_KEY_OPTIONS = [
  'Unknown',
  ...CHROMATIC_KEYS,
  ...CHROMATIC_KEYS.map((k) => `${k}m`),
] as const;

export function transposeKeyName(baseKey: string, semitones: number): string {
  const trimmed = baseKey.trim();
  if (!trimmed || trimmed.toLowerCase() === 'unknown') {
    const label = trimmed || 'Unknown';
    if (semitones === 0) return label;
    return `${label} (${semitones > 0 ? `+${semitones}` : semitones})`;
  }
  const isMinor = trimmed.endsWith('m') && !trimmed.toLowerCase().endsWith('maj');
  const rootRaw = (isMinor ? trimmed.slice(0, -1) : trimmed).trim();
  const alias: Record<string, string> = {
    Db: 'C#',
    'D#': 'Eb',
    Gb: 'F#',
    'G#': 'Ab',
    'A#': 'Bb',
  };
  const norm = alias[rootRaw] ?? rootRaw;
  const idx = CHROMATIC_KEYS.indexOf(norm as (typeof CHROMATIC_KEYS)[number]);
  if (idx < 0) return trimmed;
  const shifted = (((idx + semitones) % 12) + 12) % 12;
  return isMinor ? `${CHROMATIC_KEYS[shifted]}m` : CHROMATIC_KEYS[shifted];
}

export function computeKeySemitoneShift(originalKey: string, targetKey: string): number | null {
  const normRoot = (raw: string): number => {
    const trimmed = raw.trim();
    if (!trimmed || trimmed.toLowerCase() === 'unknown') return -1;
    const isMinor = trimmed.endsWith('m') && !trimmed.toLowerCase().endsWith('maj');
    const rootRaw = (isMinor ? trimmed.slice(0, -1) : trimmed).trim();
    const alias: Record<string, string> = {
      Db: 'C#',
      'D#': 'Eb',
      Gb: 'F#',
      'G#': 'Ab',
      'A#': 'Bb',
    };
    const norm = alias[rootRaw] ?? rootRaw;
    return CHROMATIC_KEYS.indexOf(norm as (typeof CHROMATIC_KEYS)[number]);
  };
  const origIdx = normRoot(originalKey);
  const targetIdx = normRoot(targetKey);
  if (origIdx < 0 || targetIdx < 0) return null;
  let diff = targetIdx - origIdx;
  if (diff > 6) diff -= 12;
  if (diff < -6) diff += 12;
  return diff;
}

const PREVIEW_LIBRARY_ENTRIES: LibrarySongEntry[] = [
  {
    dir: '/library/Goodness of God',
    status: 'Ready',
    song_id: 'goodness-of-god',
    title: 'Goodness of God',
    artist: 'Bethel Music',
    original_key: 'Ab',
    bpm: 63,
    time_signature: [4, 4],
    estimated_duration_sec: 294,
    section_names: ['Intro', 'Verse 1', 'Chorus', 'Verse 2', 'Bridge', 'Outro'],
    stem_names: ['drums', 'bass', 'keys', 'eg', 'vocals', 'cues'],
    has_cues: true,
    missing_files: [],
  },
  {
    dir: '/library/Gratitude',
    status: 'Ready',
    song_id: 'gratitude',
    title: 'Gratitude',
    artist: 'Brandon Lake',
    original_key: 'B',
    bpm: 78,
    time_signature: [4, 4],
    estimated_duration_sec: 312,
    section_names: ['Intro', 'Verse 1', 'Chorus', 'Bridge', 'Tag', 'Outro'],
    stem_names: ['drums', 'bass', 'keys', 'ag', 'cues'],
    has_cues: true,
    missing_files: [],
  },
  {
    dir: '/library/Living Hope',
    status: 'Ready',
    song_id: 'living-hope',
    title: 'Living Hope',
    artist: 'Phil Wickham',
    original_key: 'Eb',
    bpm: 72,
    time_signature: [4, 4],
    estimated_duration_sec: 280,
    section_names: ['Intro', 'Verse 1', 'Pre-Chorus', 'Chorus', 'Bridge', 'Outro'],
    stem_names: ['drums', 'bass', 'piano', 'eg', 'cues'],
    has_cues: true,
    missing_files: [],
  },
  {
    dir: '/library/Praise',
    status: 'Ready',
    song_id: 'praise',
    title: 'Praise',
    artist: 'Elevation Worship',
    original_key: 'A',
    bpm: 127,
    time_signature: [4, 4],
    estimated_duration_sec: 262,
    section_names: ['Intro', 'Verse 1', 'Chorus', 'Bridge', 'Outro'],
    stem_names: ['drums', 'bass', 'synths', 'eg', 'cues'],
    has_cues: true,
    missing_files: [],
  },
  {
    dir: '/library/New YouTube Import',
    status: 'NeedsSetup',
    song_id: 'new-youtube-import',
    title: 'New YouTube Import',
    artist: '',
    original_key: 'C',
    bpm: 120,
    time_signature: [4, 4],
    estimated_duration_sec: 245,
    section_names: [],
    stem_names: ['drums', 'bass', 'vocals', 'music'],
    has_cues: false,
    missing_files: [],
  },
];

const PREVIEW_TOOLCHAIN: ToolchainStatus = {
  platform: 'macos',
  ffmpeg: {
    id: 'ffmpeg',
    name: 'FFmpeg',
    available: true,
    resolved_path: '/opt/homebrew/bin/ffmpeg',
    version: 'ffmpeg 7.1 (Preview)',
    install_hint: 'brew install ffmpeg',
    purpose: 'Audio decoding, format conversion, and YouTube audio extraction',
  },
  yt_dlp: {
    id: 'yt_dlp',
    name: 'yt-dlp',
    available: true,
    resolved_path: '/opt/homebrew/bin/yt-dlp',
    version: '2025.02.19 (Preview)',
    install_hint: 'brew install yt-dlp',
    purpose: 'Downloads audio from YouTube links into the Workbench',
  },
  audio_separator: {
    id: 'audio_separator',
    name: 'UVR (audio-separator)',
    available: false,
    resolved_path: null,
    version: null,
    install_hint: 'pipx install "audio-separator[cpu]"',
    purpose: 'Local UVR neural stem separation (MDX-Net, Roformer, Demucs)',
  },
  demucs: {
    id: 'demucs',
    name: 'Demucs',
    available: false,
    resolved_path: null,
    version: null,
    install_hint: 'pipx install demucs',
    purpose: 'Alternative local 4-stem / 6-stem AI separator',
  },
};

export const DEFAULT_PAD_LAYERS: PadLayerConfig[] = [
  { enabled: true, archetype: 'WarmAnalog', register: 'Mid', tonality: 'Auto', gain: 0.85 },
  { enabled: false, archetype: 'WarmAnalog', register: 'Mid', tonality: 'Auto', gain: 0.75 },
  { enabled: false, archetype: 'WarmAnalog', register: 'Mid', tonality: 'Auto', gain: 0.75 },
  { enabled: false, archetype: 'WarmAnalog', register: 'Mid', tonality: 'Auto', gain: 0.75 },
];

export const PREVIEW_FACTORY_PAD_PRESETS: PadPreset[] = [
  {
    id: 'classic-foundation',
    name: 'Classic Foundation',
    category: 'Classic',
    is_factory: true,
    layers: [{ enabled: true, archetype: 'WarmAnalog', register: 'Mid', tonality: 'Auto', gain: 0.85 }],
    brightness: 0.55,
    shimmer: 0.2,
    pan: 0,
  },
  {
    id: 'classic-bright',
    name: 'Classic Bright',
    category: 'Classic',
    is_factory: true,
    layers: [
      { enabled: true, archetype: 'BrightSynth', register: 'Mid', tonality: 'Auto', gain: 0.8 },
      { enabled: true, archetype: 'AiryChoir', register: 'High', tonality: 'OpenFifth', gain: 0.4 },
    ],
    brightness: 0.82,
    shimmer: 0.45,
    pan: 0,
  },
  {
    id: 'classic-warm',
    name: 'Classic Warm',
    category: 'Classic',
    is_factory: true,
    layers: [
      { enabled: true, archetype: 'WarmAnalog', register: 'Low', tonality: 'OpenFifth', gain: 0.65 },
      { enabled: true, archetype: 'WarmAnalog', register: 'Mid', tonality: 'Auto', gain: 0.8 },
    ],
    brightness: 0.36,
    shimmer: 0.12,
    pan: 0,
  },
  {
    id: 'reverse-piano',
    name: 'Reverse Piano',
    category: 'Motion',
    is_factory: true,
    layers: [
      { enabled: true, archetype: 'ReversePiano', register: 'Mid', tonality: 'Auto', gain: 0.85 },
      { enabled: true, archetype: 'ReversePiano', register: 'High', tonality: 'Melodic', gain: 0.45 },
      { enabled: true, archetype: 'WarmAnalog', register: 'Mid', tonality: 'OpenFifth', gain: 0.45 },
    ],
    brightness: 0.68,
    shimmer: 0.52,
    pan: 0,
  },
  {
    id: 'minor-lament',
    name: 'Minor Lament',
    category: 'Minor',
    is_factory: true,
    layers: [
      { enabled: true, archetype: 'StringsCello', register: 'Mid', tonality: 'Minor', gain: 0.8 },
      { enabled: true, archetype: 'WarmAnalog', register: 'Low', tonality: 'OpenFifth', gain: 0.65 },
      { enabled: true, archetype: 'AiryChoir', register: 'High', tonality: 'Minor', gain: 0.45 },
    ],
    brightness: 0.44,
    shimmer: 0.36,
    pan: 0,
  },
];

const DEMO_FALLBACK_SNAPSHOT: EngineSnapshot = {
  song_id: 'demo-praise',
  active_song_dir: null,
  song_title: 'Psalm 23 Demo (Browser Preview)',
  original_key: 'G',
  current_key: 'G',
  is_playing: false,
  current_bar: 1,
  current_beat: 1,
  beats_per_bar: 4,
  current_position_sec: 0,
  total_duration_sec: 106.7,
  active_section_id: 'intro',
  armed_section_id: null,
  loop_enabled: false,
  continue_until_stopped: false,
  quantize_mode: 'NextBar',
  key_shift_semitones: 0,
  base_bpm: 72,
  current_bpm: 72,
  master_gain: 1.0,
  backing_gain: 1.0,
  click_gain: 0.85,
  click_muted: false,
  click_mode: 'AlwaysOn',
  click_intro_end_bar: 5,
  pad_enabled: false,
  pad_muted: false,
  pad_gain: 0.45,
  pad_key: 'G',
  pad_is_minor: false,
  pad_follow_song_key: true,
  pad_preset_id: 'classic-foundation',
  pad_preset_name: 'Classic Foundation',
  pad_layers: DEFAULT_PAD_LAYERS.map((l) => ({ ...l })),
  pad_brightness: 0.55,
  pad_shimmer: 0.2,
  pad_pan: 0,
  pad_crossfade_sec: 4.0,
  setlist_name: 'Sunday Morning Set',
  setlist_sections: [],
  setlist_items: [],
  active_setlist_slot: null,
  song_crossfade_progress: null,
  crossfading_from_title: null,
  percussion_mode: 'Off',
  percussion_style: 'Worship44',
  percussion_intensity: 'Medium',
  percussion_follow_sections: true,
  percussion_gain: 0.9,
  percussion_iem_send: 0.0,
  percussion_room_mix: 0.24,
  percussion_kit_name: null,
  percussion_kit_dir: null,
  percussion_custom_pattern: null,
  percussion_current_step: null,
  percussion_audition_playing: false,
  percussion_audition_bpm: 120,
  percussion_channels: defaultPercussionChannels(),
  routing_mode: 'SplitLeftRight',
  device_name: 'Browser Preview (Run via Tauri for native audio)',
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

export const PREVIEW_SESSION_STORAGE_KEY = 'psalm-stage.preview-session.v1';

function loadInitialPreviewSnapshot(): EngineSnapshot {
  if (
    typeof window === 'undefined' ||
    '__TAURI_INTERNALS__' in window ||
    !window.localStorage
  ) {
    return DEMO_FALLBACK_SNAPSHOT;
  }
  try {
    const raw = window.localStorage.getItem(PREVIEW_SESSION_STORAGE_KEY);
    if (!raw) return DEMO_FALLBACK_SNAPSHOT;
    const parsed = JSON.parse(raw) as Partial<EngineSnapshot> | null;
    if (!parsed || typeof parsed !== 'object') return DEMO_FALLBACK_SNAPSHOT;
    return {
      ...DEMO_FALLBACK_SNAPSHOT,
      ...parsed,
      setlist_sections: Array.isArray(parsed.setlist_sections)
        ? parsed.setlist_sections
        : DEMO_FALLBACK_SNAPSHOT.setlist_sections,
      setlist_items: Array.isArray(parsed.setlist_items)
        ? parsed.setlist_items
        : DEMO_FALLBACK_SNAPSHOT.setlist_items,
      pad_layers: Array.isArray(parsed.pad_layers)
        ? parsed.pad_layers
        : DEMO_FALLBACK_SNAPSHOT.pad_layers,
      percussion_channels: Array.isArray(parsed.percussion_channels)
        ? parsed.percussion_channels
        : DEMO_FALLBACK_SNAPSHOT.percussion_channels,
      sections: Array.isArray(parsed.sections)
        ? parsed.sections
        : DEMO_FALLBACK_SNAPSHOT.sections,
      stems: Array.isArray(parsed.stems) ? parsed.stems : DEMO_FALLBACK_SNAPSHOT.stems,
      // Always boot stopped & pad disengaged for safety on reload
      is_playing: false,
      percussion_audition_playing: false,
      pad_enabled: false,
      armed_section_id: null,
      song_crossfade_progress: null,
      crossfading_from_title: null,
    };
  } catch {
    return DEMO_FALLBACK_SNAPSHOT;
  }
}

export const PREVIEW_LOOPS_STORAGE_KEY = 'psalm-stage.preview-loops.v1';

function loadPreviewCustomLoops(): CustomPercussionPattern[] {
  if (
    typeof window === 'undefined' ||
    '__TAURI_INTERNALS__' in window ||
    !window.localStorage
  ) {
    return [];
  }
  try {
    const raw = window.localStorage.getItem(PREVIEW_LOOPS_STORAGE_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? (parsed as CustomPercussionPattern[]) : [];
  } catch {
    return [];
  }
}

function persistPreviewCustomLoops(loops: CustomPercussionPattern[]): void {
  if (typeof window === 'undefined' || !window.localStorage) return;
  try {
    window.localStorage.setItem(PREVIEW_LOOPS_STORAGE_KEY, JSON.stringify(loops));
  } catch {
    // Ignore quota errors gracefully.
  }
}

function persistPreviewSnapshot(snap: EngineSnapshot): void {
  if (typeof window === 'undefined' || !window.localStorage) return;
  try {
    window.localStorage.setItem(PREVIEW_SESSION_STORAGE_KEY, JSON.stringify(snap));
  } catch {
    // Ignore storage quota / private mode failures gracefully.
  }
}

@Injectable({ providedIn: 'root' })
export class PsalmStageService implements OnDestroy {
  readonly isTauriRuntime = signal<boolean>(
    typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window,
  );

  readonly snapshot = signal<EngineSnapshot>(loadInitialPreviewSnapshot());
  readonly audioDevices = signal<AudioDeviceInfo[]>([]);
  readonly scanningOutputs = signal<boolean>(false);
  readonly newlyDetectedOutput = signal<AudioDeviceInfo | null>(null);
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
  readonly APP_VERSION = '0.2.0'; // x-release-please-version
  readonly availableUpdate = signal<AppUpdateInfo | null>(null);
  readonly isCheckingUpdate = signal<boolean>(false);
  readonly updateDismissed = signal<boolean>(false);

  // Song Library & Studio state
  readonly libraryConfig = signal<LibraryConfig>({
    library_root: '~/Music/PsalmStage/Library',
    setlists_root: '~/Music/PsalmStage/Setlists',
    linked_folders: [],
    recent_setlists: [],
  });
  readonly libraryEntries = signal<LibrarySongEntry[]>(
    typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
      ? []
      : PREVIEW_LIBRARY_ENTRIES,
  );
  readonly libraryScanning = signal<boolean>(false);
  readonly managedSetlists = signal<SetlistSummary[]>([]);
  readonly activeSetlistPath = signal<string | null>(null);
  readonly aiHarnessInfo = signal<AiHarnessInfo | null>(
    typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
      ? null
      : {
          cli_path: 'psalm-stage-cli',
          cli_available: true,
          mcp_config_json: JSON.stringify(
            {
              mcpServers: {
                'psalm-stage': {
                  command: 'psalm-stage-cli',
                  args: ['mcp'],
                },
              },
            },
            null,
            2,
          ),
          skill_markdown: '# Psalm Stage Sunday Setlist Workflow\nRun plan_sunday_setlist first.',
          symlink_supported: true,
          symlink_path: '~/.local/bin/psalm-stage-cli',
          symlink_installed: false,
        },
  );
  readonly externalSyncNotice = signal<string | null>(null);
  readonly liveLockActive = signal<boolean>(false);
  readonly screenWakeLockActive = signal<boolean>(false);
  readonly padPresets = signal<PadPreset[]>(PREVIEW_FACTORY_PAD_PRESETS);
  readonly percussionKits = signal<PercussionKitInfo[]>([]);
  readonly percussionLoops = signal<CustomPercussionPattern[]>(loadPreviewCustomLoops());

  // Optional Import & Stem Separation Workbench state
  readonly workbenchConfig = signal<WorkbenchConfig>({
    yt_dlp_path: null,
    ffmpeg_path: null,
    audio_separator_path: null,
    demucs_path: null,
    default_separation_engine: 'None',
    default_stem_preset: 'FourStems',
    custom_uvr_model: 'htdemucs_ft.yaml',
  });
  readonly toolchainStatus = signal<ToolchainStatus | null>(
    typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window ? null : PREVIEW_TOOLCHAIN,
  );
  readonly workbenchJob = signal<WorkbenchJobProgress | null>(null);
  readonly workbenchLogs = signal<string[]>([]);

  private unlistenFns: UnlistenFn[] = [];
  private wakeLockSentinel: { release: () => Promise<void> } | null = null;
  private visibilityListener: (() => void) | null = null;
  private wakeLockEpoch = 0;
  private lastKnownOutputNames = new Set<string>();
  private outputInventoryInitialized = false;

  async setLiveKeepAwake(enabled: boolean): Promise<void> {
    const epoch = ++this.wakeLockEpoch;
    if (enabled) {
      let active = false;
      if (this.isTauriRuntime()) {
        try {
          active = await invoke<boolean>('set_live_keep_awake', { enabled: true });
        } catch {
          active = false;
        }
      }
      if (epoch !== this.wakeLockEpoch) {
        if (this.isTauriRuntime()) {
          await invoke<boolean>('set_live_keep_awake', { enabled: false }).catch(() => false);
        }
        return;
      }
      if (typeof navigator !== 'undefined' && 'wakeLock' in navigator) {
        try {
          const sentinel = await (
            navigator as unknown as {
              wakeLock: { request: (type: 'screen') => Promise<{ release: () => Promise<void> }> };
            }
          ).wakeLock.request('screen');
          if (epoch !== this.wakeLockEpoch) {
            await sentinel.release().catch(() => undefined);
            return;
          }
          this.wakeLockSentinel = sentinel;
          active = true;
        } catch {
          // Web WakeLock unsupported or denied in background tab
        }
      }
      if (typeof document !== 'undefined' && !this.visibilityListener) {
        this.visibilityListener = () => {
          if (document.visibilityState === 'visible' && this.liveLockActive()) {
            void this.reacquireWebWakeLock();
          }
        };
        document.addEventListener('visibilitychange', this.visibilityListener);
      }
      this.screenWakeLockActive.set(active || this.isTauriRuntime());
      return;
    }

    this.screenWakeLockActive.set(false);
    if (typeof document !== 'undefined' && this.visibilityListener) {
      document.removeEventListener('visibilitychange', this.visibilityListener);
      this.visibilityListener = null;
    }
    if (this.wakeLockSentinel) {
      const sentinel = this.wakeLockSentinel;
      this.wakeLockSentinel = null;
      await sentinel.release().catch(() => undefined);
    }
    if (this.isTauriRuntime()) {
      await invoke<boolean>('set_live_keep_awake', { enabled: false }).catch(() => false);
    }
  }

  private async reacquireWebWakeLock(): Promise<void> {
    if (typeof navigator === 'undefined' || !('wakeLock' in navigator) || this.wakeLockSentinel) {
      return;
    }
    try {
      const sentinel = await (
        navigator as unknown as {
          wakeLock: { request: (type: 'screen') => Promise<{ release: () => Promise<void> }> };
        }
      ).wakeLock.request('screen');
      this.wakeLockSentinel = sentinel;
    } catch {
      // Ignore transient visibility error
    }
  }

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
      const res = await fetch('https://api.github.com/repos/adedayo/psalm-stage/releases/latest', {
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
          url: data.html_url ?? 'https://github.com/adedayo/psalm-stage/releases',
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
      this.applyAudioDevices(devices, false);
      this.midiPorts.set(ports);
      this.connectedMidiPort.set(connected);
      this.midiBindings.set(bindings);

      const unlistenSnap = await listen<EngineSnapshot>('engine:snapshot', (event) => {
        this.snapshot.set(event.payload);
      });

      const unlistenAudio = await listen<AudioDeviceInfo[]>('audio:devices', (event) => {
        this.applyAudioDevices(event.payload, false);
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

      const unlistenJob = await listen<WorkbenchJobProgress>('workbench:job-progress', (event) => {
        const progress = event.payload;
        this.workbenchJob.set(progress);
        if (progress.log_line) {
          this.workbenchLogs.update((logs) => [...logs.slice(-49), progress.log_line!]);
        }
        if (progress.status === 'Completed') {
          void this.refreshLibrary();
        }
      });

      const unlistenExternal = await listen('library:external-update', () => {
        if (!this.liveLockActive()) {
          void this.refreshLibrary(true);
        }
      });

      this.unlistenFns.push(
        unlistenSnap,
        unlistenAudio,
        unlistenMidi,
        unlistenPorts,
        unlistenConnected,
        unlistenJob,
        unlistenExternal,
      );
      void this.refreshLibrary();
      void this.loadWorkbenchState();
      void this.loadAiHarnessInfo();
      void this.loadPadPresets();
      void this.loadPercussionKits();
      void this.loadPercussionLoops();
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

  async pickSongFolder(): Promise<string | null> {
    if (!this.isTauriRuntime()) return null;
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select Song Stem Folder',
      });
      return typeof selected === 'string' ? selected : null;
    } catch (err) {
      this.statusMessage.set(`Folder picker error: ${String(err)}`);
      return null;
    }
  }

  async pickOpenSetlistFile(): Promise<string | null> {
    if (!this.isTauriRuntime()) return null;
    try {
      const selected = await open({
        multiple: false,
        title: 'Open Setlist JSON',
        filters: [{ name: 'Psalm Stage Setlist', extensions: ['json'] }],
      });
      return typeof selected === 'string' ? selected : null;
    } catch (err) {
      this.statusMessage.set(`Open setlist error: ${String(err)}`);
      return null;
    }
  }

  async pickSaveSetlistFile(defaultName = 'sunday-setlist.json'): Promise<string | null> {
    if (!this.isTauriRuntime()) return null;
    try {
      const selected = await save({
        title: 'Save Setlist JSON',
        defaultPath: defaultName,
        filters: [{ name: 'Psalm Stage Setlist', extensions: ['json'] }],
      });
      return typeof selected === 'string' ? selected : null;
    } catch (err) {
      this.statusMessage.set(`Save setlist error: ${String(err)}`);
      return null;
    }
  }

  async inspectSongFolder(dir: string): Promise<InspectedSongBundle | null> {
    if (!this.isTauriRuntime()) {
      const name = dir.split(/[/\\]/).filter(Boolean).pop() ?? 'Preview Song';
      return {
        dir,
        has_existing_manifest: false,
        estimated_duration_sec: 192,
        available_files: ['drums.wav', 'bass.wav', 'keys.wav', 'cues.wav'],
        manifest: {
          id: name.toLowerCase().replace(/[^a-z0-9]+/g, '-'),
          title: name,
          artist: '',
          bpm: 74,
          time_signature: [4, 4],
          first_downbeat_sec: 0,
          original_key: 'G',
          click_mode: 'AlwaysOn',
          sections: [
            { id: 'intro', name: 'Intro', start_bar: 1, end_bar: 5, color: 'slate' },
            { id: 'verse-1', name: 'Verse 1', start_bar: 5, end_bar: 13, color: 'sky' },
          ],
          stems: {
            drums: { file: 'drums.wav', pitched: false, is_cues: false, default_gain: 0.95 },
            bass: { file: 'bass.wav', pitched: true, is_cues: false, default_gain: 0.9 },
            keys: { file: 'keys.wav', pitched: true, is_cues: false, default_gain: 0.85 },
            cues: { file: 'cues.wav', pitched: false, is_cues: true, default_gain: 0.85 },
          },
        },
      };
    }
    try {
      return await invoke<InspectedSongBundle>('inspect_song_folder', { dir });
    } catch (err) {
      this.statusMessage.set(`Inspect error: ${String(err)}`);
      return null;
    }
  }

  async saveSongManifest(
    dir: string,
    manifest: SongManifest,
    reloadIfActive: boolean,
  ): Promise<boolean> {
    if (!this.isTauriRuntime()) {
      this.statusMessage.set(`Preview saved manifest for ${manifest.title}`);
      return true;
    }
    try {
      await invoke('save_song_manifest', { dir, manifest, reloadIfActive });
      this.statusMessage.set(`Saved manifest.json for "${manifest.title}"`);
      return true;
    } catch (err) {
      this.statusMessage.set(`Save manifest error: ${String(err)}`);
      return false;
    }
  }

  async generateSectionCues(
    dir: string,
    timingOverride: SectionCueTimingOverride | null,
    options: Partial<SectionCueOptions> = {},
    reloadIfActive = true,
    currentManifest?: SongManifest | null,
  ): Promise<GeneratedSectionCues | null> {
    const resolvedOpts: SectionCueOptions = {
      lead_in_beats: options.lead_in_beats ?? 2,
      include_chime: options.include_chime ?? true,
      sample_rate: options.sample_rate ?? null,
      use_os_tts: options.use_os_tts ?? null,
    };
    if (!this.isTauriRuntime()) {
      const bpm = Math.max(30, Math.min(260, timingOverride?.bpm ?? currentManifest?.bpm ?? 120));
      const bpb = Math.max(
        1,
        Math.min(12, timingOverride?.beats_per_bar ?? currentManifest?.time_signature?.[0] ?? 4),
      );
      const firstDownbeat =
        timingOverride?.first_downbeat_sec ?? currentManifest?.first_downbeat_sec ?? 0;
      const sections = timingOverride?.sections ?? currentManifest?.sections ?? [];
      const beatSec = 60 / bpm;
      const leadInSec = Math.max(0, Math.min(8, resolvedOpts.lead_in_beats)) * beatSec;
      const secPerBar = beatSec * bpb;
      const placements: SectionCuePlacement[] = sections.map((sec) => {
        const secStart =
          sec.start_sec != null
            ? Math.max(0, sec.start_sec)
            : Math.max(0, firstDownbeat + (sec.start_bar - 1) * secPerBar);
        return {
          section_id: sec.id,
          section_name: sec.name,
          phrase: sec.name.trim() || 'Section',
          section_start_sec: secStart,
          cue_start_sec: Math.max(0, secStart - leadInSec),
        };
      });
      const existingCues = currentManifest?.stems?.['cues'];
      const existingFile =
        typeof existingCues === 'string' ? existingCues : existingCues?.file;
      const stemKey =
        existingFile && existingFile !== 'section-cues.wav' ? 'section_cues' : 'cues';
      const baseManifest: SongManifest = currentManifest
        ? {
            ...currentManifest,
            bpm,
            first_downbeat_sec: firstDownbeat,
            time_signature: [bpb, currentManifest.time_signature?.[1] ?? 4],
            sections: sections.map((s) => ({ ...s })),
            stems: {
              ...currentManifest.stems,
              [stemKey]: {
                file: 'section-cues.wav',
                pitched: false,
                is_cues: true,
                default_gain: 0.9,
                muted: false,
              },
            },
          }
        : {
            id: 'preview-song',
            title: 'Preview Song',
            artist: '',
            bpm,
            time_signature: [bpb, 4],
            first_downbeat_sec: firstDownbeat,
            original_key: 'G',
            click_mode: 'AlwaysOn',
            sections: sections.map((s) => ({ ...s })),
            stems: {
              [stemKey]: {
                file: 'section-cues.wav',
                pitched: false,
                is_cues: true,
                default_gain: 0.9,
                muted: false,
              },
            },
          };
      if (reloadIfActive) {
        this.snapshot.update((cur) => {
          const without = cur.stems.filter((s) => s.name !== stemKey);
          return {
            ...cur,
            stems: [
              ...without,
              { name: stemKey, pitched: false, is_cues: true, gain: 0.9, muted: false },
            ],
          };
        });
        persistPreviewSnapshot(this.snapshot());
      }
      const lastCue = placements[placements.length - 1]?.cue_start_sec ?? 0;
      return {
        stem_key: stemKey,
        file_name: 'section-cues.wav',
        used_os_tts: false,
        placements,
        duration_sec: lastCue + 1.0,
        manifest: baseManifest,
      };
    }

    try {
      const res = await invoke<GeneratedSectionCues>('generate_section_cues', {
        dir,
        timingOverride,
        options: resolvedOpts,
        reloadIfActive,
      });
      if (reloadIfActive) {
        const snap = await invoke<EngineSnapshot>('get_snapshot');
        this.snapshot.set(snap);
      }
      return res;
    } catch (err) {
      this.statusMessage.set(`Generate cues error: ${String(err)}`);
      return null;
    }
  }

  async saveSetlistToDisk(path: string, setlist: SetlistManifest): Promise<boolean> {
    if (!this.isTauriRuntime()) {
      this.activeSetlistPath.set(path);
      this.statusMessage.set(`Saved setlist "${setlist.name}"`);
      return true;
    }
    try {
      await invoke('save_setlist_file', { path, setlist });
      this.activeSetlistPath.set(path);
      this.statusMessage.set(`Saved setlist "${setlist.name}"`);
      void this.refreshManagedSetlists();
      return true;
    } catch (err) {
      this.statusMessage.set(`Save setlist error: ${String(err)}`);
      return false;
    }
  }

  async saveManagedSetlist(
    setlist: SetlistManifest,
    targetPath?: string | null,
  ): Promise<string | null> {
    if (!this.isTauriRuntime()) {
      const root = this.libraryConfig().setlists_root || '~/Music/PsalmStage/Setlists';
      const slug =
        setlist.name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, '-')
          .replace(/^-+|-+$/g, '') || 'sunday-set';
      const resolvedPath =
        targetPath && targetPath.trim().length > 0 ? targetPath : `${root}/${slug}.json`;
      const summary: SetlistSummary = {
        id: setlist.id || slug,
        name: setlist.name,
        path: resolvedPath,
        song_count: setlist.items.length,
        total_estimated_sec: setlist.items.length * 240,
        modified_ms: Date.now(),
        song_titles: setlist.items.map((it) => it.title || it.song_id).filter(Boolean),
      };
      this.managedSetlists.update((list) => [
        summary,
        ...list.filter((s) => s.path !== resolvedPath),
      ]);
      this.activeSetlistPath.set(resolvedPath);
      this.statusMessage.set(`Saved setlist "${setlist.name}"`);
      return resolvedPath;
    }
    try {
      const savedPath = await invoke<string>('save_managed_setlist', {
        setlist,
        path: targetPath ?? null,
      });
      this.activeSetlistPath.set(savedPath);
      this.statusMessage.set(`Saved setlist "${setlist.name}"`);
      await this.refreshManagedSetlists();
      return savedPath;
    } catch (err) {
      this.statusMessage.set(`Save setlist error: ${String(err)}`);
      return null;
    }
  }

  async createNewSetlist(options: {
    name: string;
    saveCurrent: boolean;
    saveAsCustomPath?: boolean;
  }): Promise<boolean> {
    if (this.liveLockActive()) {
      this.statusMessage.set('Unlock Live Lock before creating a new setlist.');
      return false;
    }
    const cur = this.snapshot();
    const hasContent = cur.setlist_items.length > 0 || cur.setlist_sections.length > 0;
    let savedLocation: string | null = null;

    if (options.saveCurrent && hasContent) {
      const currentManifest = serializeSetlistManifest(cur);
      if (options.saveAsCustomPath) {
        const picked = await this.pickSaveSetlistFile(`${currentManifest.id}.json`);
        if (!picked) {
          return false;
        }
        const ok = await this.saveSetlistToDisk(picked, currentManifest);
        if (!ok) return false;
        savedLocation = picked;
      } else {
        const path = await this.saveManagedSetlist(currentManifest, this.activeSetlistPath());
        if (!path) return false;
        savedLocation = path;
      }
    }

    const nextName = options.name.trim() || 'New Sunday Set';
    const nextId =
      nextName
        .toLowerCase()
        .replace(/[^a-z0-9]+/g, '-')
        .replace(/^-+|-+$/g, '') || 'sunday-set';
    const freshManifest: SetlistManifest = {
      id: nextId,
      name: nextName,
      sections: [],
      items: [],
    };
    await this.sendCommand({ type: 'LoadSetlist', payload: freshManifest });
    this.activeSetlistPath.set(null);
    this.dismissExternalSyncNotice();
    this.statusMessage.set(
      savedLocation
        ? `Saved "${cur.setlist_name}" and started fresh setlist "${nextName}".`
        : `Started fresh setlist "${nextName}".`,
    );
    return true;
  }

  async loadSetlistFromDisk(path: string): Promise<SetlistManifest | null> {
    if (!this.isTauriRuntime()) return null;
    try {
      const loaded = await invoke<SetlistManifest>('load_setlist_file', { path });
      this.activeSetlistPath.set(path);
      return loaded;
    } catch (err) {
      this.statusMessage.set(`Load setlist error: ${String(err)}`);
      return null;
    }
  }

  async refreshLibrary(fromExternalWatch = false): Promise<void> {
    if (!this.isTauriRuntime()) {
      return;
    }
    this.libraryScanning.set(true);
    try {
      const prevSetlists = new Set(this.managedSetlists().map((s) => `${s.path}:${s.modified_ms}`));
      const [cfg, entries, setlists] = await Promise.all([
        invoke<LibraryConfig>('get_library_config'),
        invoke<LibrarySongEntry[]>('scan_song_library'),
        invoke<SetlistSummary[]>('list_managed_setlists'),
      ]);
      this.libraryConfig.set(cfg);
      this.libraryEntries.set(entries);
      this.managedSetlists.set(setlists);
      if (fromExternalWatch && !this.liveLockActive()) {
        const newest = setlists.find((s) => !prevSetlists.has(`${s.path}:${s.modified_ms}`));
        if (newest) {
          this.externalSyncNotice.set(
            `Setlist "${newest.name}" (${newest.song_count} songs) updated on disk — ready to load.`,
          );
        } else {
          this.externalSyncNotice.set('Song Library updated from external tool.');
        }
      }
    } catch (err) {
      this.statusMessage.set(`Library scan error: ${String(err)}`);
    } finally {
      this.libraryScanning.set(false);
    }
  }

  async refreshManagedSetlists(): Promise<void> {
    if (!this.isTauriRuntime()) return;
    try {
      const setlists = await invoke<SetlistSummary[]>('list_managed_setlists');
      this.managedSetlists.set(setlists);
    } catch (err) {
      this.statusMessage.set(`Managed setlists error: ${String(err)}`);
    }
  }

  async loadAiHarnessInfo(): Promise<AiHarnessInfo | null> {
    if (!this.isTauriRuntime()) {
      return this.aiHarnessInfo();
    }
    try {
      const info = await invoke<AiHarnessInfo>('get_ai_harness_info');
      this.aiHarnessInfo.set(info);
      return info;
    } catch (err) {
      this.statusMessage.set(`AI harness info error: ${String(err)}`);
      return null;
    }
  }

  async installCliSymlink(): Promise<AiHarnessInfo | null> {
    if (!this.isTauriRuntime()) {
      const cur = this.aiHarnessInfo();
      if (cur) {
        const next = { ...cur, symlink_installed: true };
        this.aiHarnessInfo.set(next);
        return next;
      }
      return null;
    }
    try {
      const info = await invoke<AiHarnessInfo>('install_cli_symlink');
      this.aiHarnessInfo.set(info);
      this.statusMessage.set(`Installed CLI symlink at ${info.symlink_path ?? '~/.local/bin/psalm-stage-cli'}`);
      return info;
    } catch (err) {
      this.statusMessage.set(`CLI symlink error: ${String(err)}`);
      return null;
    }
  }

  dismissExternalSyncNotice(): void {
    this.externalSyncNotice.set(null);
  }

  async loadPadPresets(): Promise<PadPreset[]> {
    if (!this.isTauriRuntime()) {
      return this.padPresets();
    }
    try {
      const presets = await invoke<PadPreset[]>('list_pad_presets');
      this.padPresets.set(presets);
      return presets;
    } catch (err) {
      this.statusMessage.set(`Load pad presets error: ${String(err)}`);
      return this.padPresets();
    }
  }

  async saveUserPadPreset(preset: PadPreset): Promise<PadPreset[]> {
    if (!this.isTauriRuntime()) {
      const id =
        preset.id.trim() ||
        `user-${preset.name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, '-')
          .replace(/^-|-$/g, '')}-${Date.now().toString().slice(-4)}`;
      const saved: PadPreset = { ...preset, id, is_factory: false };
      this.padPresets.update((list) => [...list.filter((p) => p.id !== id), saved]);
      return this.padPresets();
    }
    try {
      const next = await invoke<PadPreset[]>('save_user_pad_preset', { preset });
      this.padPresets.set(next);
      return next;
    } catch (err) {
      this.statusMessage.set(`Save pad preset error: ${String(err)}`);
      return this.padPresets();
    }
  }

  async deleteUserPadPreset(id: string): Promise<PadPreset[]> {
    if (!this.isTauriRuntime()) {
      this.padPresets.update((list) => list.filter((p) => p.id !== id || p.is_factory));
      return this.padPresets();
    }
    try {
      const next = await invoke<PadPreset[]>('delete_user_pad_preset', { id });
      this.padPresets.set(next);
      return next;
    } catch (err) {
      this.statusMessage.set(`Delete pad preset error: ${String(err)}`);
      return this.padPresets();
    }
  }

  async loadPercussionKits(): Promise<PercussionKitInfo[]> {
    if (!this.isTauriRuntime()) {
      return this.percussionKits();
    }
    try {
      const kits = await invoke<PercussionKitInfo[]>('list_percussion_kits');
      this.percussionKits.set(kits);
      return kits;
    } catch (err) {
      this.statusMessage.set(`Load percussion kits error: ${String(err)}`);
      return this.percussionKits();
    }
  }

  async generateStudioPercussionKit(
    profile: StudioKitProfile,
  ): Promise<PercussionKitInfo | null> {
    if (!this.isTauriRuntime()) {
      const mockKit: PercussionKitInfo = {
        id: profile === 'MakossaLive' ? 'makossa-live-kit' : profile === 'AcousticCajon' ? 'acoustic-cajon-kit' : 'studio-praise-kit',
        name: profile === 'MakossaLive' ? 'Makossa African Live Kit' : profile === 'AcousticCajon' ? 'Acoustic Cajon & Shakers Kit' : 'Studio Praise Kit',
        dir: `~/Music/PsalmStage/PercussionKits/${profile}`,
        sample_count: 28,
        instruments_covered: [
          'Kick',
          'Snare',
          'HiHat',
          'HandDrums',
          'Tambourine',
          'Shaker',
          'Claps',
          'Clave',
          'Bell',
        ],
      };
      this.percussionKits.update((list) => [
        ...list.filter((k) => k.id !== mockKit.id),
        mockKit,
      ]);
      return mockKit;
    }
    try {
      const info = await invoke<PercussionKitInfo>('generate_studio_percussion_kit', { profile });
      await this.loadPercussionKits();
      this.statusMessage.set(`Generated ${info.name} (${info.sample_count} WAV samples)`);
      return info;
    } catch (err) {
      this.statusMessage.set(`Generate percussion kit error: ${String(err)}`);
      return null;
    }
  }

  async loadPercussionLoops(): Promise<CustomPercussionPattern[]> {
    if (!this.isTauriRuntime()) {
      return this.percussionLoops();
    }
    try {
      const loops = await invoke<CustomPercussionPattern[]>('list_percussion_loops');
      this.percussionLoops.set(loops);
      return loops;
    } catch (err) {
      this.statusMessage.set(`Load percussion loops error: ${String(err)}`);
      return this.percussionLoops();
    }
  }

  async savePercussionLoop(
    pattern: CustomPercussionPattern,
  ): Promise<CustomPercussionPattern[]> {
    if (!this.isTauriRuntime()) {
      const id =
        pattern.id.trim() ||
        pattern.name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, '-')
          .replace(/^-|-$/g, '') ||
        `loop-${Date.now().toString().slice(-4)}`;
      const saved: CustomPercussionPattern = { ...pattern, id };
      this.percussionLoops.update((list) => {
        const next = [...list.filter((l) => l.id !== id), saved];
        persistPreviewCustomLoops(next);
        return next;
      });
      return this.percussionLoops();
    }
    try {
      const next = await invoke<CustomPercussionPattern[]>('save_percussion_loop', { pattern });
      this.percussionLoops.set(next);
      return next;
    } catch (err) {
      this.statusMessage.set(`Save percussion loop error: ${String(err)}`);
      return this.percussionLoops();
    }
  }

  async deletePercussionLoop(id: string): Promise<CustomPercussionPattern[]> {
    if (!this.isTauriRuntime()) {
      this.percussionLoops.update((list) => {
        const next = list.filter((l) => l.id !== id);
        persistPreviewCustomLoops(next);
        return next;
      });
      return this.percussionLoops();
    }
    try {
      const next = await invoke<CustomPercussionPattern[]>('delete_percussion_loop', { id });
      this.percussionLoops.set(next);
      return next;
    } catch (err) {
      this.statusMessage.set(`Delete percussion loop error: ${String(err)}`);
      return this.percussionLoops();
    }
  }

  async cloneStyleToCustomPattern(
    style: PercussionStyle,
    id: string,
    name: string,
  ): Promise<CustomPercussionPattern> {
    if (!this.isTauriRuntime()) {
      const isCompound = style === 'Woro' || style === 'Worship68';
      return {
        id,
        name,
        steps_per_bar: isCompound ? 12 : 16,
        bars_in_cycle: 1,
        swing: style === 'Makossa' ? 0.03 : 0.0,
        is_compound: isCompound,
        hits: [
          { step: 0, instrument: 'Kick', velocity: 0.92, variant: 'Standard', min_intensity: 'Low' },
          { step: 4, instrument: 'Snare', velocity: 0.85, variant: 'Standard', min_intensity: 'Medium' },
          { step: 4, instrument: 'Claps', velocity: 0.82, variant: 'Standard', min_intensity: 'Medium' },
          { step: 8, instrument: 'Kick', velocity: 0.88, variant: 'Standard', min_intensity: 'Low' },
          { step: 12, instrument: 'Snare', velocity: 0.85, variant: 'Standard', min_intensity: 'Medium' },
          { step: 12, instrument: 'Claps', velocity: 0.84, variant: 'Standard', min_intensity: 'Medium' },
          { step: 2, instrument: 'Tambourine', velocity: 0.72, variant: 'Accent', min_intensity: 'High' },
          { step: 10, instrument: 'Tambourine', velocity: 0.72, variant: 'Accent', min_intensity: 'High' },
        ],
      };
    }
    return invoke<CustomPercussionPattern>('clone_style_to_custom_pattern', { style, id, name });
  }

  async exportPercussionLoopBundle(
    request: ExportLoopBundleRequest,
  ): Promise<ExportLoopBundleResult | null> {
    if (!this.isTauriRuntime()) {
      const mockRes: ExportLoopBundleResult = {
        song_dir: `${this.libraryConfig().library_root}/${request.title.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`,
        title: request.title,
        bpm: request.bpm,
        duration_sec: (request.bars * request.time_signature[0] * 60) / request.bpm,
        stem_files: [
          'kick.wav',
          'snare.wav',
          'hihat.wav',
          'hand_drums.wav',
          'tambourine.wav',
          'shaker.wav',
          'claps.wav',
          'clave.wav',
          'bell.wav',
        ],
      };
      this.statusMessage.set(
        `Exported 9-stem loop bundle "${mockRes.title}" (${mockRes.stem_files.length} WAV stems)`,
      );
      return mockRes;
    }
    try {
      const res = await invoke<ExportLoopBundleResult>('export_percussion_loop_bundle', {
        request,
      });
      await this.refreshLibrary();
      this.statusMessage.set(
        `Exported ${res.stem_files.length}-stem loop "${res.title}" to Library`,
      );
      return res;
    } catch (err) {
      this.statusMessage.set(`Export loop bundle error: ${String(err)}`);
      return null;
    }
  }

  async ensureLibraryRoot(): Promise<string | null> {
    if (!this.isTauriRuntime()) {
      return this.libraryConfig().library_root;
    }
    try {
      const root = await invoke<string>('ensure_library_root');
      await this.refreshLibrary();
      return root;
    } catch (err) {
      this.statusMessage.set(`Library root error: ${String(err)}`);
      return null;
    }
  }

  async saveLibraryConfig(config: LibraryConfig): Promise<void> {
    if (!this.isTauriRuntime()) {
      this.libraryConfig.set(config);
      return;
    }
    try {
      const saved = await invoke<LibraryConfig>('save_library_config', { config });
      this.libraryConfig.set(saved);
      await this.refreshLibrary();
    } catch (err) {
      this.statusMessage.set(`Save library config error: ${String(err)}`);
    }
  }

  async linkLibraryFolder(folder: string): Promise<void> {
    const trimmed = folder.trim();
    if (!trimmed) return;
    const cur = this.libraryConfig();
    if (cur.linked_folders.includes(trimmed) || cur.library_root === trimmed) return;
    await this.saveLibraryConfig({
      ...cur,
      linked_folders: [...cur.linked_folders, trimmed],
    });
  }

  async unlinkLibraryFolder(folder: string): Promise<void> {
    const cur = this.libraryConfig();
    await this.saveLibraryConfig({
      ...cur,
      linked_folders: cur.linked_folders.filter((f) => f !== folder),
    });
  }

  async loadWorkbenchState(): Promise<void> {
    if (!this.isTauriRuntime()) return;
    try {
      const [cfg, tools] = await Promise.all([
        invoke<WorkbenchConfig>('get_workbench_config'),
        invoke<ToolchainStatus>('detect_workbench_tools'),
      ]);
      this.workbenchConfig.set(cfg);
      this.toolchainStatus.set(tools);
    } catch (err) {
      this.statusMessage.set(`Workbench init error: ${String(err)}`);
    }
  }

  async saveWorkbenchConfig(config: WorkbenchConfig): Promise<void> {
    if (!this.isTauriRuntime()) {
      this.workbenchConfig.set(config);
      return;
    }
    try {
      const saved = await invoke<WorkbenchConfig>('save_workbench_config', { config });
      this.workbenchConfig.set(saved);
      await this.detectWorkbenchTools();
    } catch (err) {
      this.statusMessage.set(`Save workbench config error: ${String(err)}`);
    }
  }

  async detectWorkbenchTools(): Promise<ToolchainStatus | null> {
    if (!this.isTauriRuntime()) {
      return this.toolchainStatus();
    }
    try {
      const status = await invoke<ToolchainStatus>('detect_workbench_tools');
      this.toolchainStatus.set(status);
      return status;
    } catch (err) {
      this.statusMessage.set(`Tool detection error: ${String(err)}`);
      return null;
    }
  }

  async pickAudioFile(): Promise<string | null> {
    if (!this.isTauriRuntime()) return null;
    try {
      const selected = await open({
        multiple: false,
        title: 'Select Audio File (WAV, MP3, FLAC, OGG, M4A)',
        filters: [
          {
            name: 'Audio Files',
            extensions: ['wav', 'mp3', 'flac', 'ogg', 'm4a', 'aac'],
          },
        ],
      });
      return typeof selected === 'string' ? selected : null;
    } catch (err) {
      this.statusMessage.set(`Audio picker error: ${String(err)}`);
      return null;
    }
  }

  async pickExecutableFile(toolLabel: string): Promise<string | null> {
    if (!this.isTauriRuntime()) return null;
    try {
      const selected = await open({
        multiple: false,
        title: `Locate ${toolLabel} Executable`,
      });
      return typeof selected === 'string' ? selected : null;
    } catch (err) {
      this.statusMessage.set(`File picker error: ${String(err)}`);
      return null;
    }
  }

  async probeYouTubeUrl(url: string): Promise<{ meta: YouTubeMetadata | null; error: string | null }> {
    if (!this.isTauriRuntime()) {
      return {
        meta: {
          title: 'YouTube Worship Track (Preview)',
          uploader: 'Worship Channel',
          duration_sec: 264,
          webpage_url: url,
        },
        error: null,
      };
    }
    try {
      const meta = await invoke<YouTubeMetadata>('probe_youtube_url', { url });
      return { meta, error: null };
    } catch (err) {
      const msg = String(err);
      this.statusMessage.set(`YouTube lookup error: ${msg}`);
      return { meta: null, error: msg };
    }
  }

  async startImportJob(request: ImportJobRequest): Promise<string | null> {
    this.workbenchLogs.set([]);
    if (!this.isTauriRuntime()) {
      const sourceKind = request.source.kind === 'YouTubeUrl' ? 'youtube' : 'local_file';
      const sourceRef = canonicalizeSourceRef(sourceKind, request.source.value);
      const cachedMatch = !request.force_reextract
        ? this.libraryEntries().find(
            (e) => e.status === 'Ready' && e.provenance?.source_ref === sourceRef,
          )
        : undefined;

      const jobId = `preview-job-${Date.now()}`;
      if (cachedMatch) {
        this.workbenchJob.set({
          job_id: jobId,
          status: 'Completed',
          stage: 'Loaded from Library Cache',
          progress_pct: 100,
          message: `Reused existing stems for "${cachedMatch.title}" from Library (skipped re-extraction).`,
          log_line: cachedMatch.dir,
          output_dir: cachedMatch.dir,
        });
        return jobId;
      }

      const previewTitle =
        request.song_title?.trim() ||
        (request.source.kind === 'LocalFile'
          ? request.source.value.split(/[/\\]/).pop()?.replace(/\.[^.]+$/, '') || 'Imported Track'
          : 'YouTube Worship Track');
      const dir = `/library/${previewTitle}`;
      const provenance: ImportProvenance = {
        source_kind: sourceKind,
        source_ref: sourceRef,
        original_source: request.source.value,
        separation_engine: request.separation_engine,
        stem_preset: request.stem_preset,
        custom_model: request.custom_model,
      };
      const stemNames =
        request.separation_engine === 'None'
          ? ['music']
          : request.stem_preset === 'TwoStems'
            ? ['vocals', 'instrumental']
            : request.stem_preset === 'SixStems'
              ? ['drums', 'bass', 'vocals', 'guitar', 'piano', 'other']
              : ['drums', 'bass', 'vocals', 'music'];

      this.workbenchJob.set({
        job_id: jobId,
        status: 'Completed',
        stage: 'Available in Library',
        progress_pct: 100,
        message: `Preview imported "${previewTitle}" and added to Library`,
        log_line: dir,
        output_dir: dir,
      });

      const nextEntry: LibrarySongEntry = {
        dir,
        status: 'Ready',
        song_id: previewTitle.toLowerCase().replace(/[^a-z0-9]+/g, '-'),
        title: previewTitle,
        artist: '',
        original_key: 'G',
        bpm: 76,
        time_signature: [4, 4],
        estimated_duration_sec: 240,
        section_names: ['Intro', 'Verse 1', 'Chorus'],
        stem_names: stemNames,
        has_cues: false,
        missing_files: [],
        provenance,
      };
      this.libraryEntries.update((list) => {
        const filtered = list.filter((e) => e.dir !== dir);
        return [nextEntry, ...filtered];
      });
      return jobId;
    }
    try {
      this.workbenchJob.set({
        job_id: 'pending',
        status: 'Running',
        stage: 'Starting',
        progress_pct: 2,
        message: 'Launching import pipeline...',
        log_line: null,
        output_dir: null,
      });
      return await invoke<string>('start_import_job', { request });
    } catch (err) {
      this.workbenchJob.set({
        job_id: 'failed',
        status: 'Failed',
        stage: 'Error',
        progress_pct: 0,
        message: String(err),
        log_line: String(err),
        output_dir: null,
      });
      return null;
    }
  }

  async cancelImportJob(): Promise<void> {
    if (!this.isTauriRuntime()) return;
    try {
      await invoke('cancel_import_job');
    } catch (err) {
      this.statusMessage.set(`Cancel job error: ${String(err)}`);
    }
  }

  async analyzeWaveform(
    path: string,
    isDir: boolean,
    numBuckets = 640,
    hintBpm?: number,
    beatsPerBar?: number,
    forceRefresh?: boolean,
  ): Promise<AudioAnalysisResult | null> {
    if (!this.isTauriRuntime()) {
      const peaks: [number, number][] = Array.from({ length: numBuckets }, (_, i) => {
        const env = 0.25 + 0.55 * Math.abs(Math.sin(i * 0.08)) * (0.6 + 0.4 * Math.cos(i * 0.023));
        return [-env, env] as [number, number];
      });
      return {
        source_file: path.split(/[/\\]/).pop() ?? 'audio.wav',
        duration_sec: 240,
        sample_rate: 48000,
        channels: 2,
        estimated_bpm: hintBpm ?? 76,
        estimated_first_downbeat_sec: 0.4,
        estimated_key: null,
        key_confidence: 0,
        key_candidates: [],
        peaks,
      };
    }
    try {
      return await invoke<AudioAnalysisResult>('analyze_audio_waveform', {
        path,
        isDir,
        numBuckets,
        hintBpm: hintBpm ?? null,
        beatsPerBar: beatsPerBar ?? null,
        forceRefresh: forceRefresh ?? null,
      });
    } catch (err) {
      this.statusMessage.set(`Waveform analysis error: ${String(err)}`);
      return null;
    }
  }

  applyAudioDevices(devices: AudioDeviceInfo[], fromManualScan = false): void {
    this.audioDevices.set(devices);
    const activeName = this.snapshot().device_name;
    const nextNames = new Set(devices.map((d) => d.name));

    if (!this.outputInventoryInitialized) {
      this.lastKnownOutputNames = nextNames;
      this.outputInventoryInitialized = true;
      if (fromManualScan) {
        this.statusMessage.set(`Scanned ${devices.length} audio output(s)`);
      }
      return;
    }

    const newlyAdded = devices.filter(
      (d) => !this.lastKnownOutputNames.has(d.name) && d.name !== activeName,
    );
    this.lastKnownOutputNames = nextNames;

    if (newlyAdded.length > 0) {
      const detected = newlyAdded[0];
      this.newlyDetectedOutput.set(detected);
      this.statusMessage.set(
        `New audio output detected: ${detected.name} (${detected.max_channels} ch)`,
      );
      return;
    }

    const currentDetected = this.newlyDetectedOutput();
    if (
      currentDetected &&
      (!nextNames.has(currentDetected.name) || currentDetected.name === activeName)
    ) {
      this.newlyDetectedOutput.set(null);
    }

    if (fromManualScan) {
      this.statusMessage.set(`Scanned ${devices.length} audio output(s)`);
    }
  }

  async scanAudioOutputs(): Promise<AudioDeviceInfo[]> {
    this.scanningOutputs.set(true);
    try {
      if (!this.isTauriRuntime()) {
        const current = this.audioDevices();
        this.applyAudioDevices(current, true);
        return current;
      }
      const devices = await invoke<AudioDeviceInfo[]>('list_audio_devices');
      this.applyAudioDevices(devices, true);
      return devices;
    } catch (err) {
      this.statusMessage.set(`Output scan error: ${String(err)}`);
      return this.audioDevices();
    } finally {
      this.scanningOutputs.set(false);
    }
  }

  async useDetectedOutput(deviceName?: string): Promise<void> {
    const target = deviceName ?? this.newlyDetectedOutput()?.name;
    if (!target) return;
    this.newlyDetectedOutput.set(null);
    await this.sendCommand({ type: 'SelectOutputDevice', payload: target });
    this.statusMessage.set(`Switched audio output to ${target}`);
  }

  dismissDetectedOutput(): void {
    this.newlyDetectedOutput.set(null);
  }

  async refreshHardware(): Promise<void> {
    if (!this.isTauriRuntime()) {
      await this.scanAudioOutputs();
      return;
    }
    try {
      const [, ports, connected] = await Promise.all([
        this.scanAudioOutputs(),
        invoke<MidiPortInfo[]>('list_midi_ports'),
        invoke<string | null>('connected_midi_port'),
      ]);
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

  bindingsFor(target: MidiTarget): MidiBinding[] {
    const key = JSON.stringify(target);
    return this.midiBindings().filter((b) => JSON.stringify(b.target) === key);
  }

  async unassignMidiTarget(target: MidiTarget): Promise<void> {
    const key = JSON.stringify(target);
    const matches = this.midiBindings().filter((b) => JSON.stringify(b.target) === key);
    if (this.listeningTarget() && JSON.stringify(this.listeningTarget()!.target) === key) {
      this.listeningTarget.set(null);
    }
    for (const binding of matches) {
      await this.deleteMidiBinding(binding.id);
    }
  }

  bindingLabelFor(target: MidiTarget): string | null {
    const match = this.midiBindings().find(
      (b) => JSON.stringify(b.target) === JSON.stringify(target),
    );
    if (!match) return null;
    const ch = match.channel ? `Ch${match.channel}` : 'Any';
    const kind =
      match.kind === 'Note'
        ? `Note ${match.number}`
        : match.kind === 'ControlChange'
          ? `CC ${match.number}`
          : match.kind === 'PitchBend'
            ? 'PitchBend'
            : `PC ${match.number}`;
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
          return {
            ...cur,
            is_playing: !cur.is_playing,
            pad_enabled: cur.is_playing ? false : cur.pad_enabled,
            percussion_audition_playing: !cur.is_playing ? false : cur.percussion_audition_playing,
          };
        case 'Play':
          return { ...cur, is_playing: true, percussion_audition_playing: false };
        case 'Stop':
          return {
            ...cur,
            is_playing: false,
            pad_enabled: false,
            armed_section_id: null,
            current_position_sec: 0,
          };
        case 'SeekSec': {
          const clamped = Math.max(0, Math.min(cur.total_duration_sec, command.payload));
          const spb = (60 / Math.max(30, cur.current_bpm)) * Math.max(1, cur.beats_per_bar);
          const bar = Math.max(1, Math.floor(clamped / spb) + 1);
          const matchedSection = cur.sections.find((s) =>
            s.start_sec != null && s.end_sec != null
              ? clamped >= s.start_sec && clamped < s.end_sec
              : bar >= s.start_bar && bar < s.end_bar,
          );
          return {
            ...cur,
            current_position_sec: clamped,
            current_bar: bar,
            active_section_id: matchedSection?.id ?? cur.active_section_id,
          };
        }
        case 'ToggleLoop':
          return { ...cur, loop_enabled: !cur.loop_enabled };
        case 'SetLoop':
          return { ...cur, loop_enabled: command.payload };
        case 'ToggleContinueTrack':
          return { ...cur, continue_until_stopped: !cur.continue_until_stopped };
        case 'SetContinueTrack':
          return { ...cur, continue_until_stopped: command.payload };
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
        case 'SetBpm': {
          const bpm = Math.max(30, Math.min(260, command.payload));
          return {
            ...cur,
            current_bpm: bpm,
            percussion_audition_bpm: Math.max(40, Math.min(240, bpm)),
          };
        }
        case 'AdjustBpm': {
          const bpm = Math.max(30, Math.min(260, cur.current_bpm + command.payload));
          return {
            ...cur,
            current_bpm: bpm,
            percussion_audition_bpm: Math.max(40, Math.min(240, bpm)),
          };
        }
        case 'SetSongTiming': {
          const nextMode = command.payload.click_mode ?? cur.click_mode;
          const activeOverride =
            cur.active_setlist_slot !== null
              ? cur.setlist_items[cur.active_setlist_slot]?.click_mode ?? null
              : null;
          const effectiveMode = activeOverride ?? nextMode;
          const nextSections =
            command.payload.sections && command.payload.sections.length > 0
              ? command.payload.sections
                  .filter((s) => {
                    const hasTimes = s.start_sec != null && s.end_sec != null;
                    const validBounds = hasTimes
                      ? Number.isFinite(s.start_sec) &&
                        Number.isFinite(s.end_sec) &&
                        (s.start_sec ?? 0) >= 0 &&
                        (s.end_sec ?? 0) > (s.start_sec ?? 0) + 0.01
                      : s.start_bar >= 1 && s.end_bar > s.start_bar;
                    return validBounds && s.name.trim().length > 0;
                  })
                  .map((s) => ({
                    id: s.id,
                    name: s.name,
                    start_bar: s.start_bar,
                    end_bar: s.end_bar,
                    start_sec: s.start_sec ?? null,
                    end_sec: s.end_sec ?? null,
                    color: s.color ?? null,
                  }))
              : cur.sections;
          const introSec = nextSections.find((s) => s.name.toLowerCase().includes('intro'));
          const introEndBar = introSec
            ? introSec.end_bar
            : nextSections.length > 1 && nextSections[0].start_bar === 1
              ? nextSections[0].end_bar
              : 5;
          const activeSec =
            nextSections.find((s) =>
              s.start_sec != null && s.end_sec != null
                ? cur.current_position_sec >= s.start_sec && cur.current_position_sec < s.end_sec
                : cur.current_bar >= s.start_bar && cur.current_bar < s.end_bar,
            )?.id ??
            nextSections[0]?.id ??
            cur.active_section_id;
          const armedStillValid = cur.armed_section_id
            ? nextSections.some((s) => s.id === cur.armed_section_id)
            : false;
          return {
            ...cur,
            base_bpm: command.payload.bpm,
            current_bpm: command.payload.bpm,
            percussion_audition_bpm: Math.max(40, Math.min(240, command.payload.bpm)),
            beats_per_bar: Math.max(1, Math.min(12, Math.round(command.payload.beats_per_bar))),
            sections: nextSections,
            active_section_id: activeSec,
            armed_section_id: armedStillValid ? cur.armed_section_id : null,
            click_intro_end_bar: introEndBar,
            click_mode: effectiveMode,
            click_muted:
              effectiveMode === 'Off'
                ? true
                : effectiveMode === 'AlwaysOn'
                  ? false
                  : cur.current_bar >= introEndBar,
          };
        }
        case 'SetRoutingMode':
          return { ...cur, routing_mode: command.payload };
        case 'SelectOutputDevice': {
          const matched = this.audioDevices().find((d) => d.name === command.payload);
          if (this.newlyDetectedOutput()?.name === command.payload) {
            this.newlyDetectedOutput.set(null);
          }
          return {
            ...cur,
            device_name: command.payload,
            device_channels: matched?.max_channels ?? cur.device_channels,
            device_sample_rate: matched?.default_sample_rate ?? cur.device_sample_rate,
          };
        }
        case 'SetMasterGain':
          return { ...cur, master_gain: command.payload };
        case 'SetBackingGain':
          return { ...cur, backing_gain: command.payload };
        case 'SetClickGain':
          return { ...cur, click_gain: command.payload };
        case 'SetClickMuted':
          return { ...cur, click_muted: command.payload };
        case 'ToggleClickMuted':
          return { ...cur, click_muted: !cur.click_muted };
        case 'SetClickMode': {
          const mode = command.payload;
          const clickMuted =
            mode === 'Off'
              ? true
              : mode === 'AlwaysOn'
                ? false
                : cur.current_bar >= cur.click_intro_end_bar;
          return {
            ...cur,
            click_mode: mode,
            click_muted: clickMuted,
            setlist_items: cur.setlist_items.map((item, idx) =>
              idx === cur.active_setlist_slot
                ? { ...item, click_mode: mode, effective_click_mode: mode }
                : item,
            ),
          };
        }
        case 'SetStemGain':
        case 'SetStemMuted': {
          const stems = cur.stems.map((s) => {
            if (s.name !== command.payload.name) return s;
            return command.type === 'SetStemGain'
              ? { ...s, gain: command.payload.gain }
              : { ...s, muted: command.payload.muted };
          });
          const stemMix = stems.map((s) => ({ name: s.name, gain: s.gain, muted: s.muted }));
          return {
            ...cur,
            stems,
            setlist_items:
              cur.active_setlist_slot !== null
                ? cur.setlist_items.map((item, idx) =>
                    idx === cur.active_setlist_slot ? { ...item, stem_mix: stemMix } : item,
                  )
                : cur.setlist_items,
          };
        }
        case 'SetPadEnabled':
          return { ...cur, pad_enabled: command.payload };
        case 'SetPadMuted':
          return { ...cur, pad_muted: command.payload };
        case 'TogglePad':
          return {
            ...cur,
            pad_enabled: !cur.pad_enabled,
            pad_muted: !cur.pad_enabled ? false : cur.pad_muted,
          };
        case 'SetPadGain':
          return { ...cur, pad_gain: Math.max(0, Math.min(1.5, command.payload)) };
        case 'SetPadKey': {
          const k = command.payload;
          return {
            ...cur,
            pad_key: k,
            pad_is_minor: k.toLowerCase().endsWith('m') && !k.toLowerCase().endsWith('maj'),
            pad_follow_song_key: false,
          };
        }
        case 'AdjustPadKey': {
          const next = transposeKeyName(cur.pad_key, command.payload);
          return {
            ...cur,
            pad_key: next,
            pad_is_minor: next.toLowerCase().endsWith('m') && !next.toLowerCase().endsWith('maj'),
            pad_follow_song_key: false,
          };
        }
        case 'SetPadFollowSongKey': {
          const nextKey = command.payload ? cur.current_key : cur.pad_key;
          return {
            ...cur,
            pad_follow_song_key: command.payload,
            pad_key: nextKey,
            pad_is_minor:
              nextKey.toLowerCase().endsWith('m') && !nextKey.toLowerCase().endsWith('maj'),
          };
        }
        case 'SetPadCrossfadeSec':
          return { ...cur, pad_crossfade_sec: Math.max(0.5, Math.min(12, command.payload)) };
        case 'SetPadPatch': {
          const layers = [...DEFAULT_PAD_LAYERS.map((l) => ({ ...l, enabled: false }))];
          command.payload.layers.slice(0, 4).forEach((l, i) => {
            layers[i] = { ...l };
          });
          return {
            ...cur,
            pad_preset_id: command.payload.preset_id ?? null,
            pad_preset_name: command.payload.preset_name || 'Custom Pad',
            pad_layers: layers,
            pad_brightness: Math.max(0, Math.min(1, command.payload.brightness)),
            pad_shimmer: Math.max(0, Math.min(1, command.payload.shimmer)),
            pad_pan: Math.max(-1, Math.min(1, command.payload.pan)),
            pad_crossfade_sec:
              command.payload.crossfade_sec !== undefined && command.payload.crossfade_sec !== null
                ? Math.max(0.5, Math.min(12, command.payload.crossfade_sec))
                : cur.pad_crossfade_sec,
          };
        }
        case 'SetPadLayer': {
          const nextLayers = cur.pad_layers.map((l, i) =>
            i === command.payload.index
              ? (command.payload.layer ?? {
                  enabled: false,
                  archetype: 'WarmAnalog' as const,
                  register: 'Mid' as const,
                  tonality: 'Auto' as const,
                  gain: 0.75,
                })
              : l,
          );
          return {
            ...cur,
            pad_preset_id: null,
            pad_preset_name: 'Custom Mix',
            pad_layers: nextLayers,
          };
        }
        case 'SetPadLayerGain':
          return {
            ...cur,
            pad_layers: cur.pad_layers.map((l, i) =>
              i === command.payload.index
                ? { ...l, gain: Math.max(0, Math.min(1, command.payload.gain)) }
                : l,
            ),
          };
        case 'SetPadBrightness':
          return { ...cur, pad_brightness: Math.max(0, Math.min(1, command.payload)) };
        case 'SetPadShimmer':
          return { ...cur, pad_shimmer: Math.max(0, Math.min(1, command.payload)) };
        case 'SetPadPan':
          return { ...cur, pad_pan: Math.max(-1, Math.min(1, command.payload)) };
        case 'LockCurrentPadToSlot': {
          const lock: PadLockConfig = {
            enabled: cur.pad_enabled && !cur.pad_muted,
            gain: cur.pad_gain,
            preset_id: cur.pad_preset_id,
            preset_name: cur.pad_preset_name,
            layers: cur.pad_layers.map((l) => ({ ...l })),
            brightness: cur.pad_brightness,
            shimmer: cur.pad_shimmer,
            pan: cur.pad_pan,
          };
          return {
            ...cur,
            setlist_items: cur.setlist_items.map((it, idx) =>
              idx === command.payload ? { ...it, pad_lock: lock } : it,
            ),
          };
        }
        case 'ClearSlotPadLock':
          return {
            ...cur,
            setlist_items: cur.setlist_items.map((it, idx) =>
              idx === command.payload ? { ...it, pad_lock: null } : it,
            ),
          };
        case 'SetSetlistName':
          return { ...cur, setlist_name: command.payload };
        case 'AddSetlistSong': {
          const libMatch = this.libraryEntries().find((e) => e.dir === command.payload.dir);
          const title =
            libMatch?.title ?? (command.payload.dir.split(/[/\\]/).filter(Boolean).pop() ?? 'Song');
          const origKey = libMatch?.original_key ?? 'G';
          const baseBpm = libMatch?.bpm ?? 72;
          const newItem = {
            slot: cur.setlist_items.length,
            song_id: libMatch?.song_id ?? title.toLowerCase().replace(/[^a-z0-9]+/g, '-'),
            song_dir: command.payload.dir,
            title,
            original_key: origKey,
            effective_key: origKey,
            base_bpm: baseBpm,
            effective_bpm: baseBpm,
            key_shift_semitones: 0,
            bpm_override: null,
            transition_mode: 'AutoPause' as const,
            crossfade_sec: 2.0,
            ambient_pad_enabled: true,
            pad_key_override: null,
            click_mode: null,
            effective_click_mode: 'AlwaysOn' as const,
            preload_state: { status: 'Ready' as const },
          };
          const nextItems = [...cur.setlist_items];
          const insertAt = command.payload.insert_at;
          if (insertAt !== null && insertAt !== undefined && insertAt >= 0 && insertAt <= nextItems.length) {
            nextItems.splice(insertAt, 0, newItem);
          } else {
            nextItems.push(newItem);
          }
          return {
            ...cur,
            setlist_items: nextItems.map((it, idx) => ({ ...it, slot: idx })),
            active_setlist_slot: cur.active_setlist_slot ?? 0,
          };
        }
        case 'SelectSetlistSlot': {
          const nextSlot =
            command.payload < cur.setlist_items.length ? command.payload : cur.active_setlist_slot;
          const slotItem = nextSlot !== null ? cur.setlist_items[nextSlot] : undefined;
          return this.applyPreviewSlotTransition(cur, nextSlot, slotItem);
        }
        case 'NextSong': {
          if (cur.setlist_items.length === 0) return cur;
          const next =
            cur.active_setlist_slot === null
              ? 0
              : Math.min(cur.active_setlist_slot + 1, cur.setlist_items.length - 1);
          return this.applyPreviewSlotTransition(cur, next, cur.setlist_items[next]);
        }
        case 'PrevSong': {
          if (cur.setlist_items.length === 0) return cur;
          const prev = Math.max((cur.active_setlist_slot ?? 0) - 1, 0);
          return this.applyPreviewSlotTransition(cur, prev, cur.setlist_items[prev]);
        }
        case 'RemoveSetlistSlot': {
          const filtered = cur.setlist_items
            .filter((_, i) => i !== command.payload)
            .map((item, i) => ({ ...item, slot: i }));
          return {
            ...cur,
            setlist_items: filtered,
            active_setlist_slot:
              filtered.length === 0
                ? null
                : Math.min(cur.active_setlist_slot ?? 0, filtered.length - 1),
          };
        }
        case 'MoveSetlistSlot': {
          const { from, to } = command.payload;
          if (
            from < 0 ||
            to < 0 ||
            from >= cur.setlist_items.length ||
            to >= cur.setlist_items.length ||
            from === to
          ) {
            return cur;
          }
          const copy = [...cur.setlist_items];
          const [moved] = copy.splice(from, 1);
          copy.splice(to, 0, moved);
          let nextActive = cur.active_setlist_slot;
          if (nextActive === from) {
            nextActive = to;
          } else if (nextActive !== null) {
            if (from < nextActive && to >= nextActive) nextActive -= 1;
            else if (from > nextActive && to <= nextActive) nextActive += 1;
          }
          return {
            ...cur,
            setlist_items: copy.map((item, i) => ({ ...item, slot: i })),
            active_setlist_slot: nextActive,
          };
        }
        case 'UpdateSetlistSlotConfig': {
          const nextItems = cur.setlist_items.map((item, i) => {
            if (i !== command.payload.slot) return item;
            const nextShift =
              command.payload.key_shift_semitones !== undefined &&
              command.payload.key_shift_semitones !== null
                ? Math.max(-12, Math.min(12, command.payload.key_shift_semitones))
                : item.key_shift_semitones;
            const nextBpmOverride =
              command.payload.bpm_override !== undefined
                ? command.payload.bpm_override
                : item.bpm_override;
            const nextClickMode =
              command.payload.click_mode !== undefined
                ? command.payload.click_mode
                : item.click_mode;
            const nextSectionId =
              command.payload.section_id !== undefined
                ? command.payload.section_id
                : (item.section_id ?? null);
            const nextPercLock =
              command.payload.percussion_lock !== undefined
                ? command.payload.percussion_lock
                : (item.percussion_lock ?? null);
            return {
              ...item,
              key_shift_semitones: nextShift,
              effective_key: transposeKeyName(item.original_key, nextShift),
              bpm_override: nextBpmOverride,
              effective_bpm: nextBpmOverride ?? item.base_bpm,
              transition_mode: command.payload.transition_mode ?? item.transition_mode,
              crossfade_sec: command.payload.crossfade_sec ?? item.crossfade_sec,
              ambient_pad_enabled:
                command.payload.ambient_pad_enabled ?? item.ambient_pad_enabled,
              pad_key_override:
                command.payload.pad_key_override !== undefined
                  ? command.payload.pad_key_override
                  : item.pad_key_override,
              pad_lock:
                command.payload.pad_lock !== undefined ? command.payload.pad_lock : item.pad_lock,
              click_mode: nextClickMode,
              effective_click_mode: nextClickMode ?? 'AlwaysOn',
              section_id: nextSectionId,
              percussion_lock: nextPercLock,
            };
          });
          const activeUpdated =
            cur.active_setlist_slot === command.payload.slot
              ? nextItems[command.payload.slot]
              : undefined;
          const nextClickMode = activeUpdated?.effective_click_mode ?? cur.click_mode;
          const nextBpm = activeUpdated?.effective_bpm ?? cur.current_bpm;
          const activePerc = activeUpdated?.percussion_lock;
          return {
            ...cur,
            setlist_items: nextItems,
            current_bpm: nextBpm,
            percussion_audition_bpm: Math.max(40, Math.min(240, nextBpm)),
            click_mode: nextClickMode,
            click_muted:
              activeUpdated !== undefined ? nextClickMode === 'Off' : cur.click_muted,
            ...(activeUpdated !== undefined && activePerc
              ? {
                  percussion_mode: activePerc.mode,
                  percussion_style: activePerc.style,
                  percussion_intensity: activePerc.intensity,
                  percussion_custom_pattern: activePerc.custom_pattern ?? null,
                }
              : {}),
          };
        }
        case 'BatchUpdateSetlistSlots': {
          const p = command.payload;
          const slotSet = new Set(p.slots);
          const nextItems = cur.setlist_items.map((item, i) => {
            if (!slotSet.has(i)) return item;
            let nextShift = item.key_shift_semitones;
            if (p.target_key) {
              const computed = computeKeySemitoneShift(item.original_key, p.target_key);
              if (computed !== null) nextShift = computed;
            } else if (p.key_shift_semitones !== undefined) {
              nextShift = Math.max(-12, Math.min(12, p.key_shift_semitones));
            }
            const nextBpmOverride = p.bpm_override !== undefined ? p.bpm_override : item.bpm_override;
            const nextClickMode = p.click_mode !== undefined ? p.click_mode : item.click_mode;
            let nextPercLock: PercussionLockConfig | null | undefined = item.percussion_lock ?? null;
            if (
              p.percussion_mode !== undefined ||
              p.percussion_style !== undefined ||
              p.percussion_intensity !== undefined ||
              p.percussion_custom_pattern !== undefined
            ) {
              const base: PercussionLockConfig = nextPercLock ?? {
                mode: cur.percussion_mode,
                style: cur.percussion_style,
                intensity: cur.percussion_intensity,
                gain: cur.percussion_gain,
                iem_send: cur.percussion_iem_send,
                follow_sections: cur.percussion_follow_sections,
                channels: cur.percussion_channels.map((c) => ({ ...c })),
              };
              const nextCustom =
                p.percussion_custom_pattern !== undefined
                  ? p.percussion_custom_pattern
                    ? {
                        ...p.percussion_custom_pattern,
                        hits: p.percussion_custom_pattern.hits.map((h) => ({ ...h })),
                      }
                    : null
                  : base.custom_pattern ?? null;
              let nextMode = p.percussion_mode ?? base.mode;
              if (
                p.percussion_custom_pattern !== undefined &&
                p.percussion_custom_pattern !== null &&
                p.percussion_mode === undefined &&
                nextMode === 'Off'
              ) {
                nextMode = 'PercussionOnly';
              }
              nextPercLock = {
                ...base,
                mode: nextMode,
                style: p.percussion_style ?? base.style,
                intensity: p.percussion_intensity ?? base.intensity,
                custom_pattern: nextCustom,
              };
            }
            return {
              ...item,
              key_shift_semitones: nextShift,
              effective_key: transposeKeyName(item.original_key, nextShift),
              bpm_override: nextBpmOverride,
              effective_bpm: nextBpmOverride ?? item.base_bpm,
              click_mode: nextClickMode,
              effective_click_mode: nextClickMode ?? 'AlwaysOn',
              transition_mode: p.transition_mode ?? item.transition_mode,
              crossfade_sec: p.crossfade_sec ?? item.crossfade_sec,
              ambient_pad_enabled: p.ambient_pad_enabled ?? item.ambient_pad_enabled,
              pad_lock: p.pad_lock !== undefined ? p.pad_lock : item.pad_lock,
              section_id: p.section_id !== undefined ? p.section_id : (item.section_id ?? null),
              percussion_lock: nextPercLock,
            };
          });
          const activeItem =
            cur.active_setlist_slot !== null ? nextItems[cur.active_setlist_slot] : undefined;
          return {
            ...cur,
            setlist_items: nextItems,
            ...(activeItem && cur.active_setlist_slot !== null && slotSet.has(cur.active_setlist_slot)
              ? {
                  current_key: activeItem.effective_key,
                  current_bpm: activeItem.effective_bpm,
                  percussion_audition_bpm: Math.max(40, Math.min(240, activeItem.effective_bpm)),
                  click_mode: activeItem.effective_click_mode,
                  click_muted: activeItem.effective_click_mode === 'Off',
                  pad_enabled: activeItem.ambient_pad_enabled,
                  ...(activeItem.percussion_lock
                    ? {
                        percussion_mode: activeItem.percussion_lock.mode,
                        percussion_style: activeItem.percussion_lock.style,
                        percussion_intensity: activeItem.percussion_lock.intensity,
                        percussion_custom_pattern:
                          activeItem.percussion_lock.custom_pattern ?? null,
                      }
                    : {}),
                }
              : {}),
          };
        }
        case 'UpsertSetlistSection': {
          const sec: SetlistSection = { ...command.payload };
          const idx = cur.setlist_sections.findIndex((s) => s.id === sec.id);
          const nextSections =
            idx >= 0
              ? cur.setlist_sections.map((s, i) => (i === idx ? sec : s))
              : [...cur.setlist_sections, sec];
          return { ...cur, setlist_sections: nextSections };
        }
        case 'RemoveSetlistSection': {
          const secId = command.payload;
          return {
            ...cur,
            setlist_sections: cur.setlist_sections.filter((s) => s.id !== secId),
            setlist_items: cur.setlist_items.map((it) =>
              it.section_id === secId ? { ...it, section_id: null } : it,
            ),
          };
        }
        case 'MoveSetlistSection': {
          const from = cur.setlist_sections.findIndex((s) => s.id === command.payload.id);
          if (from < 0) return cur;
          const to = Math.max(0, Math.min(cur.setlist_sections.length - 1, command.payload.to_index));
          if (from === to) return cur;
          const copy = [...cur.setlist_sections];
          const [moved] = copy.splice(from, 1);
          copy.splice(to, 0, moved);
          return { ...cur, setlist_sections: copy };
        }
        case 'ToggleSetlistSectionCollapsed':
          return {
            ...cur,
            setlist_sections: cur.setlist_sections.map((s) =>
              s.id === command.payload.id ? { ...s, collapsed: command.payload.collapsed } : s,
            ),
          };
        case 'AddStandaloneSetlistSlot': {
          const p = command.payload;
          const cleanTitle =
            p.title.trim() || (p.slot_kind === 'PadOnly' ? 'Pad Underscore' : 'Percussion Groove');
          const percLock: PercussionLockConfig | null =
            p.slot_kind === 'PercussionGroove'
              ? {
                  mode: 'PercussionOnly',
                  style:
                    p.time_signature[0] === 6 || p.time_signature[0] === 12
                      ? 'Worship68'
                      : 'Worship44',
                  intensity: 'Medium',
                  gain: cur.percussion_gain,
                  iem_send: cur.percussion_iem_send,
                  follow_sections: true,
                  channels: defaultPercussionChannels(),
                }
              : null;
          const newItem = {
            slot: cur.setlist_items.length,
            song_id: `slot-${Date.now()}`,
            song_dir: null,
            title: cleanTitle,
            original_key: p.key || 'C',
            effective_key: p.key || 'C',
            base_bpm: p.bpm || 80,
            effective_bpm: p.bpm || 80,
            key_shift_semitones: 0,
            bpm_override: null,
            transition_mode: 'Manual' as const,
            crossfade_sec: 2.0,
            ambient_pad_enabled: p.slot_kind === 'PadOnly',
            pad_key_override: null,
            pad_lock: null,
            click_mode: p.slot_kind === 'PadOnly' ? ('Off' as const) : null,
            effective_click_mode:
              p.slot_kind === 'PadOnly' ? ('Off' as const) : ('AlwaysOn' as const),
            section_id: p.section_id ?? null,
            slot_kind: p.slot_kind,
            time_signature: p.time_signature,
            percussion_lock: percLock,
            preload_state: { status: 'Ready' as const },
          };
          const nextItems = [...cur.setlist_items];
          const insertAt = p.insert_at;
          if (insertAt !== null && insertAt !== undefined && insertAt >= 0 && insertAt <= nextItems.length) {
            nextItems.splice(insertAt, 0, newItem);
          } else {
            nextItems.push(newItem);
          }
          return {
            ...cur,
            setlist_items: nextItems.map((it, idx) => ({ ...it, slot: idx })),
            active_setlist_slot: cur.active_setlist_slot ?? 0,
          };
        }
        case 'SetPercussionMode':
          return { ...cur, percussion_mode: command.payload };
        case 'SetPercussionStyle':
          return { ...cur, percussion_style: command.payload, percussion_custom_pattern: null };
        case 'SetPercussionIntensity':
          return { ...cur, percussion_intensity: command.payload };
        case 'SetPercussionFollowSections':
          return { ...cur, percussion_follow_sections: command.payload };
        case 'SetPercussionGain':
          return { ...cur, percussion_gain: Math.max(0, Math.min(1.5, command.payload)) };
        case 'SetPercussionIemSend':
          return { ...cur, percussion_iem_send: Math.max(0, Math.min(1, command.payload)) };
        case 'SetPercussionRoom':
          return { ...cur, percussion_room_mix: Math.max(0, Math.min(1, command.payload)) };
        case 'SetCustomPercussionPattern':
          return { ...cur, percussion_custom_pattern: command.payload };
        case 'LoadPercussionKitDir': {
          const kitDir = command.payload;
          const kit = kitDir ? this.percussionKits().find((k) => k.dir === kitDir) : null;
          return {
            ...cur,
            percussion_kit_dir: kitDir,
            percussion_kit_name: kit?.name ?? (kitDir ? 'Custom Kit' : null),
          };
        }
        case 'TriggerPercussionPreview':
          return cur;
        case 'SetPercussionChannel':
          return {
            ...cur,
            percussion_channels: cur.percussion_channels.map((ch) =>
              ch.instrument === command.payload.instrument
                ? {
                    ...ch,
                    enabled:
                      command.payload.enabled !== undefined && command.payload.enabled !== null
                        ? command.payload.enabled
                        : ch.enabled,
                    gain:
                      command.payload.gain !== undefined && command.payload.gain !== null
                        ? Math.max(0, Math.min(1.5, command.payload.gain))
                        : ch.gain,
                    pan:
                      command.payload.pan !== undefined && command.payload.pan !== null
                        ? Math.max(-1, Math.min(1, command.payload.pan))
                        : ch.pan,
                  }
                : ch,
            ),
          };
        case 'ResetPercussionMix':
          return { ...cur, percussion_channels: defaultPercussionChannels() };
        case 'LockCurrentPercussionToSlot': {
          const lock: PercussionLockConfig = {
            mode: cur.percussion_mode,
            style: cur.percussion_style,
            intensity: cur.percussion_intensity,
            gain: cur.percussion_gain,
            iem_send: cur.percussion_iem_send,
            follow_sections: cur.percussion_follow_sections,
            room_mix: cur.percussion_room_mix,
            kit_name: cur.percussion_kit_name,
            kit_dir: cur.percussion_kit_dir,
            custom_pattern: cur.percussion_custom_pattern,
            channels: cur.percussion_channels.map((c) => ({ ...c })),
          };
          return {
            ...cur,
            setlist_items: cur.setlist_items.map((it, idx) =>
              idx === command.payload ? { ...it, percussion_lock: lock } : it,
            ),
          };
        }
        case 'ClearSlotPercussionLock':
          return {
            ...cur,
            setlist_items: cur.setlist_items.map((it, idx) =>
              idx === command.payload ? { ...it, percussion_lock: null } : it,
            ),
          };
        case 'StartPercussionAudition':
          return {
            ...cur,
            is_playing: false,
            percussion_audition_playing: true,
            percussion_audition_bpm: Math.max(40, Math.min(240, command.payload.bpm)),
            percussion_current_step: 0,
          };
        case 'StopPercussionAudition':
          return {
            ...cur,
            percussion_audition_playing: false,
            percussion_current_step: null,
          };
        case 'SetPercussionAuditionBpm':
          return {
            ...cur,
            percussion_audition_bpm: Math.max(40, Math.min(240, command.payload)),
          };
        case 'LoadSetlist': {
          const items = command.payload.items.map((it, idx) => {
            const origKey = it.original_key ?? 'G';
            const baseBpm = it.base_bpm ?? 72;
            const baseClickMode = it.base_click_mode ?? 'AlwaysOn';
            return {
              slot: idx,
              song_id: it.song_id,
              song_dir: it.song_dir ?? null,
              title: it.title ?? it.song_id,
              original_key: origKey,
              effective_key: transposeKeyName(origKey, it.key_shift_semitones),
              base_bpm: baseBpm,
              effective_bpm: it.bpm_override ?? baseBpm,
              key_shift_semitones: it.key_shift_semitones,
              bpm_override: it.bpm_override ?? null,
              transition_mode: it.transition_mode,
              crossfade_sec: it.crossfade_sec,
              ambient_pad_enabled: it.ambient_pad_enabled,
              pad_key_override: it.pad_key_override ?? null,
              pad_lock: it.pad_lock ?? null,
              click_mode: it.click_mode ?? null,
              effective_click_mode: it.click_mode ?? baseClickMode,
              section_id: it.section_id ?? null,
              slot_kind: it.slot_kind ?? ('SongBundle' as const),
              time_signature: it.time_signature ?? null,
              percussion_lock: it.percussion_lock ?? null,
              stem_mix: (it.stem_mix ?? []).map((s) => ({ ...s })),
              preload_state: { status: 'Ready' as const },
            };
          });
          const firstClickMode = items[0]?.effective_click_mode ?? cur.click_mode;
          const firstBpm = items[0]?.effective_bpm ?? cur.current_bpm;
          const mixer = command.payload.mixer;
          const firstStems = this.applyPreviewStemMix(cur.stems, items[0]?.stem_mix);
          return {
            ...cur,
            setlist_name: command.payload.name,
            setlist_sections: (command.payload.sections ?? []).map((s) => ({ ...s })),
            setlist_items: items,
            active_setlist_slot: items.length > 0 ? 0 : null,
            current_bpm: firstBpm,
            percussion_audition_bpm: Math.max(40, Math.min(240, firstBpm)),
            click_mode: firstClickMode,
            master_gain: mixer ? Math.max(0, Math.min(1.5, mixer.master_gain)) : cur.master_gain,
            backing_gain:
              mixer?.backing_gain != null
                ? Math.max(0, Math.min(1.5, mixer.backing_gain))
                : cur.backing_gain,
            click_gain: mixer ? Math.max(0, Math.min(1.5, mixer.click_gain)) : cur.click_gain,
            click_muted:
              items.length > 0
                ? firstClickMode === 'Off' || Boolean(mixer?.click_muted)
                : (mixer?.click_muted ?? cur.click_muted),
            stems: firstStems,
          };
        }
        case 'ClearSetlist':
          return {
            ...cur,
            setlist_sections: [],
            setlist_items: [],
            active_setlist_slot: null,
          };
        default:
          return cur;
      }
    });
    persistPreviewSnapshot(this.snapshot());
  }

  private applyPreviewStemMix(
    stems: EngineSnapshot['stems'],
    stemMix: EngineSnapshot['setlist_items'][number]['stem_mix'],
  ): EngineSnapshot['stems'] {
    if (!stemMix || stemMix.length === 0) return stems;
    return stems.map((s) => {
      const ov = stemMix.find((m) => m.name === s.name);
      if (!ov) return s;
      return {
        ...s,
        gain: Math.max(0, Math.min(1.5, ov.gain)),
        muted: ov.muted,
      };
    });
  }

  private applyPreviewSlotTransition(
    cur: EngineSnapshot,
    nextSlot: number | null,
    slotItem: EngineSnapshot['setlist_items'][number] | undefined,
  ): EngineSnapshot {
    const nextClickMode = slotItem?.effective_click_mode ?? cur.click_mode;
    const nextKey = slotItem?.effective_key ?? cur.current_key;
    const padKey = slotItem?.pad_key_override ?? (cur.pad_follow_song_key ? nextKey : cur.pad_key);
    const padIsMinor = padKey.toLowerCase().endsWith('m') && !padKey.toLowerCase().endsWith('maj');
    const lock = slotItem?.pad_lock;

    let padLayers = cur.pad_layers;
    if (lock) {
      const padded = DEFAULT_PAD_LAYERS.map((l) => ({ ...l, enabled: false }));
      lock.layers.slice(0, 4).forEach((l, idx) => {
        padded[idx] = { ...l };
      });
      padLayers = padded;
    }

    const percLock = slotItem?.percussion_lock;
    const prevSlotItem =
      cur.active_setlist_slot !== null ? cur.setlist_items[cur.active_setlist_slot] : undefined;
    const prevSectionId = prevSlotItem?.section_id ?? null;
    const sameSection = Boolean(prevSectionId && slotItem?.section_id === prevSectionId);
    const shouldStopStyle =
      !percLock &&
      slotItem?.slot_kind !== 'PercussionGroove' &&
      !sameSection &&
      (prevSectionId !== null || cur.percussion_mode === 'PercussionOnly');

    const nextBpm = slotItem?.effective_bpm ?? cur.current_bpm;
    return {
      ...cur,
      active_setlist_slot: nextSlot,
      song_title: slotItem?.title ?? cur.song_title,
      current_key: nextKey,
      current_bpm: nextBpm,
      percussion_audition_bpm: Math.max(40, Math.min(240, nextBpm)),
      click_mode: nextClickMode,
      click_muted: nextClickMode === 'Off',
      stems: this.applyPreviewStemMix(cur.stems, slotItem?.stem_mix),
      pad_key: padKey,
      pad_is_minor: padIsMinor,
      pad_follow_song_key: slotItem?.pad_key_override ? false : cur.pad_follow_song_key,
      ...(lock
        ? {
            pad_enabled: lock.enabled,
            pad_muted: lock.enabled ? false : cur.pad_muted,
            pad_gain: lock.gain,
            pad_preset_id: lock.preset_id ?? null,
            pad_preset_name: lock.preset_name ?? 'Locked Pad',
            pad_layers: padLayers,
            pad_brightness: lock.brightness,
            pad_shimmer: lock.shimmer,
            pad_pan: lock.pan,
          }
        : {}),
      ...(percLock
        ? {
            percussion_mode: percLock.mode,
            percussion_style: percLock.style,
            percussion_intensity: percLock.intensity,
            percussion_gain: percLock.gain,
            percussion_iem_send: percLock.iem_send,
            percussion_follow_sections: percLock.follow_sections,
            percussion_room_mix: percLock.room_mix ?? cur.percussion_room_mix,
            percussion_kit_name: percLock.kit_name ?? null,
            percussion_kit_dir: percLock.kit_dir ?? null,
            percussion_custom_pattern: percLock.custom_pattern ?? null,
            percussion_channels: percLock.channels.map((c) => ({ ...c })),
          }
        : shouldStopStyle
          ? { percussion_mode: 'Off' as const }
          : {}),
    };
  }

  ngOnDestroy(): void {
    void this.setLiveKeepAwake(false);
    for (const unlisten of this.unlistenFns) {
      unlisten();
    }
  }
}
