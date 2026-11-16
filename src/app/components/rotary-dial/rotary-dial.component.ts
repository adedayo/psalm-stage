import {
  Component,
  computed,
  effect,
  input,
  output,
  signal,
} from '@angular/core';

@Component({
  selector: 'app-rotary-dial',
  standalone: true,
  styles: [
    `
      .dial-step-btn {
        min-width: 34px;
        min-height: 36px;
      }

      @media (pointer: coarse) {
        .dial-step-btn {
          min-width: 44px;
          min-height: 44px;
        }
      }
    `,
  ],
  template: `
    <div
      class="inline-flex flex-col items-center gap-1 rounded-xl border px-2 py-1.5 transition-colors shadow-[inset_0_1px_0_rgba(255,255,255,0.05),0_2px_8px_rgba(0,0,0,0.4)]"
      [class]="
        midiAssignMode()
          ? 'border-amber-500/60 bg-amber-950/20'
          : 'border-zinc-800/90 bg-gradient-to-b from-zinc-900/95 to-zinc-950'
      "
    >
      <div class="flex items-center gap-1.5">
        <button
          type="button"
          class="dial-step-btn inline-flex items-center justify-center rounded-lg border border-zinc-700/80 bg-zinc-900 text-base font-semibold text-zinc-200 hover:bg-zinc-800 hover:text-white active:scale-95 disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline-2 focus-visible:outline-amber-400"
          [disabled]="disabled() && !midiAssignMode()"
          [attr.aria-label]="decrementAriaLabel()"
          [attr.data-testid]="decrementTestId()"
          [title]="midiAssignMode() ? 'Click to assign MIDI button for -' + step() : decrementAriaLabel()"
          (click)="onDecrementClick()"
        >
          −
        </button>

        <!-- Circular Rotary Dial -->
        <div
          role="slider"
          [tabindex]="disabled() && !midiAssignMode() ? -1 : 0"
          [attr.aria-label]="dialAriaLabel()"
          [attr.aria-valuemin]="min()"
          [attr.aria-valuemax]="max()"
          [attr.aria-valuenow]="value()"
          [attr.aria-valuetext]="ariaValueText()"
          [attr.aria-disabled]="disabled() && !midiAssignMode() ? 'true' : null"
          [attr.data-testid]="dialTestId()"
          [title]="
            midiAssignMode()
              ? 'Click dial ring to assign a MIDI rotary knob'
              : 'Drag up/down or scroll to dial in steps of ' + step()
          "
          class="relative flex h-16 w-16 shrink-0 select-none items-center justify-center rounded-full touch-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-amber-400"
          [class]="
            midiAssignMode()
              ? 'cursor-pointer ring-1 ring-amber-400/50 hover:ring-amber-300'
              : disabled()
                ? 'cursor-not-allowed opacity-60'
                : 'cursor-ns-resize'
          "
          (pointerdown)="onDialPointerDown($event)"
          (pointermove)="onDialPointerMove($event)"
          (pointerup)="onDialPointerUp($event)"
          (pointercancel)="onDialPointerCancel()"
          (wheel)="onDialWheel($event)"
          (keydown)="onDialKeyDown($event)"
        >
          <!-- Outer SVG Calibration Ticks & Sweep Arc -->
          <svg
            aria-hidden="true"
            viewBox="0 0 80 80"
            class="pointer-events-none absolute inset-0 h-full w-full"
          >
            @for (tick of ticks(); track tick.index) {
              <line
                [attr.x1]="tick.x1"
                [attr.y1]="tick.y1"
                [attr.x2]="tick.x2"
                [attr.y2]="tick.y2"
                [attr.stroke]="tick.active ? accentStroke() : '#3f3f46'"
                [attr.stroke-width]="tick.major ? '1.6' : '1.1'"
                stroke-linecap="round"
              />
            }

            <!-- Background 270deg track -->
            <circle
              cx="40"
              cy="40"
              r="31"
              fill="none"
              stroke="#27272a"
              stroke-width="4.5"
              stroke-linecap="round"
              stroke-dasharray="146.084 194.779"
              transform="rotate(135 40 40)"
            />

            <!-- Active arc -->
            @if (progress() > 0.002) {
              <circle
                cx="40"
                cy="40"
                r="31"
                fill="none"
                [attr.stroke]="accentStroke()"
                stroke-width="4.5"
                stroke-linecap="round"
                [attr.stroke-dasharray]="activeDashArray()"
                transform="rotate(135 40 40)"
              />
            }

            <!-- Knob Indicator Pip -->
            <circle
              [attr.cx]="indicatorPos().x"
              [attr.cy]="indicatorPos().y"
              r="3.2"
              [attr.fill]="accentGlow()"
              stroke="#09090b"
              stroke-width="1.2"
            />
          </svg>

          <!-- Center Brushed Cap & Value Readout / Input -->
          <div
            class="relative z-10 flex h-11 w-11 flex-col items-center justify-center rounded-full border border-zinc-800/90 bg-zinc-950/95 shadow-[inset_0_1px_2px_rgba(0,0,0,0.9)]"
          >
            <span
              class="text-[8px] font-bold uppercase tracking-[0.14em] text-zinc-500 leading-none"
            >
              {{ label() }}
            </span>

            @if (editable() && !disabled() && !midiAssignMode()) {
              <input
                type="text"
                inputmode="decimal"
                autocomplete="off"
                autocorrect="off"
                spellcheck="false"
                enterkeyhint="done"
                [attr.aria-label]="inputAriaLabel()"
                [attr.data-testid]="inputTestId()"
                class="mt-0.5 w-10 rounded bg-transparent px-0.5 py-0 text-center font-mono text-[13px] font-bold tabular-nums text-zinc-100 focus:bg-zinc-900 focus:outline-none focus:ring-1 focus:ring-amber-400/70"
                [value]="displayedInputText()"
                (pointerdown)="$event.stopPropagation()"
                (focus)="onInputFocus($event)"
                (input)="onInput($event)"
                (change)="onInputCommit($event)"
                (blur)="onInputBlur($event)"
                (keydown)="onInputKeyDown($event)"
              />
            } @else {
              <div
                [attr.data-testid]="displayTestId()"
                class="mt-0.5 font-mono text-[12px] font-bold tabular-nums leading-tight"
                [class]="accentTextClass()"
              >
                {{ displayText() ?? formattedValue() }}
              </div>
              @if (subText()) {
                <div class="text-[8px] font-mono text-zinc-400 leading-none">
                  {{ subText() }}
                </div>
              }
            }
          </div>
        </div>

        <button
          type="button"
          class="dial-step-btn inline-flex items-center justify-center rounded-lg border border-zinc-700/80 bg-zinc-900 text-base font-semibold text-zinc-200 hover:bg-zinc-800 hover:text-white active:scale-95 disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline-2 focus-visible:outline-amber-400"
          [disabled]="disabled() && !midiAssignMode()"
          [attr.aria-label]="incrementAriaLabel()"
          [attr.data-testid]="incrementTestId()"
          [title]="midiAssignMode() ? 'Click to assign MIDI button for +' + step() : incrementAriaLabel()"
          (click)="onIncrementClick()"
        >
          +
        </button>
      </div>

      <!-- MIDI Assign Mapping / Unassign Chips -->
      @if (midiAssignMode()) {
        <div class="flex flex-wrap items-center justify-center gap-1 text-[9px] font-mono">
          <div class="inline-flex items-center rounded border border-amber-500/40 bg-zinc-950/90">
            <button
              type="button"
              class="px-1.5 py-0.5 text-amber-300 hover:text-amber-200"
              (click)="dialMidiClick.emit()"
            >
              {{ dialBindingLabel() ? 'Dial: ' + dialBindingLabel() : 'Tap Dial: Map Knob' }}
            </button>
            @if (dialBindingLabel()) {
              <button
                type="button"
                class="border-l border-amber-500/30 px-1 py-0.5 text-rose-300 hover:bg-rose-500/20 hover:text-rose-200"
                [attr.aria-label]="'Unassign MIDI from ' + label() + ' Dial'"
                title="Unassign MIDI knob"
                (click)="unassignDial.emit()"
              >
                ×
              </button>
            }
          </div>

          @if (downBindingLabel(); as downLabel) {
            <div class="inline-flex items-center rounded border border-zinc-700 bg-zinc-950/90">
              <span class="px-1 py-0.5 text-zinc-300">-: {{ downLabel }}</span>
              <button
                type="button"
                class="border-l border-zinc-700 px-1 py-0.5 text-rose-300 hover:bg-rose-500/20 hover:text-rose-200"
                [attr.aria-label]="'Unassign MIDI from ' + label() + ' Decrease'"
                title="Unassign MIDI -"
                (click)="unassignDown.emit()"
              >
                ×
              </button>
            </div>
          }

          @if (upBindingLabel(); as upLabel) {
            <div class="inline-flex items-center rounded border border-zinc-700 bg-zinc-950/90">
              <span class="px-1 py-0.5 text-zinc-300">+: {{ upLabel }}</span>
              <button
                type="button"
                class="border-l border-zinc-700 px-1 py-0.5 text-rose-300 hover:bg-rose-500/20 hover:text-rose-200"
                [attr.aria-label]="'Unassign MIDI from ' + label() + ' Increase'"
                title="Unassign MIDI +"
                (click)="unassignUp.emit()"
              >
                ×
              </button>
            </div>
          }
        </div>
      } @else if (dialBindingLabel()) {
        <div
          class="text-[9px] font-mono text-amber-300/80 leading-none"
          [title]="'MIDI Rotary mapped: ' + dialBindingLabel()"
        >
          MIDI {{ dialBindingLabel() }}
        </div>
      }
    </div>
  `,
})
export class RotaryDialComponent {
  readonly label = input.required<string>();
  readonly value = input.required<number>();
  readonly min = input<number>(30);
  readonly max = input<number>(260);
  readonly step = input<number>(1);
  readonly displayText = input<string | null>(null);
  readonly subText = input<string | null>(null);
  readonly editable = input<boolean>(false);
  readonly disabled = input<boolean>(false);
  readonly accent = input<'amber' | 'violet' | 'emerald'>('amber');

  readonly inputAriaLabel = input<string>('Tempo BPM');
  readonly dialAriaLabel = input<string>('Tempo Rotary Dial');
  readonly decrementAriaLabel = input<string>('Decrease by 1');
  readonly incrementAriaLabel = input<string>('Increase by 1');
  readonly inputTestId = input<string | null>(null);
  readonly displayTestId = input<string | null>(null);
  readonly dialTestId = input<string | null>(null);
  readonly decrementTestId = input<string | null>(null);
  readonly incrementTestId = input<string | null>(null);

  readonly midiAssignMode = input<boolean>(false);
  readonly dialBindingLabel = input<string | null>(null);
  readonly downBindingLabel = input<string | null>(null);
  readonly upBindingLabel = input<string | null>(null);

  readonly stepChange = output<number>();
  readonly valueCommit = output<number>();
  readonly dialMidiClick = output<void>();
  readonly downMidiClick = output<void>();
  readonly upMidiClick = output<void>();
  readonly unassignDial = output<void>();
  readonly unassignDown = output<void>();
  readonly unassignUp = output<void>();

  private readonly isEditing = signal<boolean>(false);
  private readonly draftText = signal<string>('');
  private readonly pendingCommitted = signal<string | null>(null);

  constructor() {
    effect(() => {
      // Clear pending display text once external value updates
      this.value();
      this.pendingCommitted.set(null);
    });
  }

  private dragging = false;
  private dragMoved = false;
  private lastDragY = 0;
  private dragAccumPx = 0;
  private wheelAccum = 0;

  private readonly arcCircumference = 2 * Math.PI * 31;
  private readonly sweepLength = this.arcCircumference * 0.75; // 270 deg

  protected readonly formattedValue = computed(() => {
    const v = this.value();
    if (!Number.isFinite(v)) return '—';
    const rounded = Math.round(v * 10) / 10;
    return Number.isInteger(rounded) ? String(rounded) : rounded.toFixed(1);
  });

  protected readonly displayedInputText = computed(() => {
    if (this.isEditing()) return this.draftText();
    return this.pendingCommitted() ?? this.formattedValue();
  });

  protected readonly ariaValueText = computed(() => {
    const dt = this.displayText();
    return dt ? `${dt} ${this.label()}` : `${this.formattedValue()} ${this.label()}`;
  });

  protected readonly progress = computed(() => {
    const span = this.max() - this.min();
    if (span <= 0) return 0;
    return Math.max(0, Math.min(1, (this.value() - this.min()) / span));
  });

  protected readonly activeDashArray = computed(() => {
    const active = this.progress() * this.sweepLength;
    return `${active.toFixed(3)} ${this.arcCircumference.toFixed(3)}`;
  });

  protected readonly indicatorPos = computed(() => {
    const angleDeg = 135 + this.progress() * 270;
    const rad = (angleDeg * Math.PI) / 180;
    return {
      x: (40 + 31 * Math.cos(rad)).toFixed(2),
      y: (40 + 31 * Math.sin(rad)).toFixed(2),
    };
  });

  protected readonly ticks = computed(() => {
    const count = 15;
    const p = this.progress();
    const list: Array<{
      index: number;
      x1: string;
      y1: string;
      x2: string;
      y2: string;
      active: boolean;
      major: boolean;
    }> = [];
    for (let i = 0; i < count; i++) {
      const frac = i / (count - 1);
      const rad = ((135 + frac * 270) * Math.PI) / 180;
      const major = i === 0 || i === count - 1 || i === Math.floor(count / 2);
      const rInner = major ? 35.2 : 36.2;
      const rOuter = 38.6;
      list.push({
        index: i,
        x1: (40 + rInner * Math.cos(rad)).toFixed(2),
        y1: (40 + rInner * Math.sin(rad)).toFixed(2),
        x2: (40 + rOuter * Math.cos(rad)).toFixed(2),
        y2: (40 + rOuter * Math.sin(rad)).toFixed(2),
        active: frac <= p + 0.015,
        major,
      });
    }
    return list;
  });

  protected accentStroke(): string {
    switch (this.accent()) {
      case 'violet':
        return '#a78bfa';
      case 'emerald':
        return '#34d399';
      default:
        return '#f59e0b';
    }
  }

  protected accentGlow(): string {
    switch (this.accent()) {
      case 'violet':
        return '#c4b5fd';
      case 'emerald':
        return '#6ee7b7';
      default:
        return '#fcd34d';
    }
  }

  protected accentTextClass(): string {
    switch (this.accent()) {
      case 'violet':
        return 'text-violet-200';
      case 'emerald':
        return 'text-emerald-300';
      default:
        return 'text-zinc-100';
    }
  }

  protected onDecrementClick(): void {
    if (this.midiAssignMode()) {
      this.downMidiClick.emit();
      return;
    }
    if (this.disabled()) return;
    this.stepChange.emit(-this.step());
  }

  protected onIncrementClick(): void {
    if (this.midiAssignMode()) {
      this.upMidiClick.emit();
      return;
    }
    if (this.disabled()) return;
    this.stepChange.emit(this.step());
  }

  protected onDialPointerDown(event: PointerEvent): void {
    if (event.button !== 0 && event.pointerType === 'mouse') return;
    this.dragging = true;
    this.dragMoved = false;
    this.lastDragY = event.clientY;
    this.dragAccumPx = 0;
    const el = event.currentTarget as HTMLElement | null;
    if (el && typeof el.setPointerCapture === 'function') {
      try {
        el.setPointerCapture(event.pointerId);
      } catch {
        // Ignore capture failures in synthetic test environments.
      }
    }
  }

  protected onDialPointerMove(event: PointerEvent): void {
    if (!this.dragging || this.disabled() || this.midiAssignMode()) return;
    const dy = this.lastDragY - event.clientY;
    this.lastDragY = event.clientY;
    this.dragAccumPx += dy;
    if (Math.abs(this.dragAccumPx) >= 3) {
      this.dragMoved = true;
    }
    const pxPerStep = 6;
    while (this.dragAccumPx >= pxPerStep) {
      this.dragAccumPx -= pxPerStep;
      this.stepChange.emit(this.step());
    }
    while (this.dragAccumPx <= -pxPerStep) {
      this.dragAccumPx += pxPerStep;
      this.stepChange.emit(-this.step());
    }
  }

  protected onDialPointerUp(event: PointerEvent): void {
    if (!this.dragging) return;
    this.dragging = false;
    const el = event.currentTarget as HTMLElement | null;
    if (el && typeof el.releasePointerCapture === 'function') {
      try {
        el.releasePointerCapture(event.pointerId);
      } catch {
        // Ignore if not captured.
      }
    }
    if (!this.dragMoved && this.midiAssignMode()) {
      this.dialMidiClick.emit();
    }
  }

  protected onDialPointerCancel(): void {
    this.dragging = false;
    this.dragAccumPx = 0;
  }

  protected onDialWheel(event: WheelEvent): void {
    if (this.disabled() || this.midiAssignMode()) return;
    event.preventDefault();
    const rawDelta = Math.abs(event.deltaY) >= Math.abs(event.deltaX) ? -event.deltaY : event.deltaX;
    if (rawDelta === 0) return;
    if (event.deltaMode !== 0) {
      this.wheelAccum = 0;
      this.stepChange.emit((rawDelta > 0 ? 1 : -1) * this.step());
      return;
    }
    this.wheelAccum += rawDelta;
    const threshold = 24;
    while (this.wheelAccum >= threshold) {
      this.wheelAccum -= threshold;
      this.stepChange.emit(this.step());
    }
    while (this.wheelAccum <= -threshold) {
      this.wheelAccum += threshold;
      this.stepChange.emit(-this.step());
    }
  }

  protected onDialKeyDown(event: KeyboardEvent): void {
    if (this.midiAssignMode()) {
      if (event.key === 'Enter' || event.key === ' ') {
        event.preventDefault();
        this.dialMidiClick.emit();
      }
      return;
    }
    if (this.disabled()) return;
    const s = this.step();
    switch (event.key) {
      case 'ArrowUp':
      case 'ArrowRight':
        event.preventDefault();
        this.stepChange.emit(s);
        break;
      case 'ArrowDown':
      case 'ArrowLeft':
        event.preventDefault();
        this.stepChange.emit(-s);
        break;
      case 'PageUp':
        event.preventDefault();
        this.stepChange.emit(s * 5);
        break;
      case 'PageDown':
        event.preventDefault();
        this.stepChange.emit(-s * 5);
        break;
      case 'Home':
        event.preventDefault();
        this.valueCommit.emit(this.min());
        break;
      case 'End':
        event.preventDefault();
        this.valueCommit.emit(this.max());
        break;
    }
  }

  protected onInputFocus(event: FocusEvent): void {
    this.isEditing.set(true);
    this.draftText.set(this.formattedValue());
    const inputEl = event.target as HTMLInputElement | null;
    inputEl?.select?.();
  }

  protected onInput(event: Event): void {
    const raw = (event.target as HTMLInputElement).value;
    if (!this.isEditing()) {
      this.isEditing.set(true);
    }
    this.draftText.set(raw);
  }

  protected onInputCommit(event: Event): void {
    const inputEl = event.target as HTMLInputElement;
    this.commitText(inputEl.value);
    if (typeof document === 'undefined' || document.activeElement !== inputEl) {
      this.isEditing.set(false);
    }
  }

  protected onInputBlur(event: FocusEvent): void {
    if (!this.isEditing()) return;
    const raw = (event.target as HTMLInputElement).value;
    this.commitText(raw);
    this.isEditing.set(false);
  }

  protected onInputKeyDown(event: KeyboardEvent): void {
    if (event.key === 'Enter') {
      if (event.isComposing) return;
      event.preventDefault();
      const inputEl = event.target as HTMLInputElement;
      this.commitText(inputEl.value);
      this.isEditing.set(false);
      inputEl.blur();
    } else if (event.key === 'Escape') {
      event.preventDefault();
      this.pendingCommitted.set(null);
      this.draftText.set(this.formattedValue());
      this.isEditing.set(false);
      (event.target as HTMLInputElement).blur();
    }
  }

  private commitText(raw: string): void {
    const trimmed = raw.trim();
    if (!trimmed) {
      this.pendingCommitted.set(null);
      this.draftText.set(this.formattedValue());
      return;
    }
    const parsed = Number(trimmed);
    if (!Number.isFinite(parsed)) {
      this.pendingCommitted.set(null);
      this.draftText.set(this.formattedValue());
      return;
    }
    const clamped = Math.max(this.min(), Math.min(this.max(), Math.round(parsed * 10) / 10));
    const formatted = Number.isInteger(clamped) ? String(clamped) : clamped.toFixed(1);
    this.draftText.set(formatted);
    this.pendingCommitted.set(formatted);
    this.valueCommit.emit(clamped);
  }
}
