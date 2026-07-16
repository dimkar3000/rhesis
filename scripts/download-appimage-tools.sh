#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

TOOLS_DIR="${BUILD_DIR:-$PROJECT_DIR/build}/tools/appimage"

echo "Downloading AppImage tools to $TOOLS_DIR..."
mkdir -p "$TOOLS_DIR"

wget -q -O "$TOOLS_DIR/linuxdeploy-x86_64.AppImage" \
    "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
chmod +x "$TOOLS_DIR/linuxdeploy-x86_64.AppImage"

wget -q -O "$TOOLS_DIR/linuxdeploy-plugin-qt-x86_64.AppImage" \
    "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage"
chmod +x "$TOOLS_DIR/linuxdeploy-plugin-qt-x86_64.AppImage"

wget -q -O "$TOOLS_DIR/appimagetool-x86_64.AppImage" \
    "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
chmod +x "$TOOLS_DIR/appimagetool-x86_64.AppImage"

echo "Downloaded:"
ls -lh "$TOOLS_DIR"/*.AppImage
