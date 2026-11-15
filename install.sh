#!/usr/bin/env bash
# Turnkey One-Line Installer for Psalm Stage on macOS
# Usage: curl -fsSL https://raw.githubusercontent.com/adedayo/psalm-stage/main/install.sh | bash

set -e

echo ""
echo "=========================================================="
echo "          PSALM STAGE — Live Worship Multitrack           "
echo "=========================================================="
echo ""

# 1. Check OS
if [ "$(uname -s)" != "Darwin" ]; then
    echo "This installation script is for macOS."
    echo "For Windows, please download the .exe installer from:"
    echo "https://github.com/adedayo/psalm-stage/releases/latest"
    exit 1
fi

# 2. Detect CPU architecture
ARCH=$(uname -m)
if [ "$ARCH" = "arm64" ]; then
    ASSET_NAME="aarch64"
    echo "Detected: Apple Silicon Mac (M1/M2/M3/M4)"
else
    ASSET_NAME="x64"
    echo "Detected: Intel Mac (x86_64)"
fi

echo "Fetching latest release from GitHub..."
REPO="adedayo/psalm-stage"
LATEST_TAG=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$LATEST_TAG" ]; then
    echo "Could not fetch release tag automatically. Defaulting to v0.1.0"
    LATEST_TAG="v0.1.0"
fi

CLEAN_TAG="${LATEST_TAG#v}"
DMG_NAME="Psalm.Stage_${CLEAN_TAG}_${ASSET_NAME}.dmg"
DOWNLOAD_URL="https://github.com/$REPO/releases/download/$LATEST_TAG/$DMG_NAME"
TMP_DIR=$(mktemp -d)
TMP_DMG="$TMP_DIR/psalm-stage.dmg"
MOUNT_POINT="$TMP_DIR/mount"

echo "Downloading Psalm Stage $LATEST_TAG ($DMG_NAME)..."
if ! curl -L -f -o "$TMP_DMG" "$DOWNLOAD_URL" 2>/dev/null; then
    echo "Direct release download failed or release is building."
    echo "You can download the package manually from: https://github.com/$REPO/releases"
    rm -rf "$TMP_DIR"
    exit 1
fi

echo "Mounting disk image..."
mkdir -p "$MOUNT_POINT"
hdiutil attach "$TMP_DMG" -nobrowse -quiet -mountpoint "$MOUNT_POINT"

echo "Installing Psalm Stage to /Applications..."
rm -rf "/Applications/Psalm Stage.app"
cp -R "$MOUNT_POINT/Psalm Stage.app" /Applications/

echo "Unmounting installer..."
hdiutil detach "$MOUNT_POINT" -quiet || true
rm -rf "$TMP_DIR"

echo "==> Stripping Gatekeeper quarantine attribute (com.apple.quarantine)..."
xattr -cr "/Applications/Psalm Stage.app" 2>/dev/null || true

echo ""
echo "=========================================================="
echo "  SUCCESS! Psalm Stage is installed in your Applications! "
echo "=========================================================="
echo ""
echo "You can launch it anytime from Spotlight (Cmd+Space -> Psalm Stage)"
echo "or by double-clicking /Applications/Psalm Stage.app."
echo ""

# Launch application
open "/Applications/Psalm Stage.app"

