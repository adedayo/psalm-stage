# SPEC-011: Automated Packaging, Installers & Distribution Pipeline

| Metadata | Value |
| :--- | :--- |
| **Spec ID** | `SPEC-011` |
| **Title** | Automated Packaging, Installers & Distribution Pipeline |
| **Module** | `.github/workflows/release.yml`, `src-tauri/tauri.conf.json` |
| **Status** | Implemented & Verified |
| **Priority** | P0 (Critical for Non-Developer End Users) |
| **Related Specs** | [SPEC-008: Stage UI](./SPEC-008-STAGE-UI-SPARTAN.md), [SPEC-010: Offline Caching](./SPEC-010-CLOUD-SYNC-OFFLINE.md) |

---

## 1. Overview & Operational Problem Statement

Worship pastors, band leaders, and volunteer sound technicians are **non-technical end users**. Expecting them to clone git repositories, install Node.js, install the Rust compiler toolchain, or run shell scripts in a terminal creates an insurmountable barrier to adoption and risks broken stage setups.

Psalm Stage must be distributed as **turnkey, double-clickable standalone installers**:
- **macOS**: Drag-and-drop `.dmg` disk image.
- **Windows**: Standard `.exe` installer (NSIS) or `.msi` package with Start Menu and Desktop shortcuts.
- **Linux**: Self-contained `.AppImage` and `.deb` packages.

---

## 2. Technical Architecture & Packaging Targets

```mermaid
flowchart TD
    Repo["Git Tag (e.g. v0.1.0)"] --> Action["GitHub Actions Matrix (Release Workflow)"]
    
    subgraph macOS_Runner["macOS Runner (aarch64 & x86_64)"]
        M1["Build Angular Frontend (dist/psalm-stage/browser)"]
        M2["Compile Rust Binary (cargo build --release)"]
        M3["Bundle .app & .dmg Disk Image"]
        M4["Codesign & Notarize (Apple Developer ID)"]
        M1 --> M2 --> M3 --> M4
    end

    subgraph Windows_Runner["Windows Runner (x86_64)"]
        W1["Build Angular Frontend"]
        W2["Compile Rust Binary"]
        W3["Bundle NSIS Installer (.exe) & MSI (.msi)"]
        W4["Authenticode Signing"]
        W1 --> W2 --> W3 --> W4
    end

    subgraph Linux_Runner["Ubuntu Runner (x86_64)"]
        L1["Install WebKit & ALSA dev headers"]
        L2["Build & Bundle .AppImage & .deb"]
        L1 --> L2
    end

    Action --> macOS_Runner
    Action --> Windows_Runner
    Action --> Linux_Runner

    M4 --> Publish["Publish to GitHub Releases"]
    W4 --> Publish
    L2 --> Publish

    Publish --> Users["End Users Download from Releases Page"]
```

---

## 3. Configuration Specifications (`tauri.conf.json`)

The desktop bundler is configured in `src-tauri/tauri.conf.json`:

```json
{
  "productName": "Psalm Stage",
  "version": "0.1.0",
  "identifier": "org.psalmstage.app",
  "build": {
    "frontendDist": "../dist/psalm-stage/browser",
    "beforeBuildCommand": "npm run build"
  },
  "bundle": {
    "active": true,
    "targets": "all",
    "category": "AudioVideo",
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ],
    "macOS": {
      "minimumSystemVersion": "10.15",
      "dmg": {
        "windowSize": { "width": 660, "height": 400 },
        "appPosition": { "x": 180, "y": 170 },
        "applicationFolderPosition": { "x": 480, "y": 170 }
      }
    },
    "windows": {
      "nsis": {
        "installMode": "currentUser"
      }
    }
  }
}
```

---

## 4. Release Automation Workflow

The release process is triggered by pushing a git tag:
```bash
git tag v0.1.0
git push origin v0.1.0
```
This automatically invokes `.github/workflows/release.yml`, compiling all platform targets and publishing the binary assets directly to the GitHub Releases page with release notes and SHA-256 checksums.

---

## 5. Verification & Testing

- **`test_dmg_bundle_structure`**: Verifies that the `.dmg` contains a valid `.app` bundle, `Applications` symlink, volume icon, and plist entries.
- **`test_windows_nsis_installer`**: Verifies that the NSIS installer properly writes uninstallation registry entries and installs cleanly in standard user directory without requiring Administrator elevation.

