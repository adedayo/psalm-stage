# Psalm Stage — Brand Assets & Logo Guide

This directory contains the official brand identity, vector logos, icons, and distribution media for **Psalm Stage**.

---

## 🎨 Visual Identity & Metaphor

The **Psalm Stage** emblem (**Sanctuary Stage Arch**) unites worship stage architecture with precision live multitrack playback:

1. **Concentric Sanctuary Acoustic Arches**:
   Three ascending golden acoustic shells radiate outward from the stage deck, evoking both a live auditorium proscenium and harmonic acoustic resonance.
2. **Stage Spotlight & Live Play Portal**:
   An overhead warm tungsten spotlight cone illuminates the center, framing a luminous forward play vector ($\blacktriangleright$) nestled inside the innermost arch.
3. **Stage Deck & 0 ms Native Sync LED**:
   A beveled amber stage horizon bar grounds the emblem, housing a recessed dark status channel with an emerald green indicator dot (`#10B981`) symbolizing bit-perfect, zero-latency DSP (`0 ms Native`).
4. **4/4 Metronome Downbeat & Measure Pulse**:
   Beneath the stage deck, four rhythmic pulse dots represent a 4/4 bar—a bright accented downbeat (`#FEF08A`) followed by three beat markers (`#F59E0B`), reflecting the sample-accurate `ClickSynth` engine.
5. **High-Contrast Stage Palette**:
   - **Radiant Stage Amber (`#F59E0B` $\to$ `#FBBF24` $\to$ `#FEF08A`)**: Warm stage wash and sanctuary spotlight glow.
   - **Obsidian / Zinc 950 (`#09090B` $\to$ `#18181B`)**: High-visibility dark mode for dimly lit stage environments.
   - **Sync Emerald (`#10B981`)**: Deterministic locked timing.

---

## 📁 Available Asset Formats

All primary vector assets are maintained in [`public/`](../public/) and [`docs/assets/`](assets/):

| Asset Name | File Link | Primary Purpose | Recommended Size |
| :--- | :--- | :--- | :--- |
| **Full GitHub Banner** | [`public/psalm-stage-banner.svg`](../public/psalm-stage-banner.svg) | GitHub README, documentation headers, release announcements | $960 \times 240\text{ px}$ |
| **Standalone Mark (Transparent)** | [`public/psalm-stage-logo.svg`](../public/psalm-stage-logo.svg) | In-app transport bar, website navigation, software headers | Dynamic / Scalable Vector |
| **Square App Icon / Avatar** | [`public/psalm-stage-logo-mark.svg`](../public/psalm-stage-logo-mark.svg) | GitHub profile/repo avatar, social media preview | $512 \times 512\text{ px}$ squircle |
| **Horizontal Lockup (Transparent)** | [`public/psalm-stage-logo-horizontal.svg`](../public/psalm-stage-logo-horizontal.svg) | Splash screens, partner integrations, transparent navbar lockup | $820 \times 180\text{ px}$ |
| **Monochrome Vector** | [`public/psalm-stage-logo-mono.svg`](../public/psalm-stage-logo-mono.svg) | Single-color silkscreens, hardware controller faceplates, mono print | Scalable Vector |
| **Modern Vector Favicon** | [`public/favicon.svg`](../public/favicon.svg) | Modern web browsers (high-DPI vector rendering) | $64 \times 64\text{ px}$ |
| **Multi-Resolution Favicon** | [`public/favicon.ico`](../public/favicon.ico) | Legacy browser fallback and standard bookmarks | $16\text{–}64\text{ px}$ transparent ICO |
| **Desktop App Icons** | [`src-tauri/icons/`](../src-tauri/icons/) | Apple HIG macOS `.icns` ($824\times 824$ squircle on transparent $1024\times 1024$ canvas + drop shadow), Windows `.ico`, Linux & Store PNGs | $16 \times 16$ to $1024 \times 1024\text{ px}$ |

---

## 💻 In-App & Web Integration

### Angular Zoneless UI
```html
<div class="flex h-8 w-8 items-center justify-center rounded-lg bg-zinc-950 border border-zinc-800 shadow-sm p-1 overflow-hidden group">
  <img src="psalm-stage-logo.svg" alt="Psalm Stage" class="h-full w-full object-contain transform group-hover:scale-105 transition-transform" />
</div>
```

### HTML `<head>` Favicon
```html
<link rel="icon" type="image/svg+xml" href="favicon.svg">
<link rel="icon" type="image/x-icon" href="favicon.ico">
```

### Markdown / GitHub README
```html
<p align="center">
  <img src="public/psalm-stage-banner.svg" alt="Psalm Stage Logo" width="840" style="max-width: 100%; border-radius: 16px;" />
</p>
```
