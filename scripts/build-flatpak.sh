#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

BUILD_DIR="$PROJECT_DIR/build"
VENV_DIR="$BUILD_DIR/venv"
GENERATOR_SCRIPT="$BUILD_DIR/flatpak-cargo-generator.py"
CARGO_SOURCES="$BUILD_DIR/cargo-sources.json"
FLATPAK_OUTPUT="$BUILD_DIR/flatpak-build-dir"
FLATPAK_STATE="$BUILD_DIR/flatpak-state"

cd "$SCRIPT_DIR"

mkdir -p "$BUILD_DIR"

# --- Install flatpak runtimes and extensions ---
echo "Ensuring flatpak runtimes and extensions are installed..."
flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
flatpak install --user --noninteractive flathub org.kde.Platform//6.10
flatpak install --user --noninteractive flathub org.kde.Sdk//6.10
flatpak install --user --noninteractive flathub org.freedesktop.Sdk.Extension.rust-stable//24.08
flatpak install --user --noninteractive flathub org.freedesktop.Sdk.Extension.openjdk17//24.08

# --- Python virtual environment ---
if [ ! -d "$VENV_DIR" ]; then
    echo "Creating Python virtual environment..."
    python3 -m venv "$VENV_DIR"
fi

source "$VENV_DIR/bin/activate"

echo "Installing Python dependencies..."
pip install -q -r "$PROJECT_DIR/requirements.txt"

# --- Download flatpak-cargo-generator and generate vendored sources ---
GENERATOR_URL="https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py"

if [ ! -f "$GENERATOR_SCRIPT" ]; then
    echo "Downloading flatpak-cargo-generator.py..."
    curl -fsSLo "$GENERATOR_SCRIPT" "$GENERATOR_URL"
fi

echo "Generating cargo-sources.json..."
python "$GENERATOR_SCRIPT" "$PROJECT_DIR/Cargo.lock" -o "$CARGO_SOURCES"


# --- Build flatpak ---
echo "Building flatpak..."
flatpak-builder \
    --user \
    --force-clean \
    --state-dir "$FLATPAK_STATE" \
    "$FLATPAK_OUTPUT" \
    "$PROJECT_DIR/io.github.dimkar3000.rhesis.json"

echo "Build complete."

echo "Exporting to local repo..."
flatpak build-export "$BUILD_DIR/rhesis-master" "$FLATPAK_OUTPUT"
echo "Creating bundle..."
flatpak build-bundle "$BUILD_DIR/rhesis-master" "$BUILD_DIR/rhesis.flatpak" io.github.dimkar3000.rhesis
echo "Bundle created: $BUILD_DIR/rhesis.flatpak"
