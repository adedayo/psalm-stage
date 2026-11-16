import { DecimalPipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  EventEmitter,
  HostListener,
  Input,
  Output,
  ViewChild,
  computed,
  effect,
  inject,
  signal,
} from '@angular/core';
import { FormsModule } from '@angular/forms';
import { HlmBadgeImports } from '@spartan-ng/helm/badge';
import { HlmButtonImports } from '@spartan-ng/helm/button';
import { HlmCardImports } from '@spartan-ng/helm/card';
import {
  type AudioAnalysisResult,
  type ClickMode,
  type LibrarySongEntry,
  type SectionCuePlacement,
  type SectionDef,
  type SeparationEngine,
  type SongManifest,
  type StemDefDetailed,
  type StemPreset,
  type WorkbenchConfig,
  type YouTubeMetadata,
} from '../../models/engine.models';
import {
  CHROMATIC_KEYS,
  SONG_KEY_OPTIONS,
  PsalmStageService,
  canonicalizeSourceRef,
} from '../../services/psalm-stage.service';

export interface WorkbenchStemRow {
  key: string;
  file: string;
  pitched: boolean;
  is_cues: boolean;
  default_gain: number;
  muted: boolean;
}

export interface RenderedBarTick {
  bar: number;
  xPct: number;
  major: boolean;
  showLabel: boolean;
}

export interface RenderedSectionBand {
  index: number;
  id: string;
  name: string;
  startBar: number;
  endBar: number;
  startSec: number;
  endSec: number;
  leftPct: number;
  widthPct: number;
  colorClass: string;
  selected: boolean;
  boundaryLeftPct: number | null;
  endHandleLeftPct: number | null;
}

export type PlatformTab = 'macos' | 'windows' | 'linux';

export interface PlatformGuideRecipe {
  id: PlatformTab;
  label: string;
  youtubeCommand: string;
  stemCommand: string;
  locateCommand: string;
  notes: string[];
}

export const PLATFORM_INSTALL_GUIDES: Record<PlatformTab, PlatformGuideRecipe> = {
  macos: {
    id: 'macos',
    label: 'macOS',
    youtubeCommand: 'brew install ffmpeg yt-dlp',
    stemCommand: 'brew install pipx && pipx ensurepath && pipx install "audio-separator[cpu]"',
    locateCommand: 'which yt-dlp ffmpeg audio-separator',
    notes: [
      'Requires Homebrew (brew.sh). On Apple Silicon, Homebrew installs binaries to /opt/homebrew/bin; Intel Macs use /usr/local/bin.',
      'For Demucs instead of UVR CLI, run: pipx install demucs',
      'After installing, click Re-Scan below (no app restart needed).',
    ],
  },
  windows: {
    id: 'windows',
    label: 'Windows',
    youtubeCommand: 'winget install Gyan.FFmpeg yt-dlp.yt-dlp',
    stemCommand: 'winget install Python.Python.3.11 && pip install pipx && pipx ensurepath && pipx install "audio-separator[cpu]"',
    locateCommand: 'where.exe yt-dlp ffmpeg audio-separator',
    notes: [
      'Run commands in PowerShell or Windows Terminal. For NVIDIA GPUs, use "audio-separator[gpu]" instead of [cpu].',
      'WinGet links typically appear in %LOCALAPPDATA%\\Microsoft\\WinGet\\Links; pipx tools appear in %USERPROFILE%\\.local\\bin.',
      'After installing, click Re-Scan (or Browse… to select yt-dlp.exe manually).',
    ],
  },
  linux: {
    id: 'linux',
    label: 'Linux',
    youtubeCommand: 'sudo apt update && sudo apt install -y ffmpeg pipx && pipx ensurepath && pipx install yt-dlp',
    stemCommand: 'pipx install "audio-separator[cpu]"',
    locateCommand: 'which yt-dlp ffmpeg audio-separator',
    notes: [
      'On Fedora use dnf install ffmpeg pipx; on Arch use pacman -S ffmpeg python-pipx yt-dlp.',
      'pipx installs isolated CLIs into ~/.local/bin so system Python packages stay untouched.',
      'If you have an NVIDIA CUDA GPU, install "audio-separator[gpu]" for ~10x faster stem separation.',
    ],
  },
};

const SECTION_PRESETS: { name: string; bars: number; color: string }[] = [
  { name: 'Intro', bars: 4, color: 'slate' },
  { name: 'Verse', bars: 8, color: 'sky' },
  { name: 'Pre-Chorus', bars: 4, color: 'violet' },
  { name: 'Chorus', bars: 8, color: 'emerald' },
  { name: 'Bridge', bars: 8, color: 'amber' },
  { name: 'Instrumental', bars: 8, color: 'cyan' },
  { name: 'Tag', bars: 4, color: 'rose' },
  { name: 'Vamp', bars: 4, color: 'violet' },
  { name: 'Outro', bars: 4, color: 'slate' },
];

const WORKBENCH_ZOOM_STEPS = [1, 2, 4, 8, 16, 32] as const;

@Component({
  selector: 'app-import-workbench',
  standalone: true,
  imports: [DecimalPipe, FormsModule, HlmButtonImports, HlmBadgeImports, HlmCardImports],
  templateUrl: './import-workbench.component.html',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class ImportWorkbenchComponent {
  protected readonly stage = inject(PsalmStageService);
  protected readonly snap = this.stage.snapshot;
  protected readonly chromaticKeys = CHROMATIC_KEYS;
  protected readonly songKeyOptions = SONG_KEY_OPTIONS;
  protected readonly sectionPresets = SECTION_PRESETS;
  protected readonly zoomSteps = WORKBENCH_ZOOM_STEPS;

  @ViewChild('waveformScroll') private waveformScrollRef?: ElementRef<HTMLDivElement>;
  @ViewChild('rapidSectionNameInput') private rapidSectionNameInputRef?: ElementRef<HTMLInputElement>;

  @Input() set targetFolder(dir: string | null) {
    if (dir && dir !== this.loadedDir()) {
      void this.openFolderInWorkbench(dir);
    }
  }

  @Output() readonly savedToLibrary = new EventEmitter<string>();
  @Output() readonly goToStudio = new EventEmitter<void>();

  // Ingest Panel State
  readonly ingestTab = signal<'file' | 'youtube' | 'folder'>('file');
  readonly localFilePath = signal('');
  readonly youtubeUrl = signal('');
  readonly youtubeMeta = signal<YouTubeMetadata | null>(null);
  readonly youtubeProbing = signal(false);
  readonly songTitleInput = signal('');
  readonly separationEngine = signal<SeparationEngine>('None');
  readonly stemPreset = signal<StemPreset>('FourStems');
  readonly customUvrModel = signal('htdemucs_ft.yaml');
  readonly showToolchainDrawer = signal(false);
  readonly guidePlatform = signal<PlatformTab>('macos');
  readonly copiedHint = signal<string | null>(null);
  readonly platformGuides = PLATFORM_INSTALL_GUIDES;
  readonly platformTabs: PlatformTab[] = ['macos', 'windows', 'linux'];

  // Toolchain custom paths draft
  readonly pathFfmpeg = signal('');
  readonly pathYtDlp = signal('');
  readonly pathAudioSeparator = signal('');
  readonly pathDemucs = signal('');

  // Loaded Bundle & Waveform Workbench State
  readonly loadedDir = signal<string>('');
  readonly loadingBundle = signal(false);
  readonly manifest = signal<SongManifest | null>(null);
  readonly analysis = signal<AudioAnalysisResult | null>(null);
  readonly analyzingWaveform = signal(false);
  readonly waveformZoom = signal<number>(1);
  readonly followPlayhead = signal(true);
  readonly selectedSectionIdx = signal<number | null>(null);
  readonly editingBandIdx = signal<number | null>(null);
  readonly editingBandName = signal('');
  readonly rapidSectionName = signal('');
  readonly cueLeadInBeats = signal<number>(2);
  readonly cueIncludeChime = signal<boolean>(true);
  readonly autoRegenerateCuesOnSave = signal<boolean>(false);
  readonly generatingCues = signal<boolean>(false);
  readonly generatedCuePlacements = signal<SectionCuePlacement[]>([]);
  readonly workbenchError = signal<string | null>(null);
  readonly workbenchNotice = signal<string | null>(null);
  readonly soloedStems = signal<string[]>([]);
  readonly forceReextract = signal(false);
  private preSoloMuted: Record<string, boolean> | null = null;
  private lastHandledJobKey = '';
  private openSeq = 0;

  // Tap Tempo tracking
  private tapTimestamps: number[] = [];
  private tapAlignTimer: ReturnType<typeof setTimeout> | null = null;
  readonly tapCount = signal(0);
  readonly aligningDownbeat = signal(false);

  // Tap Beat 1 (while auditioning) tracking
  private beat1TapRecords: { pos: number; barIdx: number }[] = [];
  private lastBeat1TapAt = 0;
  readonly beat1TapCount = signal(0);

  // Waveform / Bar 1 / Section boundary drag state
  readonly draggingTarget = signal<'playhead' | 'bar1' | 'boundary' | null>(null);
  private dragCanvasEl: HTMLElement | null = null;
  private dragBoundarySectionIdx: number | null = null;
  private dragBoundaryEdge: 'start' | 'end' = 'start';
  private lastDragSeekAt = 0;
  private lastDragTimingSyncAt = 0;

  constructor() {
    effect(() => {
      const cfg = this.stage.workbenchConfig();
      this.pathFfmpeg.set(cfg.ffmpeg_path ?? '');
      this.pathYtDlp.set(cfg.yt_dlp_path ?? '');
      this.pathAudioSeparator.set(cfg.audio_separator_path ?? '');
      this.pathDemucs.set(cfg.demucs_path ?? '');
      this.customUvrModel.set(cfg.custom_uvr_model);
    });

    effect(() => {
      const p = this.stage.toolchainStatus()?.platform;
      if (p === 'windows' || p === 'linux' || p === 'macos') {
        this.guidePlatform.set(p);
      }
    });

    effect(() => {
      const job = this.stage.workbenchJob();
      if (job?.status === 'Completed' && job.output_dir) {
        const key = `${job.job_id}:${job.output_dir}:${job.message}`;
        if (key !== this.lastHandledJobKey) {
          this.lastHandledJobKey = key;
          this.forceReextract.set(false);
          void this.stage.refreshLibrary();
          void this.openFolderInWorkbench(job.output_dir);
        }
      }
    });

    effect(() => {
      const snap = this.snap();
      const zoom = this.waveformZoom();
      if (!snap.is_playing || !this.followPlayhead() || zoom <= 1 || this.draggingTarget() !== null) {
        return;
      }
      this.ensurePlayheadInView(snap.current_position_sec);
    });
  }

  readonly effectiveDurationSec = computed(() => {
    const a = this.analysis();
    if (a && a.duration_sec > 0) return a.duration_sec;
    const m = this.manifest();
    if (!m || m.sections.length === 0) return 180;
    const maxBar = Math.max(...m.sections.map((s) => s.end_bar));
    const spb = (60 / Math.max(30, m.bpm)) * Math.max(1, m.time_signature[0]);
    return Math.max(30, m.first_downbeat_sec + (maxBar - 1) * spb);
  });

  readonly waveformPathD = computed(() => {
    const a = this.analysis();
    if (!a || a.peaks.length === 0) return '';
    const n = a.peaks.length;
    const midY = 50;
    const parts: string[] = [];
    for (let i = 0; i < n; i++) {
      const x = ((i / Math.max(1, n - 1)) * 1000).toFixed(2);
      const [lo, hi] = a.peaks[i];
      const yTop = (midY - Math.max(0.02, Math.abs(hi)) * 44).toFixed(2);
      const yBot = (midY + Math.max(0.02, Math.abs(lo)) * 44).toFixed(2);
      parts.push(`M${x} ${yTop} L${x} ${yBot}`);
    }
    return parts.join(' ');
  });

  private barToSec(m: SongManifest, bar: number): number {
    const bpb = Math.max(1, m.time_signature?.[0] || 4);
    const secPerBar = (60 / Math.max(30, m.bpm)) * bpb;
    return m.first_downbeat_sec + (bar - 1) * secPerBar;
  }

  private secToExactBarFloat(m: SongManifest, sec: number): number {
    const bpb = Math.max(1, m.time_signature?.[0] || 4);
    const secPerBar = (60 / Math.max(30, m.bpm)) * bpb;
    return 1 + (sec - m.first_downbeat_sec) / secPerBar;
  }

  private roundSec(sec: number): number {
    if (!Number.isFinite(sec)) return 0;
    return Math.max(0, Math.round(sec * 100) / 100);
  }

  sectionStartSec(m: SongManifest, sec: SectionDef): number {
    if (sec.start_sec != null && Number.isFinite(sec.start_sec) && sec.start_sec >= 0) {
      return sec.start_sec;
    }
    return Math.max(0, this.roundSec(this.barToSec(m, sec.start_bar)));
  }

  sectionEndSec(m: SongManifest, sec: SectionDef): number {
    const start = this.sectionStartSec(m, sec);
    if (sec.end_sec != null && Number.isFinite(sec.end_sec) && sec.end_sec > start) {
      return sec.end_sec;
    }
    return Math.max(start + 0.1, this.roundSec(this.barToSec(m, sec.end_bar)));
  }

  private approxBarsForTimes(
    m: SongManifest,
    startSec: number,
    endSec: number,
  ): { start_bar: number; end_bar: number } {
    const start_bar = Math.max(1, Math.round(this.secToExactBarFloat(m, startSec)));
    const end_bar = Math.max(start_bar + 1, Math.round(this.secToExactBarFloat(m, endSec)));
    return { start_bar, end_bar };
  }

  readonly playheadBarFloat = computed(() => {
    const m = this.manifest();
    if (!m) return 1;
    return Math.max(1, this.secToExactBarFloat(m, this.snap().current_position_sec));
  });

  readonly playheadMarkBar = computed(() => {
    return Math.max(1, Math.round(this.playheadBarFloat()));
  });

  readonly activeSectionIndexAtPlayhead = computed(() => {
    const m = this.manifest();
    if (!m || m.sections.length === 0) return -1;
    const posSec = this.snap().current_position_sec;
    const bar = this.playheadMarkBar();
    const idx = m.sections.findIndex((s) =>
      s.start_sec != null && s.end_sec != null
        ? posSec >= s.start_sec && posSec < s.end_sec
        : bar >= s.start_bar && bar < s.end_bar,
    );
    return idx >= 0 ? idx : m.sections.length - 1;
  });

  readonly barTicks = computed<RenderedBarTick[]>(() => {
    const m = this.manifest();
    const dur = this.effectiveDurationSec();
    const zoom = this.waveformZoom();
    if (!m || dur <= 0) return [];
    const labelEveryBar = zoom >= 4;
    const ticks: RenderedBarTick[] = [];
    let bar = 1;
    while (bar < 400) {
      const t = this.barToSec(m, bar);
      if (t > dur) break;
      if (t >= 0) {
        const major = (bar - 1) % 4 === 0;
        ticks.push({
          bar,
          xPct: (t / dur) * 100,
          major,
          showLabel: major || labelEveryBar,
        });
      }
      bar++;
    }
    return ticks;
  });

  readonly beatTicks = computed<{ xPct: number }[]>(() => {
    const m = this.manifest();
    const dur = this.effectiveDurationSec();
    if (!m || dur <= 0 || this.waveformZoom() < 8) return [];
    const bpb = Math.max(1, m.time_signature?.[0] || 4);
    const secPerBeat = 60 / Math.max(30, m.bpm);
    const ticks: { xPct: number }[] = [];
    let bar = 1;
    while (bar < 400) {
      const barStart = this.barToSec(m, bar);
      if (barStart > dur) break;
      for (let beat = 1; beat < bpb; beat++) {
        const t = barStart + beat * secPerBeat;
        if (t >= 0 && t <= dur) {
          ticks.push({ xPct: (t / dur) * 100 });
        }
      }
      bar++;
    }
    return ticks;
  });

  readonly sectionBands = computed<RenderedSectionBand[]>(() => {
    const m = this.manifest();
    const dur = this.effectiveDurationSec();
    const selected = this.selectedSectionIdx();
    if (!m || dur <= 0) return [];
    const colorMap: Record<string, string> = {
      slate: 'bg-zinc-500/20 border-zinc-400/50 text-zinc-200',
      sky: 'bg-sky-500/20 border-sky-400/50 text-sky-200',
      violet: 'bg-violet-500/20 border-violet-400/50 text-violet-200',
      emerald: 'bg-emerald-500/20 border-emerald-400/50 text-emerald-200',
      amber: 'bg-amber-500/20 border-amber-400/50 text-amber-200',
      cyan: 'bg-cyan-500/20 border-cyan-400/50 text-cyan-200',
      rose: 'bg-rose-500/20 border-rose-400/50 text-rose-200',
    };

    return m.sections.map((sec, index) => {
      const startSec = Math.max(0, this.sectionStartSec(m, sec));
      const endSec = Math.min(dur, Math.max(startSec + 0.05, this.sectionEndSec(m, sec)));
      const leftPct = (startSec / dur) * 100;
      const widthPct = Math.max(0.8, ((endSec - startSec) / dur) * 100);
      const next = m.sections[index + 1];
      const nextStartSec = next ? Math.max(0, this.sectionStartSec(m, next)) : null;
      const sharesRightWithNext = nextStartSec !== null && Math.abs(nextStartSec - endSec) < 0.03;
      return {
        index,
        id: sec.id,
        name: sec.name,
        startBar: sec.start_bar,
        endBar: sec.end_bar,
        startSec,
        endSec,
        leftPct,
        widthPct,
        colorClass: colorMap[sec.color ?? 'slate'] ?? colorMap['slate'],
        selected: selected === index,
        boundaryLeftPct: leftPct,
        endHandleLeftPct: sharesRightWithNext ? null : Math.min(100, (endSec / dur) * 100),
      };
    });
  });

  readonly activeEditingSectionIdx = computed(() => {
    const m = this.manifest();
    if (!m || m.sections.length === 0) return -1;
    const explicit = this.selectedSectionIdx();
    if (explicit !== null && explicit >= 0 && explicit < m.sections.length) {
      return explicit;
    }
    return this.activeSectionIndexAtPlayhead();
  });

  readonly activeEditingSection = computed<SectionDef | null>(() => {
    const m = this.manifest();
    const idx = this.activeEditingSectionIdx();
    if (!m || idx < 0 || idx >= m.sections.length) return null;
    return m.sections[idx];
  });

  readonly playheadPct = computed(() => {
    const dur = this.effectiveDurationSec();
    if (dur <= 0) return 0;
    const pos = this.snap().current_position_sec;
    return Math.max(0, Math.min(100, (pos / dur) * 100));
  });

  readonly bar1Pct = computed(() => {
    const m = this.manifest();
    const dur = this.effectiveDurationSec();
    if (!m || dur <= 0) return 0;
    return Math.max(0, Math.min(100, (m.first_downbeat_sec / dur) * 100));
  });

  readonly editableStems = computed<WorkbenchStemRow[]>(() => {
    const m = this.manifest();
    if (!m) return [];
    return Object.entries(m.stems).map(([key, def]) => {
      if (typeof def === 'string') {
        const lower = key.toLowerCase();
        return {
          key,
          file: def,
          pitched: !['drums', 'percussion', 'cues', 'click'].includes(lower),
          is_cues: lower === 'cues' || lower === 'cue' || lower === 'guide',
          default_gain: 1.0,
          muted: false,
        };
      }
      return {
        key,
        file: def.file,
        pitched: def.pitched,
        is_cues: def.is_cues ?? key.toLowerCase() === 'cues',
        default_gain: def.default_gain ?? 1.0,
        muted: def.muted ?? false,
      };
    });
  });

  readonly hasVocalsStem = computed(() =>
    this.editableStems().some((s) => this.isVocalStemKey(s.key)),
  );

  readonly activePlatformGuide = computed(() => this.platformGuides[this.guidePlatform()]);

  readonly missingSelectedTool = computed<string | null>(() => {
    const ts = this.stage.toolchainStatus();
    if (!ts) return null;
    if (this.ingestTab() === 'youtube' && !ts.yt_dlp.available) {
      return 'yt-dlp (required for YouTube audio import)';
    }
    if (this.separationEngine() === 'AudioSeparator' && !ts.audio_separator.available) {
      return 'UVR (audio-separator CLI)';
    }
    if (this.separationEngine() === 'Demucs' && !ts.demucs.available) {
      return 'Demucs CLI';
    }
    return null;
  });

  readonly cachedLibraryMatch = computed<LibrarySongEntry | null>(() => {
    const entries = this.stage.libraryEntries();
    const tab = this.ingestTab();
    if (tab === 'youtube') {
      const url = this.youtubeUrl().trim();
      if (!url) return null;
      const sourceRef = canonicalizeSourceRef('youtube', url);
      const exact = entries.find(
        (e) => e.status === 'Ready' && e.provenance?.source_ref === sourceRef,
      );
      if (exact) return exact;
      const title = (this.songTitleInput().trim() || this.youtubeMeta()?.title || '').toLowerCase();
      if (title) {
        return (
          entries.find(
            (e) => e.status === 'Ready' && !e.provenance && e.title.toLowerCase() === title,
          ) ?? null
        );
      }
      return null;
    }
    if (tab === 'file') {
      const path = this.localFilePath().trim();
      if (!path) return null;
      const sourceRef = canonicalizeSourceRef('local_file', path);
      return (
        entries.find(
          (e) => e.status === 'Ready' && e.provenance?.source_ref === sourceRef,
        ) ?? null
      );
    }
    return null;
  });

  readonly cachedSeparationChanged = computed(() => {
    const prov = this.cachedLibraryMatch()?.provenance;
    if (!prov) return false;
    if (prov.separation_engine !== this.separationEngine()) return true;
    if (prov.stem_preset !== this.stemPreset()) return true;
    if (
      this.stemPreset() === 'Custom' &&
      (prov.custom_model ?? '') !== this.customUvrModel().trim()
    ) {
      return true;
    }
    return false;
  });

  readonly isJobRunning = computed(() => this.stage.workbenchJob()?.status === 'Running');

  onSourceInputChanged(): void {
    this.forceReextract.set(false);
  }

  async quickAddCachedToSetlist(entry: LibrarySongEntry): Promise<void> {
    await this.stage.sendCommand({
      type: 'AddSetlistSong',
      payload: { dir: entry.dir, insert_at: null },
    });
    this.workbenchNotice.set(`Added "${entry.title}" from Library to Setlist without re-extracting.`);
  }

  prepareReextractFromManifest(): void {
    const m = this.manifest();
    const orig = m?.provenance?.original_source;
    if (!m || !orig) return;
    if (m.provenance?.source_kind === 'youtube') {
      this.ingestTab.set('youtube');
      this.youtubeUrl.set(orig);
    } else {
      this.ingestTab.set('file');
      this.localFilePath.set(orig);
    }
    this.songTitleInput.set(m.title);
    this.forceReextract.set(true);
    this.workbenchNotice.set(
      'Force Re-Extract enabled — choose a different engine/preset/model on the left and click Re-Extract Stems.',
    );
  }

  openToolchainGuide(): void {
    this.showToolchainDrawer.set(true);
  }

  async browseToolBinary(tool: 'ffmpeg' | 'yt_dlp' | 'audio_separator' | 'demucs'): Promise<void> {
    const labels: Record<typeof tool, string> = {
      ffmpeg: 'FFmpeg',
      yt_dlp: 'yt-dlp',
      audio_separator: 'audio-separator (UVR)',
      demucs: 'Demucs',
    };
    const picked = await this.stage.pickExecutableFile(labels[tool]);
    if (!picked) return;
    if (tool === 'ffmpeg') this.pathFfmpeg.set(picked);
    else if (tool === 'yt_dlp') this.pathYtDlp.set(picked);
    else if (tool === 'audio_separator') this.pathAudioSeparator.set(picked);
    else this.pathDemucs.set(picked);
    await this.saveToolchainPaths();
  }

  async browseAudioFile(): Promise<void> {
    const picked = await this.stage.pickAudioFile();
    if (picked) {
      this.localFilePath.set(picked);
      if (!this.songTitleInput().trim()) {
        const base = picked.split(/[/\\]/).pop()?.replace(/\.[^.]+$/, '') ?? '';
        this.songTitleInput.set(base);
      }
    }
  }

  async browseFolderForWorkbench(): Promise<void> {
    const picked = await this.stage.pickSongFolder();
    if (picked) {
      await this.openFolderInWorkbench(picked);
    }
  }

  async fetchYouTubeInfo(): Promise<void> {
    const url = this.youtubeUrl().trim();
    if (!url) {
      this.workbenchError.set('Paste a YouTube video URL before clicking Lookup.');
      return;
    }
    this.youtubeProbing.set(true);
    this.workbenchError.set(null);
    this.workbenchNotice.set(null);
    try {
      const { meta, error } = await this.stage.probeYouTubeUrl(url);
      if (meta) {
        this.youtubeMeta.set(meta);
        if (!this.songTitleInput().trim()) {
          this.songTitleInput.set(meta.title);
        }
      } else if (error) {
        this.workbenchError.set(error);
        if (error.toLowerCase().includes('yt-dlp') || error.toLowerCase().includes('not found')) {
          this.showToolchainDrawer.set(true);
        }
      }
    } finally {
      this.youtubeProbing.set(false);
    }
  }

  async startImportPipeline(): Promise<void> {
    this.workbenchError.set(null);
    this.workbenchNotice.set(null);

    if (this.ingestTab() === 'file') {
      const path = this.localFilePath().trim();
      if (!path) {
        this.workbenchError.set('Select or enter a local audio file path first.');
        return;
      }
      await this.stage.startImportJob({
        source: { kind: 'LocalFile', value: path },
        song_title: this.songTitleInput().trim() || null,
        target_dir: null,
        separation_engine: this.separationEngine(),
        stem_preset: this.stemPreset(),
        custom_model: this.customUvrModel().trim() || null,
        force_reextract: this.forceReextract(),
      });
    } else if (this.ingestTab() === 'youtube') {
      const url = this.youtubeUrl().trim();
      if (!url) {
        this.workbenchError.set('Paste a YouTube video URL first.');
        return;
      }
      await this.stage.startImportJob({
        source: { kind: 'YouTubeUrl', value: url },
        song_title: this.songTitleInput().trim() || this.youtubeMeta()?.title || null,
        target_dir: null,
        separation_engine: this.separationEngine(),
        stem_preset: this.stemPreset(),
        custom_model: this.customUvrModel().trim() || null,
        force_reextract: this.forceReextract(),
      });
    }
  }

  async openFolderInWorkbench(dir: string): Promise<void> {
    const trimmed = dir.trim();
    if (!trimmed) return;
    const seq = ++this.openSeq;
    this.workbenchError.set(null);
    this.workbenchNotice.set(null);
    this.analysis.set(null);
    if (this.tapAlignTimer) {
      clearTimeout(this.tapAlignTimer);
      this.tapAlignTimer = null;
    }
    this.tapTimestamps = [];
    this.tapCount.set(0);
    this.resetBeat1Taps();
    this.draggingTarget.set(null);
    this.dragCanvasEl = null;
    this.soloedStems.set([]);
    this.preSoloMuted = null;
    this.loadedDir.set(trimmed);
    this.loadingBundle.set(true);

    let inspected = null;
    try {
      inspected = await this.stage.inspectSongFolder(trimmed);
    } finally {
      if (seq === this.openSeq) {
        this.loadingBundle.set(false);
      }
    }
    if (seq !== this.openSeq) return;
    if (!inspected) {
      this.workbenchError.set(`Could not inspect folder: ${trimmed}`);
      return;
    }
    this.manifest.set(inspected.manifest);
    this.generatedCuePlacements.set([]);
    const hasAutoCues = ['cues', 'section_cues'].some((k) => {
      const s = inspected.manifest.stems?.[k];
      const file = typeof s === 'string' ? s : s?.file;
      return file === 'section-cues.wav';
    });
    this.autoRegenerateCuesOnSave.set(hasAutoCues);

    // Preload audio into engine immediately if manifest timing already exists
    if (inspected.has_existing_manifest) {
      void this.stage
        .sendCommand({ type: 'LoadSongDir', payload: trimmed })
        .then(() => {
          if (seq === this.openSeq) {
            return this.syncEngineTiming();
          }
          return undefined;
        });
    }

    this.analyzingWaveform.set(true);
    try {
      const peaks = await this.stage.analyzeWaveform(trimmed, true, 640);
      if (seq !== this.openSeq) return;
      if (peaks) {
        this.analysis.set(peaks);
        if (!inspected.has_existing_manifest) {
          this.manifest.update((cur) =>
            cur
              ? {
                ...cur,
                bpm: peaks.estimated_bpm,
                first_downbeat_sec: peaks.estimated_first_downbeat_sec,
                original_key: peaks.estimated_key ?? 'Unknown',
              }
              : null,
          );
        }
      }
    } finally {
      if (seq === this.openSeq) {
        this.analyzingWaveform.set(false);
      }
    }

    if (seq === this.openSeq && !inspected.has_existing_manifest) {
      await this.stage.sendCommand({ type: 'LoadSongDir', payload: trimmed });
      await this.syncEngineTiming();
    }
  }

  async reanalyzeWaveform(): Promise<void> {
    const dir = this.loadedDir();
    if (!dir) return;
    this.analyzingWaveform.set(true);
    try {
      const result = await this.stage.analyzeWaveform(dir, true, 640, undefined, undefined, true);
      if (result) {
        this.analysis.set(result);
        this.manifest.update((m) =>
          m
            ? {
              ...m,
              bpm: result.estimated_bpm,
              first_downbeat_sec: result.estimated_first_downbeat_sec,
              original_key: result.estimated_key ?? 'Unknown',
            }
            : null,
        );
        await this.syncEngineTiming();
        const keyNote = result.estimated_key
          ? `Key ${result.estimated_key} (${Math.round((result.key_confidence ?? 0) * 100)}%)`
          : result.key_candidates?.length
            ? `Key uncertain (closest: ${result.key_candidates.join(' / ')}) — set to Unknown`
            : 'Key could not be detected reliably — set to Unknown';
        this.workbenchNotice.set(
          `Detected ${result.estimated_bpm} BPM (Bar 1 @ ${result.estimated_first_downbeat_sec.toFixed(2)}s), ${keyNote} & synced metronome`,
        );
      }
    } finally {
      this.analyzingWaveform.set(false);
    }
  }

  async applyDetectedTiming(): Promise<void> {
    const detected = this.analysis();
    if (!detected) {
      await this.reanalyzeWaveform();
      return;
    }
    this.manifest.update((m) =>
      m
        ? {
          ...m,
          bpm: detected.estimated_bpm,
          first_downbeat_sec: detected.estimated_first_downbeat_sec,
          original_key: detected.estimated_key ?? 'Unknown',
        }
        : null,
    );
    await this.syncEngineTiming();
    const keyNote = detected.estimated_key
      ? `, Key ${detected.estimated_key}`
      : ' (Key set to Unknown — verify manually)';
    this.workbenchNotice.set(
      `Applied detected ${detected.estimated_bpm} BPM${keyNote} & synced metronome click`,
    );
  }

  async syncEngineTiming(): Promise<void> {
    const m = this.manifest();
    if (!m) return;
    await this.stage.sendCommand({
      type: 'SetSongTiming',
      payload: {
        bpm: Math.max(30, Math.min(260, Number(m.bpm) || 120)),
        first_downbeat_sec: Math.max(0, Number(m.first_downbeat_sec) || 0),
        beats_per_bar: Math.max(1, Math.min(16, Number(m.time_signature?.[0]) || 4)),
        click_mode: m.click_mode ?? 'AlwaysOn',
        sections: m.sections,
      },
    });
  }

  setZoom(nextZoom: number, anchorClientX?: number): void {
    const clamped = Math.max(1, Math.min(32, Math.round(nextZoom * 100) / 100));
    const container = this.waveformScrollRef?.nativeElement;
    if (!container) {
      this.waveformZoom.set(clamped);
      return;
    }

    const rect = container.getBoundingClientRect();
    const viewW = container.clientWidth || rect.width;
    let anchorFrac = this.playheadPct() / 100;
    let anchorViewportX = viewW * 0.5;
    if (anchorClientX !== undefined && viewW > 0) {
      const totalWidth = Math.max(viewW, container.scrollWidth);
      anchorViewportX = Math.max(0, Math.min(viewW, anchorClientX - rect.left));
      anchorFrac = Math.max(0, Math.min(1, (container.scrollLeft + anchorViewportX) / totalWidth));
    }

    this.waveformZoom.set(clamped);
    const applyScroll = (): void => {
      const newTotalW = Math.max(container.clientWidth, viewW * clamped);
      container.scrollLeft = Math.max(0, anchorFrac * newTotalW - anchorViewportX);
    };
    applyScroll();
    if (typeof requestAnimationFrame === 'function') {
      requestAnimationFrame(applyScroll);
    }
  }

  zoomIn(anchorClientX?: number): void {
    const cur = this.waveformZoom();
    const next = this.zoomSteps.find((s) => s > cur + 0.01) ?? Math.min(32, cur * 2);
    this.setZoom(next, anchorClientX);
  }

  zoomOut(anchorClientX?: number): void {
    const cur = this.waveformZoom();
    const prev = [...this.zoomSteps].reverse().find((s) => s < cur - 0.01) ?? 1;
    this.setZoom(prev, anchorClientX);
  }

  centerOnPlayhead(): void {
    const container = this.waveformScrollRef?.nativeElement;
    if (!container) return;
    const totalW = Math.max(container.clientWidth, container.scrollWidth);
    const playX = (this.playheadPct() / 100) * totalW;
    container.scrollLeft = Math.max(0, playX - container.clientWidth * 0.5);
  }

  private ensurePlayheadInView(posSec: number): void {
    const container = this.waveformScrollRef?.nativeElement;
    const dur = this.effectiveDurationSec();
    if (!container || dur <= 0 || container.clientWidth <= 0) return;
    const totalW = Math.max(container.clientWidth, container.scrollWidth);
    const playX = Math.max(0, Math.min(1, posSec / dur)) * totalW;
    const viewStart = container.scrollLeft;
    const viewEnd = viewStart + container.clientWidth;
    if (playX < viewStart + container.clientWidth * 0.08 || playX > viewEnd - container.clientWidth * 0.15) {
      container.scrollLeft = Math.max(0, playX - container.clientWidth * 0.25);
    }
  }

  onWaveformWheel(event: WheelEvent): void {
    if (!event.ctrlKey && !event.metaKey && !event.altKey) return;
    event.preventDefault();
    const factor = event.deltaY < 0 ? 1.35 : 1 / 1.35;
    const next = this.waveformZoom() * factor;
    this.setZoom(next < 1.05 ? 1 : next, event.clientX);
  }

  nudgePlayheadBeats(beats: number): void {
    const m = this.manifest();
    if (!m) return;
    const spb = 60 / Math.max(30, m.bpm);
    this.nudgePlayheadSec(beats * spb);
  }

  nudgePlayheadSec(deltaSec: number): void {
    const dur = this.effectiveDurationSec();
    const next = Math.max(0, Math.min(dur, this.snap().current_position_sec + deltaSec));
    void this.stage.sendCommand({ type: 'SeekSec', payload: next });
    if (this.waveformZoom() > 1) {
      this.ensurePlayheadInView(next);
    }
  }

  seekToSection(index: number): void {
    const m = this.manifest();
    const sec = m?.sections[index];
    if (!m || !sec) return;
    this.selectedSectionIdx.set(index);
    const targetSec = Math.max(0, Math.min(this.effectiveDurationSec(), this.sectionStartSec(m, sec)));
    void this.stage.sendCommand({ type: 'SeekSec', payload: targetSec });
    if (this.waveformZoom() > 1) {
      this.centerOnPlayhead();
    }
  }

  private clientXToSec(clientX: number, el: HTMLElement): number {
    const rect = el.getBoundingClientRect();
    if (rect.width <= 0) return 0;
    const ratio = Math.max(0, Math.min(1, (clientX - rect.left) / rect.width));
    return ratio * this.effectiveDurationSec();
  }

  private updateBar1SecLocal(sec: number, syncEngine: boolean): void {
    const rounded = Math.max(0, Math.round(sec * 1000) / 1000);
    this.manifest.update((cur) => (cur ? { ...cur, first_downbeat_sec: rounded } : cur));
    if (syncEngine) {
      void this.syncEngineTiming();
    }
  }

  onWaveformPointerDown(event: PointerEvent): void {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    const target = event.currentTarget as HTMLElement | null;
    if (!target) return;
    this.dragCanvasEl = target;
    this.draggingTarget.set('playhead');
    target.setPointerCapture?.(event.pointerId);
    const seekSec = this.clientXToSec(event.clientX, target);
    this.lastDragSeekAt = performance.now();
    void this.stage.sendCommand({ type: 'SeekSec', payload: seekSec });
  }

  onBar1PointerDown(event: PointerEvent, canvasEl: HTMLElement): void {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    event.stopPropagation();
    event.preventDefault();
    this.resetBeat1Taps();
    this.dragCanvasEl = canvasEl;
    this.draggingTarget.set('bar1');
    (event.currentTarget as HTMLElement | null)?.setPointerCapture?.(event.pointerId);
    this.lastDragTimingSyncAt = performance.now();
    const sec = this.clientXToSec(event.clientX, canvasEl);
    this.updateBar1SecLocal(sec, false);
  }

  onBoundaryPointerDown(
    event: PointerEvent,
    sectionIndex: number,
    canvasEl: HTMLElement,
    edge: 'start' | 'end' = 'start',
  ): void {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    event.stopPropagation();
    event.preventDefault();
    this.selectedSectionIdx.set(sectionIndex);
    this.dragCanvasEl = canvasEl;
    this.dragBoundarySectionIdx = sectionIndex;
    this.dragBoundaryEdge = edge;
    this.draggingTarget.set('boundary');
    (event.currentTarget as HTMLElement | null)?.setPointerCapture?.(event.pointerId);
    this.lastDragTimingSyncAt = performance.now();
    this.applyBoundaryDragAtSec(
      sectionIndex,
      this.clientXToSec(event.clientX, canvasEl),
      false,
      event.altKey,
    );
  }

  private applyBoundaryDragAtSec(
    sectionIndex: number,
    sec: number,
    syncEngine: boolean,
    detachContiguous = false,
  ): void {
    const m = this.manifest();
    if (!m || sectionIndex < 0 || sectionIndex >= m.sections.length) return;
    const dur = Math.max(1, this.effectiveDurationSec());
    const rawSec = this.roundSec(Math.max(0, Math.min(dur, sec)));
    const cur = m.sections[sectionIndex];
    const curStart = this.sectionStartSec(m, cur);
    const curEnd = this.sectionEndSec(m, cur);
    const minDur = 0.1;

    if (this.dragBoundaryEdge === 'end') {
      const next = sectionIndex + 1 < m.sections.length ? m.sections[sectionIndex + 1] : null;
      const nextStart = next ? this.sectionStartSec(m, next) : dur;
      const nextEnd = next ? this.sectionEndSec(m, next) : dur;
      const wasContiguous = next !== null && Math.abs(nextStart - curEnd) < 0.03;
      const maxSec = next
        ? (wasContiguous && !detachContiguous ? nextEnd - minDur : nextStart - minDur)
        : dur;
      const minSec = curStart + minDur;
      if (minSec > maxSec) return;
      const clampedEnd = this.roundSec(Math.max(minSec, Math.min(maxSec, rawSec)));
      if (Math.abs(curEnd - clampedEnd) >= 0.005 || cur.end_sec == null) {
        const nextSections = m.sections.map((s, idx) => {
          if (idx === sectionIndex) {
            return {
              ...s,
              start_sec: curStart,
              end_sec: clampedEnd,
              ...this.approxBarsForTimes(m, curStart, clampedEnd),
            };
          }
          if (next && idx === sectionIndex + 1 && wasContiguous && !detachContiguous) {
            return {
              ...s,
              start_sec: clampedEnd,
              end_sec: nextEnd,
              ...this.approxBarsForTimes(m, clampedEnd, nextEnd),
            };
          }
          return s;
        });
        this.manifest.set({ ...m, sections: nextSections });
      }
      if (syncEngine) {
        void this.syncEngineTiming();
      }
      return;
    }

    const prev = sectionIndex > 0 ? m.sections[sectionIndex - 1] : null;
    const prevStart = prev ? this.sectionStartSec(m, prev) : 0;
    const prevEnd = prev ? this.sectionEndSec(m, prev) : 0;
    const wasContiguous = prev !== null && Math.abs(prevEnd - curStart) < 0.03;
    const minSec = prev
      ? (wasContiguous && !detachContiguous ? prevStart + minDur : prevEnd + minDur)
      : 0;
    const maxSec = curEnd - minDur;
    if (minSec > maxSec) return;
    const clampedStart = this.roundSec(Math.max(minSec, Math.min(maxSec, rawSec)));
    if (Math.abs(curStart - clampedStart) >= 0.005 || cur.start_sec == null || (prev && wasContiguous && prev.end_sec == null)) {
      const nextSections = m.sections.map((s, idx) => {
        if (prev && idx === sectionIndex - 1 && wasContiguous && !detachContiguous) {
          return {
            ...s,
            start_sec: prevStart,
            end_sec: clampedStart,
            ...this.approxBarsForTimes(m, prevStart, clampedStart),
          };
        }
        if (idx === sectionIndex) {
          return {
            ...s,
            start_sec: clampedStart,
            end_sec: curEnd,
            ...this.approxBarsForTimes(m, clampedStart, curEnd),
          };
        }
        return s;
      });
      this.manifest.set({ ...m, sections: nextSections });
    }
    if (syncEngine) {
      void this.syncEngineTiming();
    }
  }

  @HostListener('window:pointermove', ['$event'])
  onWindowPointerMove(event: PointerEvent): void {
    const mode = this.draggingTarget();
    const el = this.dragCanvasEl;
    if (!mode || !el) return;
    const now = performance.now();
    const sec = this.clientXToSec(event.clientX, el);
    if (mode === 'playhead') {
      if (now - this.lastDragSeekAt >= 25) {
        this.lastDragSeekAt = now;
        void this.stage.sendCommand({ type: 'SeekSec', payload: sec });
      }
    } else if (mode === 'boundary' && this.dragBoundarySectionIdx !== null) {
      const shouldSync = now - this.lastDragTimingSyncAt >= 65;
      if (shouldSync) {
        this.lastDragTimingSyncAt = now;
      }
      this.applyBoundaryDragAtSec(this.dragBoundarySectionIdx, sec, shouldSync, event.altKey);
    } else {
      const shouldSync = now - this.lastDragTimingSyncAt >= 65;
      if (shouldSync) {
        this.lastDragTimingSyncAt = now;
      }
      this.updateBar1SecLocal(sec, shouldSync);
    }
  }

  @HostListener('window:pointerup', ['$event'])
  @HostListener('window:pointercancel', ['$event'])
  onWindowPointerUp(event: PointerEvent): void {
    const mode = this.draggingTarget();
    const el = this.dragCanvasEl;
    const boundaryIdx = this.dragBoundarySectionIdx;
    if (!mode) return;
    this.draggingTarget.set(null);
    this.dragCanvasEl = null;
    this.dragBoundarySectionIdx = null;
    if (!el) return;
    const sec = this.clientXToSec(event.clientX, el);
    if (mode === 'playhead') {
      void this.stage.sendCommand({ type: 'SeekSec', payload: sec });
    } else if (mode === 'boundary' && boundaryIdx !== null) {
      const edge = this.dragBoundaryEdge;
      this.applyBoundaryDragAtSec(boundaryIdx, sec, true, event.altKey);
      const mAfter = this.manifest();
      const secAfter = mAfter?.sections[boundaryIdx];
      if (mAfter && secAfter) {
        const sTime = this.formatTimePrecise(this.sectionStartSec(mAfter, secAfter));
        const eTime = this.formatTimePrecise(this.sectionEndSec(mAfter, secAfter));
        this.workbenchNotice.set(
          edge === 'end'
            ? `Updated "${secAfter.name}" end to ${eTime} (live synced to Stage)`
            : `Updated "${secAfter.name}" start to ${sTime} (live synced to Stage)`,
        );
      }
    } else {
      const rounded = Math.max(0, Math.round(sec * 1000) / 1000);
      this.updateBar1SecLocal(rounded, true);
      this.workbenchNotice.set(`Bar 1 aligned to ${rounded.toFixed(2)}s`);
    }
  }

  async onWaveformClick(event: MouseEvent): Promise<void> {
    const target = event.currentTarget as HTMLElement | null;
    if (!target) return;
    const seekSec = this.clientXToSec(event.clientX, target);
    await this.stage.sendCommand({ type: 'SeekSec', payload: seekSec });
  }

  async toggleAuditionPlay(): Promise<void> {
    await this.stage.sendCommand({ type: 'PlayStop' });
  }

  async toggleAuditionClick(): Promise<void> {
    await this.stage.sendCommand({
      type: 'SetClickMuted',
      payload: !this.snap().click_muted,
    });
  }

  tapTempo(): void {
    const now = performance.now();
    if (this.tapTimestamps.length > 0 && now - this.tapTimestamps[this.tapTimestamps.length - 1] > 2200) {
      this.tapTimestamps = [];
    }
    this.tapTimestamps.push(now);
    if (this.tapTimestamps.length > 8) {
      this.tapTimestamps = this.tapTimestamps.slice(-8);
    }
    this.tapCount.set(this.tapTimestamps.length);
    if (this.tapTimestamps.length >= 2) {
      const intervals: number[] = [];
      for (let i = 1; i < this.tapTimestamps.length; i++) {
        intervals.push(this.tapTimestamps[i] - this.tapTimestamps[i - 1]);
      }
      const avgMs = intervals.reduce((a, b) => a + b, 0) / intervals.length;
      if (avgMs > 200 && avgMs < 2000) {
        const bpm = Math.max(30, Math.min(260, Math.round((60000 / avgMs) * 2) / 2));
        this.updateManifestField('bpm', bpm);
        if (this.tapAlignTimer) {
          clearTimeout(this.tapAlignTimer);
          this.tapAlignTimer = null;
        }
        if (this.loadedDir()) {
          const settleDelayMs = Math.max(1100, Math.min(1800, Math.round(avgMs * 1.4)));
          this.tapAlignTimer = setTimeout(() => {
            this.tapAlignTimer = null;
            void this.alignBeat1ToCurrentBpm(true);
          }, settleDelayMs);
        }
      }
    }
  }

  scaleBpm(factor: number): void {
    const cur = this.manifest()?.bpm;
    if (!cur) return;
    const next = Math.max(30, Math.min(260, Math.round(cur * factor * 2) / 2));
    this.updateManifestField('bpm', next);
    if (this.loadedDir()) {
      void this.alignBeat1ToCurrentBpm(true);
    }
  }

  async alignBeat1ToCurrentBpm(fromTap = false): Promise<void> {
    const dir = this.loadedDir();
    const m = this.manifest();
    if (!dir || !m || this.aligningDownbeat()) return;
    this.aligningDownbeat.set(true);
    try {
      const result = await this.stage.analyzeWaveform(
        dir,
        true,
        640,
        m.bpm,
        m.time_signature?.[0] ?? 4,
      );
      if (result) {
        this.analysis.set(result);
        const snapped = Math.max(0, Math.round(result.estimated_first_downbeat_sec * 1000) / 1000);
        this.updateManifestField('first_downbeat_sec', snapped);
        this.workbenchNotice.set(
          fromTap
            ? `Tap tempo ${m.bpm} BPM • Aligned Beat 1 @ ${snapped.toFixed(2)}s`
            : `Aligned Beat 1 @ ${snapped.toFixed(2)}s for ${m.bpm} BPM`,
        );
      }
    } finally {
      this.aligningDownbeat.set(false);
    }
  }

  setDownbeatToPlayhead(): void {
    this.resetBeat1Taps();
    const pos = Math.round(this.snap().current_position_sec * 1000) / 1000;
    this.updateManifestField('first_downbeat_sec', Math.max(0, pos));
  }

  tapBeat1(): void {
    const m = this.manifest();
    if (!m) return;
    const snap = this.snap();
    const pos = snap.current_position_sec;
    if (!snap.is_playing && pos <= 0.001) {
      this.workbenchNotice.set('Press Play Audition (or scrub cursor to a downbeat), then tap Beat 1.');
      return;
    }

    const bpb = Math.max(1, Number(m.time_signature?.[0]) || 4);
    let nextBpm = Math.max(30, Math.min(260, Number(m.bpm) || 120));
    const curSecPerBar = (60 / nextBpm) * bpb;
    const minBarSec = (60 * bpb) / 260;
    const maxBarSec = (60 * bpb) / 30;

    const now = performance.now();
    const maxWindowMs = Math.max(10000, maxBarSec * 3000);
    const prev = this.beat1TapRecords[this.beat1TapRecords.length - 1];

    if (
      !prev ||
      !snap.is_playing ||
      now - this.lastBeat1TapAt > maxWindowMs ||
      pos <= prev.pos + 0.001
    ) {
      this.beat1TapRecords = [{ pos, barIdx: 0 }];
    } else if (this.beat1TapRecords.length === 1) {
      const dt = pos - prev.pos;
      if (dt >= minBarSec * 0.96 && dt <= maxBarSec * 1.04) {
        this.beat1TapRecords = [prev, { pos, barIdx: 1 }];
      } else {
        // Out-of-range interval (e.g. accidental double tap or long skip): start fresh at this tap.
        this.beat1TapRecords = [{ pos, barIdx: 0 }];
      }
    } else {
      const establishedBarSec = this.estimateBarSecFromTaps(this.beat1TapRecords) ?? curSecPerBar;
      const dt = pos - prev.pos;
      const barsFloat = dt / establishedBarSec;
      const barsNearest = Math.round(barsFloat);
      if (barsNearest >= 1 && barsNearest <= 4 && Math.abs(barsFloat - barsNearest) <= 0.28) {
        const candidate = [...this.beat1TapRecords, { pos, barIdx: prev.barIdx + barsNearest }].slice(-8);
        const candidateBarSec = this.estimateBarSecFromTaps(candidate);
        if (candidateBarSec && candidateBarSec >= minBarSec * 0.96 && candidateBarSec <= maxBarSec * 1.04) {
          this.beat1TapRecords = candidate;
        } else {
          this.beat1TapRecords = [{ pos, barIdx: 0 }];
        }
      } else {
        // User tapped a different beat (e.g. correcting from Beat 3 to Beat 1): start fresh.
        this.beat1TapRecords = [{ pos, barIdx: 0 }];
      }
    }

    this.lastBeat1TapAt = now;
    this.beat1TapCount.set(this.beat1TapRecords.length);

    if (this.beat1TapRecords.length >= 2) {
      const estBarSec = this.estimateBarSecFromTaps(this.beat1TapRecords);
      if (estBarSec && estBarSec >= minBarSec * 0.96 && estBarSec <= maxBarSec * 1.04) {
        nextBpm = Math.max(30, Math.min(260, Math.round(((60 * bpb) / estBarSec) * 2) / 2));
      }
    }

    const secPerBar = (60 / nextBpm) * bpb;
    const phases = this.beat1TapRecords.map((r) => ((r.pos % secPerBar) + secPerBar) % secPerBar);
    let aligned = this.circularMeanPhase(phases, secPerBar);
    if (secPerBar - aligned < 0.005) {
      aligned = 0;
    }
    const roundedDownbeat = Math.max(0, Math.round(aligned * 1000) / 1000);

    this.manifest.update((cur) => {
      if (!cur) return cur;
      return {
        ...cur,
        bpm: nextBpm,
        first_downbeat_sec: roundedDownbeat,
      };
    });
    void this.syncEngineTiming();

    this.workbenchNotice.set(
      this.beat1TapRecords.length > 1
        ? `${nextBpm} BPM • Bar 1 aligned to ${roundedDownbeat.toFixed(2)}s (${this.beat1TapRecords.length} Beat 1 taps)`
        : `Bar 1 aligned to ${roundedDownbeat.toFixed(2)}s (from Beat 1 tap @ ${pos.toFixed(2)}s — tap Beat 1 again for tempo)`,
    );
  }

  private resetBeat1Taps(): void {
    this.beat1TapRecords = [];
    this.lastBeat1TapAt = 0;
    this.beat1TapCount.set(0);
  }

  private estimateBarSecFromTaps(records: { pos: number; barIdx: number }[]): number | null {
    if (records.length < 2) return null;
    let sumX = 0;
    let sumY = 0;
    for (const r of records) {
      sumX += r.barIdx;
      sumY += r.pos;
    }
    const meanX = sumX / records.length;
    const meanY = sumY / records.length;
    let num = 0;
    let den = 0;
    for (const r of records) {
      const dx = r.barIdx - meanX;
      num += dx * (r.pos - meanY);
      den += dx * dx;
    }
    if (den <= 0 || num <= 0) return null;
    return num / den;
  }

  private circularMeanPhase(phases: number[], period: number): number {
    if (phases.length === 0 || period <= 0) return 0;
    if (phases.length === 1) return phases[0];
    let sumSin = 0;
    let sumCos = 0;
    for (const p of phases) {
      const angle = (p / period) * 2 * Math.PI;
      sumSin += Math.sin(angle);
      sumCos += Math.cos(angle);
    }
    let meanAngle = Math.atan2(sumSin, sumCos);
    if (meanAngle < 0) meanAngle += 2 * Math.PI;
    return (meanAngle / (2 * Math.PI)) * period;
  }

  shiftDownbeatBeats(beats: number): void {
    const m = this.manifest();
    if (!m) return;
    const spb = 60 / Math.max(30, m.bpm);
    const bpb = Math.max(1, m.time_signature?.[0] ?? 4);
    const cur = m.first_downbeat_sec ?? 0;
    let next = cur + beats * spb;
    if (next < -0.002 && beats < 0) {
      // If subtracting 1 beat crosses 0, rotate Beat 1 within the first bar (e.g. +3 beats in 4/4)
      next = cur + (bpb + beats) * spb;
    }
    next = Math.max(0, Math.round(next * 1000) / 1000);
    this.updateManifestField('first_downbeat_sec', next);
    this.workbenchNotice.set(`Beat 1 shifted to ${next.toFixed(2)}s (${beats > 0 ? '+' : ''}${beats} beat)`);
  }

  nudgeDownbeat(deltaSec: number): void {
    const cur = this.manifest()?.first_downbeat_sec ?? 0;
    const next = Math.max(0, Math.round((cur + deltaSec) * 1000) / 1000);
    this.updateManifestField('first_downbeat_sec', next);
  }

  introEndBar(): number {
    const sections = this.manifest()?.sections ?? [];
    const intro = sections.find((s) => s.name.toLowerCase().includes('intro'));
    if (intro) return intro.end_bar;
    if (sections.length > 1 && sections[0].start_bar === 1) {
      return sections[0].end_bar;
    }
    return 5;
  }

  updateManifestField(field: 'title' | 'artist' | 'bpm' | 'original_key' | 'first_downbeat_sec' | 'ts_num' | 'click_mode', value: unknown): void {
    this.manifest.update((cur) => {
      if (!cur) return cur;
      if (field === 'ts_num') {
        const n = Math.max(1, Math.min(16, Number(value) || 4));
        return { ...cur, time_signature: [n, cur.time_signature[1]] };
      }
      if (field === 'bpm') {
        return { ...cur, bpm: Math.max(30, Math.min(260, Number(value) || 120)) };
      }
      if (field === 'click_mode') {
        return { ...cur, click_mode: (value as ClickMode) || 'AlwaysOn' };
      }
      if (field === 'first_downbeat_sec') {
        const nextDownbeat = Math.max(0, Number(value) || 0);
        return { ...cur, first_downbeat_sec: nextDownbeat };
      }
      return { ...cur, [field]: value };
    });
    if (field === 'bpm' || field === 'first_downbeat_sec' || field === 'ts_num' || field === 'click_mode') {
      void this.syncEngineTiming();
    }
  }

  private buildUniqueSectionId(name: string, index: number): string {
    const slug = name.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'section';
    return `${slug}-${index + 1}-${Date.now().toString(36).slice(-3)}`;
  }

  private resolveSectionDisplayName(
    sections: SectionDef[],
    rawName: string,
    replacingIndex: number | null,
  ): string {
    const trimmed = rawName.trim();
    if (!trimmed) return 'Section';
    if (trimmed === 'Verse' || trimmed === 'Chorus') {
      const countOther = sections.filter(
        (s, idx) => idx !== replacingIndex && s.name.startsWith(trimmed),
      ).length;
      return countOther > 0 ? `${trimmed} ${countOther + 1}` : trimmed;
    }
    return trimmed;
  }

  @HostListener('window:keydown', ['$event'])
  onWorkbenchKeydown(event: KeyboardEvent): void {
    if (!this.manifest()) return;
    const target = event.target as HTMLElement | null;
    const tag = target?.tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA' || target?.isContentEditable) {
      return;
    }
    if (event.code === 'Space') {
      event.preventDefault();
      void this.toggleAuditionPlay();
    } else if (event.key === '=' || event.key === '+' || event.key === ']') {
      event.preventDefault();
      this.zoomIn();
    } else if (event.key === '-' || event.key === '[') {
      event.preventDefault();
      this.zoomOut();
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault();
      if (event.altKey) {
        this.nudgePlayheadSec(-0.025);
      } else if (event.shiftKey) {
        this.nudgePlayheadBeats(-(this.manifest()?.time_signature?.[0] || 4));
      } else {
        this.nudgePlayheadBeats(-1);
      }
    } else if (event.key === 'ArrowRight') {
      event.preventDefault();
      if (event.altKey) {
        this.nudgePlayheadSec(0.025);
      } else if (event.shiftKey) {
        this.nudgePlayheadBeats(this.manifest()?.time_signature?.[0] || 4);
      } else {
        this.nudgePlayheadBeats(1);
      }
    } else if ((event.key === 's' || event.key === 'S') && !event.ctrlKey && !event.metaKey) {
      event.preventDefault();
      this.splitAndFocusCustomSection();
    }
  }

  markSectionAtPlayhead(rawName: string, color = 'sky', defaultBars = 8): void {
    const m = this.manifest();
    const cleaned = rawName.trim();
    if (!m || !cleaned) return;
    const dur = Math.max(1, this.effectiveDurationSec());
    const markSec = this.roundSec(Math.min(dur, this.snap().current_position_sec));
    const secPerBar = (60 / Math.max(30, m.bpm)) * Math.max(1, m.time_signature?.[0] || 4);
    const defaultSpanSec = Math.max(1, defaultBars * secPerBar);
    const sections = [...m.sections];

    const exactStartIdx = sections.findIndex(
      (s) => Math.abs(this.sectionStartSec(m, s) - markSec) <= 0.08,
    );
    if (exactStartIdx >= 0) {
      const target = sections[exactStartIdx];
      const displayName =
        target.name.toLowerCase() === cleaned.toLowerCase()
          ? target.name
          : this.resolveSectionDisplayName(sections, cleaned, exactStartIdx);
      sections[exactStartIdx] = {
        ...target,
        id: this.buildUniqueSectionId(displayName, exactStartIdx),
        name: displayName,
        color,
      };
      this.manifest.set({ ...m, sections });
      this.selectedSectionIdx.set(exactStartIdx);
      this.workbenchNotice.set(
        `Named ${this.formatTimePrecise(markSec)} section "${displayName}" & synced to Stage`,
      );
      void this.syncEngineTiming();
      return;
    }

    const enclosingIdx = sections.findIndex((s) => {
      const sStart = this.sectionStartSec(m, s);
      const sEnd = this.sectionEndSec(m, s);
      return markSec > sStart + 0.08 && markSec < sEnd - 0.08;
    });
    if (enclosingIdx >= 0) {
      const target = sections[enclosingIdx];
      const tStart = this.sectionStartSec(m, target);
      const tEnd = this.sectionEndSec(m, target);
      const displayName = this.resolveSectionDisplayName(sections, cleaned, null);
      const firstHalf: SectionDef = {
        ...target,
        start_sec: tStart,
        end_sec: markSec,
        ...this.approxBarsForTimes(m, tStart, markSec),
      };
      const secondHalf: SectionDef = {
        id: this.buildUniqueSectionId(displayName, enclosingIdx + 1),
        name: displayName,
        start_sec: markSec,
        end_sec: tEnd,
        ...this.approxBarsForTimes(m, markSec, tEnd),
        color,
      };
      sections.splice(enclosingIdx, 1, firstHalf, secondHalf);
      this.manifest.set({ ...m, sections });
      this.selectedSectionIdx.set(enclosingIdx + 1);
      this.workbenchNotice.set(
        `Marked "${displayName}" at ${this.formatTimePrecise(markSec)} (live synced to Stage)`,
      );
      void this.syncEngineTiming();
      return;
    }

    const first = sections[0];
    if (first && markSec < this.sectionStartSec(m, first) - 0.08) {
      const firstStart = this.sectionStartSec(m, first);
      const displayName = this.resolveSectionDisplayName(sections, cleaned, null);
      sections.unshift({
        id: this.buildUniqueSectionId(displayName, 0),
        name: displayName,
        start_sec: markSec,
        end_sec: firstStart,
        ...this.approxBarsForTimes(m, markSec, firstStart),
        color,
      });
      this.manifest.set({ ...m, sections });
      this.selectedSectionIdx.set(0);
      this.workbenchNotice.set(
        `Marked "${displayName}" at ${this.formatTimePrecise(markSec)} (live synced to Stage)`,
      );
      void this.syncEngineTiming();
      return;
    }

    const displayName = this.resolveSectionDisplayName(sections, cleaned, null);
    let startSec = markSec;
    if (sections.length > 0) {
      const lastIdx = sections.length - 1;
      const last = sections[lastIdx];
      const lastStart = this.sectionStartSec(m, last);
      const lastEnd = this.sectionEndSec(m, last);
      if (markSec > lastStart + 0.1) {
        sections[lastIdx] = {
          ...last,
          start_sec: lastStart,
          end_sec: markSec,
          ...this.approxBarsForTimes(m, lastStart, markSec),
        };
        startSec = markSec;
      } else {
        startSec = Math.max(markSec, lastEnd);
      }
    }
    const endSec = this.roundSec(Math.min(Math.max(dur, startSec + 1), startSec + defaultSpanSec));
    sections.push({
      id: this.buildUniqueSectionId(displayName, sections.length),
      name: displayName,
      start_sec: startSec,
      end_sec: endSec,
      ...this.approxBarsForTimes(m, startSec, endSec),
      color,
    });
    this.manifest.set({ ...m, sections });
    this.selectedSectionIdx.set(sections.length - 1);
    this.workbenchNotice.set(
      `Added "${displayName}" at ${this.formatTimePrecise(startSec)} (live synced to Stage)`,
    );
    void this.syncEngineTiming();
  }

  markCustomSectionAtPlayhead(): void {
    const custom = this.rapidSectionName().trim();
    if (!custom) return;
    this.markSectionAtPlayhead(custom, 'sky', 8);
    this.rapidSectionName.set('');
  }

  splitAndFocusCustomSection(): void {
    this.splitSectionAtCurrentBar();
    const input = this.rapidSectionNameInputRef?.nativeElement;
    if (input) {
      input.focus();
      input.select();
    }
  }

  setSectionStartToPlayhead(index: number): void {
    const m = this.manifest();
    const sec = m?.sections[index];
    if (!m || !sec) return;
    const markSec = this.roundSec(this.snap().current_position_sec);
    const curStart = this.sectionStartSec(m, sec);
    const curEnd = this.sectionEndSec(m, sec);
    if (markSec >= curEnd - 0.1) {
      this.workbenchNotice.set(
        `Playhead ${this.formatTimePrecise(markSec)} must be before "${sec.name}" end (${this.formatTimePrecise(curEnd)}).`,
      );
      return;
    }
    const prev = index > 0 ? m.sections[index - 1] : null;
    const prevStart = prev ? this.sectionStartSec(m, prev) : 0;
    const prevEnd = prev ? this.sectionEndSec(m, prev) : 0;
    const wasContiguous = prev !== null && Math.abs(prevEnd - curStart) < 0.03;
    const nextSections = m.sections.map((s, i) => {
      if (i === index) {
        return {
          ...s,
          start_sec: markSec,
          end_sec: curEnd,
          ...this.approxBarsForTimes(m, markSec, curEnd),
        };
      }
      if (i === index - 1 && prev && wasContiguous && markSec > prevStart + 0.1) {
        return {
          ...s,
          start_sec: prevStart,
          end_sec: markSec,
          ...this.approxBarsForTimes(m, prevStart, markSec),
        };
      }
      return s;
    });
    this.manifest.set({ ...m, sections: nextSections });
    this.selectedSectionIdx.set(index);
    void this.syncEngineTiming();
  }

  setSectionEndToPlayhead(index: number): void {
    const m = this.manifest();
    const sec = m?.sections[index];
    if (!m || !sec) return;
    const markSec = this.roundSec(this.snap().current_position_sec);
    const curStart = this.sectionStartSec(m, sec);
    const curEnd = this.sectionEndSec(m, sec);
    if (markSec <= curStart + 0.1) {
      this.workbenchNotice.set(
        `Playhead ${this.formatTimePrecise(markSec)} must be after "${sec.name}" start (${this.formatTimePrecise(curStart)}).`,
      );
      return;
    }
    const next = index + 1 < m.sections.length ? m.sections[index + 1] : null;
    const nextStart = next ? this.sectionStartSec(m, next) : 0;
    const nextEnd = next ? this.sectionEndSec(m, next) : 0;
    const wasContiguous = next !== null && Math.abs(nextStart - curEnd) < 0.03;
    const nextSections = m.sections.map((s, i) => {
      if (i === index) {
        return {
          ...s,
          start_sec: curStart,
          end_sec: markSec,
          ...this.approxBarsForTimes(m, curStart, markSec),
        };
      }
      if (i === index + 1 && next && wasContiguous && markSec < nextEnd - 0.1) {
        return {
          ...s,
          start_sec: markSec,
          end_sec: nextEnd,
          ...this.approxBarsForTimes(m, markSec, nextEnd),
        };
      }
      return s;
    });
    this.manifest.set({ ...m, sections: nextSections });
    this.selectedSectionIdx.set(index);
    void this.syncEngineTiming();
  }

  appendPresetSection(preset: { name: string; bars: number; color: string }): void {
    this.manifest.update((cur) => {
      if (!cur) return cur;
      const last = cur.sections[cur.sections.length - 1];
      const startSec = last ? this.sectionEndSec(cur, last) : this.roundSec(cur.first_downbeat_sec);
      const secPerBar = (60 / Math.max(30, cur.bpm)) * Math.max(1, cur.time_signature?.[0] || 4);
      const endSec = this.roundSec(startSec + preset.bars * secPerBar);
      const countSame = cur.sections.filter((s) => s.name.startsWith(preset.name)).length;
      const displayName =
        countSame > 0 && (preset.name === 'Verse' || preset.name === 'Chorus')
          ? `${preset.name} ${countSame + 1}`
          : preset.name;
      const id = `${displayName.toLowerCase().replace(/[^a-z0-9]+/g, '-')}-${cur.sections.length + 1}`;
      return {
        ...cur,
        sections: [
          ...cur.sections,
          {
            id,
            name: displayName,
            start_sec: startSec,
            end_sec: endSec,
            ...this.approxBarsForTimes(cur, startSec, endSec),
            color: preset.color,
          },
        ],
      };
    });
    void this.syncEngineTiming();
  }

  splitSectionAtCurrentBar(): void {
    const m = this.manifest();
    if (!m) return;
    const splitSec = this.roundSec(this.snap().current_position_sec);

    const idx = m.sections.findIndex((s) => {
      const sStart = this.sectionStartSec(m, s);
      const sEnd = this.sectionEndSec(m, s);
      return splitSec > sStart + 0.1 && splitSec < sEnd - 0.1;
    });
    if (idx < 0) {
      this.workbenchNotice.set(
        `Move playhead inside an existing section to split (${this.formatTimePrecise(splitSec)}).`,
      );
      return;
    }

    const target = m.sections[idx];
    const tStart = this.sectionStartSec(m, target);
    const tEnd = this.sectionEndSec(m, target);
    const firstHalf: SectionDef = {
      ...target,
      start_sec: tStart,
      end_sec: splitSec,
      ...this.approxBarsForTimes(m, tStart, splitSec),
    };
    const secondHalf: SectionDef = {
      id: this.buildUniqueSectionId(`${target.name} B`, m.sections.length),
      name: `${target.name} B`,
      start_sec: splitSec,
      end_sec: tEnd,
      ...this.approxBarsForTimes(m, splitSec, tEnd),
      color: target.color ?? 'emerald',
    };
    const nextSections = [...m.sections];
    nextSections.splice(idx, 1, firstHalf, secondHalf);
    this.manifest.set({ ...m, sections: nextSections });
    this.selectedSectionIdx.set(idx + 1);
    this.workbenchNotice.set(
      `Split "${target.name}" at ${this.formatTimePrecise(splitSec)} (synced to Stage)`,
    );
    void this.syncEngineTiming();
  }

  startInlineBandRename(index: number): void {
    const sec = this.manifest()?.sections[index];
    if (!sec) return;
    this.selectedSectionIdx.set(index);
    this.editingBandIdx.set(index);
    this.editingBandName.set(sec.name);
  }

  commitInlineBandRename(): void {
    const idx = this.editingBandIdx();
    if (idx === null) return;
    const nextName = this.editingBandName().trim();
    this.editingBandIdx.set(null);
    if (nextName) {
      this.updateSection(idx, { name: nextName });
      this.workbenchNotice.set(`Renamed section to "${nextName}" (synced to Stage)`);
    }
  }

  cancelInlineBandRename(): void {
    this.editingBandIdx.set(null);
  }

  extendSectionSec(index: number, deltaSec: number): void {
    const m = this.manifest();
    const sec = m?.sections[index];
    if (!m || !sec) return;
    const dur = Math.max(1, this.effectiveDurationSec());
    const startSec = this.sectionStartSec(m, sec);
    const endSec = this.sectionEndSec(m, sec);
    const next = index + 1 < m.sections.length ? m.sections[index + 1] : null;
    const nextStart = next ? this.sectionStartSec(m, next) : dur;
    const nextEnd = next ? this.sectionEndSec(m, next) : dur;
    const sharesWithNext = next !== null && Math.abs(nextStart - endSec) < 0.03;
    const maxEnd = next
      ? (sharesWithNext ? nextEnd - 0.1 : nextStart - 0.1)
      : dur;
    const targetEnd = this.roundSec(Math.max(startSec + 0.1, endSec + deltaSec));
    const clampedEnd = this.roundSec(Math.max(startSec + 0.1, Math.min(maxEnd, targetEnd)));
    if (Math.abs(clampedEnd - endSec) < 0.005) return;
    const nextSections = m.sections.map((s, i) => {
      if (i === index) {
        return {
          ...s,
          start_sec: startSec,
          end_sec: clampedEnd,
          ...this.approxBarsForTimes(m, startSec, clampedEnd),
        };
      }
      if (next && i === index + 1 && sharesWithNext) {
        return {
          ...s,
          start_sec: clampedEnd,
          end_sec: nextEnd,
          ...this.approxBarsForTimes(m, clampedEnd, nextEnd),
        };
      }
      return s;
    });
    this.manifest.set({ ...m, sections: nextSections });
    this.selectedSectionIdx.set(index);
    void this.syncEngineTiming();
  }

  extendSectionBars(index: number, deltaBars: number): void {
    const m = this.manifest();
    if (!m) return;
    const secPerBar = (60 / Math.max(30, m.bpm)) * Math.max(1, m.time_signature?.[0] || 4);
    this.extendSectionSec(index, deltaBars * secPerBar);
  }

  updateSection(index: number, patch: Partial<SectionDef>): void {
    this.manifest.update((cur) => {
      if (!cur) return cur;
      const dur = Math.max(1, this.effectiveDurationSec());
      return {
        ...cur,
        sections: cur.sections.map((s, i) => {
          if (i !== index) return s;
          if (patch.start_sec !== undefined || patch.end_sec !== undefined) {
            const prevStart = this.sectionStartSec(cur, s);
            const prevEnd = this.sectionEndSec(cur, s);
            let nextStart =
              typeof patch.start_sec === 'number' && Number.isFinite(patch.start_sec)
                ? this.roundSec(Math.max(0, Math.min(dur - 0.1, patch.start_sec)))
                : prevStart;
            let nextEnd =
              typeof patch.end_sec === 'number' && Number.isFinite(patch.end_sec)
                ? this.roundSec(Math.max(nextStart + 0.1, Math.min(dur, patch.end_sec)))
                : prevEnd;
            if (nextEnd <= nextStart + 0.1) {
              if (patch.start_sec !== undefined) {
                nextStart = this.roundSec(Math.max(0, nextEnd - 0.1));
              } else {
                nextEnd = this.roundSec(nextStart + 0.1);
              }
            }
            return {
              ...s,
              ...patch,
              start_sec: nextStart,
              end_sec: nextEnd,
              ...this.approxBarsForTimes(cur, nextStart, nextEnd),
            };
          }
          if (patch.start_bar !== undefined || patch.end_bar !== undefined) {
            const nextStartBar = Math.max(1, Math.round(patch.start_bar ?? s.start_bar));
            const nextEndBar = Math.max(nextStartBar + 1, Math.round(patch.end_bar ?? s.end_bar));
            return {
              ...s,
              ...patch,
              start_bar: nextStartBar,
              end_bar: nextEndBar,
              start_sec: this.roundSec(this.barToSec(cur, nextStartBar)),
              end_sec: this.roundSec(this.barToSec(cur, nextEndBar)),
            };
          }
          return { ...s, ...patch };
        }),
      };
    });
    void this.syncEngineTiming();
  }

  removeSection(index: number): void {
    const m = this.manifest();
    if (!m || m.sections.length <= 1 || index < 0 || index >= m.sections.length) return;
    const removed = m.sections[index];
    const remStart = this.sectionStartSec(m, removed);
    const remEnd = this.sectionEndSec(m, removed);
    const nextSections = m.sections.filter((_, i) => i !== index).map((s) => ({ ...s }));
    // Keep contiguous sections gap-free by absorbing removed span into the neighbor
    if (index > 0) {
      const prev = nextSections[index - 1];
      const prevStart = this.sectionStartSec(m, prev);
      const prevEnd = this.sectionEndSec(m, prev);
      if (Math.abs(prevEnd - remStart) < 0.05 || prev.end_bar === removed.start_bar) {
        prev.start_sec = prevStart;
        prev.end_sec = remEnd;
        const bars = this.approxBarsForTimes(m, prevStart, remEnd);
        prev.start_bar = bars.start_bar;
        prev.end_bar = Math.max(bars.end_bar, removed.end_bar);
      }
    } else if (index === 0 && nextSections.length > 0) {
      const first = nextSections[0];
      const firstStart = this.sectionStartSec(m, first);
      const firstEnd = this.sectionEndSec(m, first);
      if (Math.abs(remEnd - firstStart) < 0.05 || removed.end_bar === first.start_bar) {
        first.start_sec = remStart;
        first.end_sec = firstEnd;
        const bars = this.approxBarsForTimes(m, remStart, firstEnd);
        first.start_bar = Math.min(bars.start_bar, removed.start_bar);
        first.end_bar = bars.end_bar;
      }
    }
    this.manifest.set({ ...m, sections: nextSections });
    if (this.editingBandIdx() === index) {
      this.editingBandIdx.set(null);
    }
    const curSel = this.selectedSectionIdx();
    if (curSel !== null && curSel >= index) {
      this.selectedSectionIdx.set(Math.max(0, Math.min(nextSections.length - 1, curSel - 1)));
    }
    this.workbenchNotice.set(`Deleted section "${removed.name}" (synced to Stage)`);
    void this.syncEngineTiming();
  }

  isVocalStemKey(key: string): boolean {
    const lower = key.toLowerCase();
    return (
      lower.includes('vocal') ||
      lower === 'vox' ||
      lower === 'lead' ||
      lower === 'backing' ||
      lower === 'choir'
    );
  }

  isStemMuted(key: string): boolean {
    const live = this.snap().stems.find((s) => s.name === key);
    if (live) return live.muted;
    return this.editableStems().find((s) => s.key === key)?.muted ?? false;
  }

  isStemSoloed(key: string): boolean {
    return this.soloedStems().includes(key);
  }

  toggleStemActive(key: string, active: boolean): void {
    const muted = !active;
    // Manual checkbox change exits any solo isolation with an explicit mute mask
    this.soloedStems.set([]);
    this.preSoloMuted = null;
    this.updateStemRole(key, { muted });
    void this.stage.sendCommand({ type: 'SetStemMuted', payload: { name: key, muted } });
  }

  toggleStemSolo(key: string): void {
    const stems = this.editableStems();
    if (stems.length === 0) return;

    const currentSolo = this.soloedStems();
    if (currentSolo.length === 0 && !this.preSoloMuted) {
      const snapshot: Record<string, boolean> = {};
      for (const st of stems) {
        snapshot[st.key] = this.isStemMuted(st.key);
      }
      this.preSoloMuted = snapshot;
    }

    const nextSolo = currentSolo.includes(key)
      ? currentSolo.filter((k) => k !== key)
      : [...currentSolo, key];
    this.soloedStems.set(nextSolo);

    if (nextSolo.length === 0) {
      const restore = this.preSoloMuted ?? {};
      this.preSoloMuted = null;
      this.applyStemMuteMap(
        Object.fromEntries(stems.map((st) => [st.key, restore[st.key] ?? false])),
      );
      return;
    }

    this.applyStemMuteMap(
      Object.fromEntries(stems.map((st) => [st.key, !nextSolo.includes(st.key)])),
    );
  }

  clearSolo(): void {
    if (this.soloedStems().length === 0) return;
    const stems = this.editableStems();
    const restore = this.preSoloMuted ?? {};
    this.soloedStems.set([]);
    this.preSoloMuted = null;
    this.applyStemMuteMap(
      Object.fromEntries(stems.map((st) => [st.key, restore[st.key] ?? false])),
    );
  }

  applyInstrumentalOnly(): void {
    const stems = this.editableStems();
    if (stems.length === 0) return;
    this.soloedStems.set([]);
    this.preSoloMuted = null;
    this.applyStemMuteMap(
      Object.fromEntries(stems.map((st) => [st.key, this.isVocalStemKey(st.key)])),
    );
    this.workbenchNotice.set('Instrumental mix active (vocals muted).');
  }

  unmuteAllStems(): void {
    const stems = this.editableStems();
    if (stems.length === 0) return;
    this.soloedStems.set([]);
    this.preSoloMuted = null;
    this.applyStemMuteMap(Object.fromEntries(stems.map((st) => [st.key, false])));
  }

  setStemGain(key: string, gain: number): void {
    const clamped = Math.max(0, Math.min(1.5, Number(gain) || 0));
    this.updateStemRole(key, { default_gain: clamped });
    void this.stage.sendCommand({
      type: 'SetStemGain',
      payload: { name: key, gain: clamped },
    });
  }

  private applyStemMuteMap(muteMap: Record<string, boolean>): void {
    for (const [key, muted] of Object.entries(muteMap)) {
      this.updateStemRole(key, { muted });
      void this.stage.sendCommand({ type: 'SetStemMuted', payload: { name: key, muted } });
    }
  }

  updateStemRole(key: string, patch: Partial<StemDefDetailed>): void {
    this.manifest.update((cur) => {
      if (!cur) return cur;
      const existing = cur.stems[key];
      const base: StemDefDetailed =
        typeof existing === 'string'
          ? {
            file: existing,
            pitched: !['drums', 'percussion', 'cues', 'click'].includes(key.toLowerCase()),
            is_cues: ['cues', 'cue', 'guide'].includes(key.toLowerCase()),
            default_gain: 1.0,
            muted: false,
          }
          : existing;
      return {
        ...cur,
        stems: {
          ...cur.stems,
          [key]: { ...base, ...patch },
        },
      };
    });
  }

  validateManifest(m: SongManifest): string | null {
    if (!m.title.trim()) return 'Song title is required.';
    if (m.bpm < 30 || m.bpm > 260) return 'BPM must be between 30 and 260.';
    if (m.sections.length === 0) return 'At least one arrangement section is required.';
    for (const sec of m.sections) {
      if (!sec.name.trim()) return 'Section names cannot be empty.';
      if (sec.start_sec != null && sec.end_sec != null) {
        if (!Number.isFinite(sec.start_sec) || !Number.isFinite(sec.end_sec) || sec.start_sec < 0 || sec.end_sec <= sec.start_sec + 0.01) {
          return `Section "${sec.name}" must have End time > Start time.`;
        }
      } else if (sec.start_bar < 1 || sec.end_bar <= sec.start_bar) {
        return `Section "${sec.name}" must have End Bar > Start Bar.`;
      }
    }
    if (Object.keys(m.stems).length === 0) return 'At least one stem is required.';
    return null;
  }

  hasGeneratedSectionCues(): boolean {
    const stems = this.manifest()?.stems;
    if (!stems) return false;
    return Object.entries(stems).some(([key, def]) => {
      const file = typeof def === 'string' ? def : def.file;
      const isCue = typeof def === 'string' ? key.toLowerCase() === 'cues' : (def.is_cues ?? key.toLowerCase() === 'cues');
      return isCue || file === 'section-cues.wav';
    });
  }

  async generateIemSectionCues(): Promise<void> {
    const m = this.manifest();
    const dir = this.loadedDir();
    if (!m || !dir) return;
    if (m.sections.length === 0) {
      this.workbenchError.set('Add at least one section marker before generating IEM cues.');
      return;
    }

    this.workbenchError.set(null);
    this.generatingCues.set(true);
    try {
      const res = await this.stage.generateSectionCues(
        dir,
        {
          bpm: m.bpm,
          first_downbeat_sec: m.first_downbeat_sec,
          beats_per_bar: m.time_signature?.[0] || 4,
          sections: m.sections,
        },
        {
          lead_in_beats: this.cueLeadInBeats(),
          include_chime: this.cueIncludeChime(),
        },
        true,
        m,
      );
      if (!res) {
        this.workbenchError.set(this.stage.statusMessage() || 'Failed to generate IEM section cues.');
        return;
      }
      this.manifest.set(res.manifest);
      this.generatedCuePlacements.set(res.placements);
      this.autoRegenerateCuesOnSave.set(true);
      this.workbenchNotice.set(
        `Generated IEM cues for ${res.placements.length} section${res.placements.length === 1 ? '' : 's'} (${res.stem_key}) — routed strictly to IEM!`,
      );
    } finally {
      this.generatingCues.set(false);
    }
  }

  async saveToLibrary(alsoAddToSetlist: boolean): Promise<void> {
    const m = this.manifest();
    const dir = this.loadedDir();
    if (!m || !dir) return;

    const err = this.validateManifest(m);
    if (err) {
      this.workbenchError.set(err);
      return;
    }

    let workingManifest: SongManifest = m.provenance
      ? {
          ...m,
          provenance: {
            ...m.provenance,
            needs_review: false,
            review_notes: [],
          },
        }
      : m;

    if (this.autoRegenerateCuesOnSave() && workingManifest.sections.length > 0) {
      const cueRes = await this.stage.generateSectionCues(
        dir,
        {
          bpm: workingManifest.bpm,
          first_downbeat_sec: workingManifest.first_downbeat_sec,
          beats_per_bar: workingManifest.time_signature?.[0] || 4,
          sections: workingManifest.sections,
        },
        {
          lead_in_beats: this.cueLeadInBeats(),
          include_chime: this.cueIncludeChime(),
        },
        false,
        workingManifest,
      );
      if (cueRes) {
        workingManifest = {
          ...cueRes.manifest,
          provenance: workingManifest.provenance,
        };
        this.generatedCuePlacements.set(cueRes.placements);
      }
    }

    this.manifest.set(workingManifest);

    const ok = await this.stage.saveSongManifest(dir, workingManifest, true);
    if (!ok) return;

    await this.stage.refreshLibrary();
    this.workbenchNotice.set(
      alsoAddToSetlist
        ? `Saved "${m.title}" to Library and queued into Setlist!`
        : `Saved "${m.title}" to Library!`,
    );

    if (alsoAddToSetlist) {
      await this.stage.sendCommand({
        type: 'AddSetlistSong',
        payload: { dir, insert_at: null },
      });
    }
    this.savedToLibrary.emit(dir);
  }

  async saveToolchainPaths(): Promise<void> {
    const cur = this.stage.workbenchConfig();
    const next: WorkbenchConfig = {
      ...cur,
      ffmpeg_path: this.pathFfmpeg().trim() || null,
      yt_dlp_path: this.pathYtDlp().trim() || null,
      audio_separator_path: this.pathAudioSeparator().trim() || null,
      demucs_path: this.pathDemucs().trim() || null,
      default_separation_engine: this.separationEngine(),
      default_stem_preset: this.stemPreset(),
      custom_uvr_model: this.customUvrModel().trim() || 'htdemucs_ft.yaml',
    };
    await this.stage.saveWorkbenchConfig(next);
  }

  copyCommand(cmd: string): void {
    if (typeof navigator !== 'undefined' && navigator.clipboard) {
      void navigator.clipboard.writeText(cmd);
    }
    this.copiedHint.set(cmd);
    setTimeout(() => {
      if (this.copiedHint() === cmd) this.copiedHint.set(null);
    }, 2000);
  }

  formatTime(sec: number): string {
    if (!Number.isFinite(sec) || sec < 0) return '0:00.0';
    const mins = Math.floor(sec / 60);
    const rem = sec % 60;
    return `${mins}:${rem.toFixed(1).padStart(4, '0')}`;
  }

  formatTimePrecise(sec: number): string {
    if (!Number.isFinite(sec) || sec < 0) return '0:00.00';
    const mins = Math.floor(sec / 60);
    const rem = sec % 60;
    return `${mins}:${rem.toFixed(2).padStart(5, '0')}`;
  }
}

