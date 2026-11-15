#!/bin/bash
# Builds a standalone macOS .pkg installer with an automated postinstall quarantine-stripping hook
set -e

APP_PATH="${1:-src-tauri/target/release/bundle/macos/Psalm Stage.app}"
VERSION="${2:-0.1.0}"
ARCH="$(uname -m)"
OUTPUT_DIR="src-tauri/target/release/bundle/pkg"
OUTPUT_PKG="$OUTPUT_DIR/Psalm-Stage_${VERSION}_${ARCH}.pkg"

if [ ! -d "$APP_PATH" ]; then
    echo "Error: Application bundle not found at: $APP_PATH"
    echo "Please run 'npm run package' first to generate the app bundle."
    exit 1
fi

mkdir -p "$OUTPUT_DIR"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "==> Building macOS .pkg installer for Psalm Stage..."
pkgbuild --component "$APP_PATH" \
    --install-location "/Applications" \
    --scripts "$SCRIPT_DIR" \
    --identifier "org.psalmstage.app" \
    --version "$VERSION" \
    "$OUTPUT_PKG"

echo "==> Successfully created standalone installer: $OUTPUT_PKG"

