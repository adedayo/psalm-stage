import { DecimalPipe, UpperCasePipe } from '@angular/common';
import {
  ChangeDetectionStrategy,
  Component,
  HostListener,
  computed,
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
  type MidiTarget,
  type MidiTriggerBehavior,
  type OutputRoutingMode,
  type QuantizeMode,
} from './models/engine.models';
import { PsalmStageService } from './services/psalm-stage.service';

@Component({
  selector: 'app-root',
  standalone: true,
  imports: [
    DecimalPipe,
    UpperCasePipe,
    FormsModule,
    HlmButtonImports,
    HlmBadgeImports,
    HlmCardImports,
    HlmSliderImports,
    HlmSwitchImports,
  ],
  templateUrl: './app.html',
  styleUrl: './app.css',
  changeDetection: ChangeDetectionStrategy.OnPush,
})
export class App {
  protected readonly stage = inject(PsalmStageService);
  protected readonly snap = this.stage.snapshot;
  protected readonly liveLock = signal(false);
  protected readonly customFolderPath = signal('');
  protected readonly selectedMidiPortIndex = signal<number>(0);

  protected readonly beatDots = computed(() => {
    const count = Math.max(1, this.snap().beats_per_bar);
    return Array.from({ length: count }, (_, idx) => idx + 1);
  });

  @HostListener('window:keydown', ['$event'])
  protected onGlobalKeydown(ev: KeyboardEvent): void {
    const tag = (ev.target as HTMLElement | null)?.tagName;
    if (tag === 'INPUT' || tag === 'SELECT' || tag === 'TEXTAREA') {
      return;
    }
    if (ev.code === 'Space') {
      ev.preventDefault();
      void this.stage.sendCommand({ type: 'PlayStop' });
    } else if (ev.key === 'l' || ev.key === 'L') {
      ev.preventDefault();
      void this.stage.sendCommand({ type: 'ToggleLoop' });
    } else if (/^[1-9]$/.test(ev.key)) {
      const slot = Number(ev.key) - 1;
      if (slot < this.snap().sections.length) {
        ev.preventDefault();
        void this.stage.sendCommand({ type: 'QueueSectionBySlot', payload: slot });
      }
    }
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
    void this.stage.sendCommand({ type: 'AdjustBpm', payload: delta });
  }

  protected onQuantizeChange(mode: QuantizeMode): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'SetQuantizeMode', payload: mode });
  }

  protected onRoutingChange(mode: OutputRoutingMode): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'SetRoutingMode', payload: mode });
  }

  protected onDeviceSelect(event: Event): void {
    if (this.liveLock()) return;
    const name = (event.target as HTMLSelectElement).value;
    if (name) {
      void this.stage.sendCommand({ type: 'SelectOutputDevice', payload: name });
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
    void this.stage.sendCommand({ type: 'SetClickMuted', payload: muted });
  }

  protected onStemSlider(name: string, values: number[]): void {
    const val = values[0];
    if (typeof val === 'number') {
      void this.stage.sendCommand({ type: 'SetStemGain', payload: { name, gain: val } });
    }
  }

  protected onStemMute(name: string, muted: boolean): void {
    void this.stage.sendCommand({ type: 'SetStemMuted', payload: { name, muted } });
  }

  protected mapMidiFader(target: MidiTarget, label: string, behavior: MidiTriggerBehavior): void {
    if (!this.stage.midiAssignMode()) return;
    this.stage.startListeningForTarget(target, label, behavior);
  }

  protected loadFolder(): void {
    const dir = this.customFolderPath().trim();
    if (!dir || this.liveLock()) return;
    void this.stage.sendCommand({ type: 'LoadSongDir', payload: dir });
  }

  protected loadDemo(): void {
    if (this.liveLock()) return;
    void this.stage.sendCommand({ type: 'LoadDemoSong' });
  }

  protected connectSelectedMidi(): void {
    void this.stage.connectMidiPort(Number(this.selectedMidiPortIndex()));
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
      case 'MasterGain':
        return 'Master Volume';
      case 'ClickGain':
        return 'Click Volume';
      case 'CuesGain':
        return 'Cues Volume';
      case 'StemGain':
        return `Stem: ${target.value}`;
      default:
        return target.type;
    }
  }
}
