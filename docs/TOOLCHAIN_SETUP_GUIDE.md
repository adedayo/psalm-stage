# Optional Workbench Toolchain Setup Guide

Psalm Stage's **Live Stage** playback engine is completely self-contained and requires **zero** external CLI tools, Python packages, or ML runtimes.

If you want to use the **Import Workbench** for **YouTube URL audio downloads** or **local AI stem separation** (UVR / Demucs), install the optional tools for your platform below.

---

## Tool Matrix

| Tool | Required For | Notes |
| :--- | :--- | :--- |
| **`ffmpeg`** | YouTube audio extraction & `.m4a`/`.aac` conversion | Not required for native `.wav`, `.mp3`, `.flac`, or `.ogg` files |
| **`yt-dlp`** | Downloading audio from YouTube URLs | Not required for local audio files |
| **`audio-separator` (UVR CLI)** | Local UVR stem separation (`MDX-Net`, `Roformer`, `Demucs`) | Optional — only needed if splitting full mixes locally |
| **`demucs`** | Alternative local 4-stem / 6-stem AI separation | Optional alternative to `audio-separator` |

> [!TIP] Zero-CLI Alternative: Ultimate Vocal Remover (UVR) Desktop GUI
> If you prefer not to use Terminal/PowerShell, you can run the desktop **Ultimate Vocal Remover (UVR)** app directly, export stems into a folder, and open that folder via **Import Workbench → Open Folder** or **Setlist Studio → Needs Setup**.

---

## 1. macOS (Apple Silicon & Intel)

### Step 1: Install YouTube & Audio Tools (`yt-dlp` + `ffmpeg`)
Using [Homebrew](https://brew.sh):
```bash
brew install ffmpeg yt-dlp
```

### Step 2: (Optional) Install Local UVR Stem Separation (`audio-separator`)
We recommend `pipx` so Python dependencies stay isolated:
```bash
brew install pipx
pipx ensurepath
pipx install "audio-separator[cpu]"
```
*(Or for Demucs: `pipx install demucs`)*

### Step 3: Verify & Re-Scan in Psalm Stage
```bash
which ffmpeg yt-dlp audio-separator
```
- Homebrew installs to `/opt/homebrew/bin` (Apple Silicon) or `/usr/local/bin` (Intel).
- `pipx` installs to `~/.local/bin`.
- In Psalm Stage, open **Import Workbench → Optional Toolchain Setup → Re-Scan** (or use **Browse…** to pick a binary path manually).

---

## 2. Windows 10 / 11

### Step 1: Install YouTube & Audio Tools (`yt-dlp` + `ffmpeg`)
Open **PowerShell** or **Windows Terminal**:
```powershell
winget install Gyan.FFmpeg yt-dlp.yt-dlp
```
*(Alternatively via Scoop: `scoop install ffmpeg yt-dlp`)*

### Step 2: (Optional) Install Local UVR Stem Separation (`audio-separator`)
Install Python 3.11 (recommended for widest PyTorch/ONNX wheel compatibility) and `pipx`:
```powershell
winget install Python.Python.3.11
pip install pipx
pipx ensurepath
```
Close and reopen PowerShell, then install UVR CLI:
- **CPU (universal):**
  ```powershell
  pipx install "audio-separator[cpu]"
  ```
- **NVIDIA CUDA GPU (fast):**
  ```powershell
  pipx install "audio-separator[gpu]"
  ```

### Step 3: Verify & Re-Scan in Psalm Stage
```powershell
where.exe ffmpeg yt-dlp audio-separator
```
Click **Re-Scan** inside **Import Workbench → Optional Toolchain Setup**. If your `PATH` was updated while Psalm Stage was open, either restart Psalm Stage or select the `.exe` via the **Browse…** button (`%LOCALAPPDATA%\Microsoft\WinGet\Links` or `%USERPROFILE%\.local\bin`).

---

## 3. Linux (Ubuntu / Debian / Fedora / Arch)

### Ubuntu / Debian
```bash
sudo apt update && sudo apt install -y ffmpeg pipx
pipx ensurepath
pipx install yt-dlp
pipx install "audio-separator[cpu]"
```

### Fedora
```bash
sudo dnf install -y ffmpeg pipx yt-dlp
pipx ensurepath
pipx install "audio-separator[cpu]"
```

### Arch Linux
```bash
sudo pacman -S ffmpeg python-pipx yt-dlp
pipx ensurepath
pipx install "audio-separator[cpu]"
```
*(For NVIDIA CUDA GPUs on Linux, replace `[cpu]` with `[gpu]`.)*

---

## Troubleshooting

- **"yt-dlp was not found" after installing**:
  1. Run `which yt-dlp` (macOS/Linux) or `where.exe yt-dlp` (Windows) in a fresh terminal to confirm it is installed.
  2. In **Import Workbench → Optional Toolchain Setup**, click **Re-Scan** — or click **…** next to **Custom yt-dlp Path** and pick the binary directly, then click **Save Paths & Detect**.
- **YouTube download fails with FFmpeg error**:
  Make sure `ffmpeg` is also installed (`brew install ffmpeg` / `winget install Gyan.FFmpeg`) so `yt-dlp` can extract WAV/audio cleanly.
- **First UVR run takes longer**:
  The first time you run `audio-separator` with a preset (e.g. `htdemucs_ft.yaml` or `UVR-MDX-NET-Inst_HQ_3.onnx`), it downloads the model weights once and caches them locally for offline use afterward.

