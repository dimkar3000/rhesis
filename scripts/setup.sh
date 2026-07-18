#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_DIR/build"

cd "$SCRIPT_DIR"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; }

LANGUAGETOOL_VERSION="${LANGUAGETOOL_VERSION:-Latest}"
# This url needs to match the download link from here: https://internal1.languagetool.org/snapshots/
LANGUAGETOOL_URL="${LANGUAGETOOL_URL:-https://internal1.languagetool.org/snapshots/LanguageTool-latest-snapshot.zip}"
LANGUAGETOOL_DIR="$BUILD_DIR/LanguageTool"

FASTTEXT_REPO="${FASTTEXT_REPO:-https://github.com/facebookresearch/fastText.git}"
FASTTEXT_DIR="$BUILD_DIR/fastText"

LID_MODEL_URL="${LID_MODEL_URL:-https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz}"
LID_MODEL_FILE="$BUILD_DIR/lid.176.ftz"

# --- Main ---
main() {
    echo "========================================"
    echo "  Rhesis - LanguageTool Setup Script"
    echo "========================================"
    echo ""

    mkdir -p "$BUILD_DIR"
    check_prerequisites
    download_languagetool
    setup_fasttext
    download_lid_model

    echo ""
    info "Setup complete!"
    echo ""
    echo "  LanguageTool: $BUILD_DIR/LanguageTool"
    echo "  fastText:     $BUILD_DIR/fastText"
    echo "  lid model:    $BUILD_DIR/lid.176.ftz"
    echo ""
    echo "You can now run 'cargo build' or 'scripts/build-flatpak.sh' to build the application."
}

check_prerequisites() {
    local missing=()

    if [ "${CI:-}" != "true" ] && ! command -v java &>/dev/null; then
        missing+=("java (openjdk 17+)")
    fi

    if ! command -v curl &>/dev/null && ! command -v wget &>/dev/null; then
        missing+=("curl or wget")
    fi

    if ! command -v unzip &>/dev/null; then
        missing+=("unzip")
    fi

    if ! command -v make &>/dev/null; then
        missing+=("make")
    fi

    if ! command -v g++ &>/dev/null && ! command -v c++ &>/dev/null; then
        missing+=("g++ (C++ compiler)")
    fi

    if ! command -v git &>/dev/null; then
        missing+=("git")
    fi

    if [ ${#missing[@]} -gt 0 ]; then
        error "Missing prerequisites:"
        for dep in "${missing[@]}"; do
            echo "  - $dep"
        done
        exit 1
    fi
}

download_languagetool() {
    if [ -d "$LANGUAGETOOL_DIR" ]; then
        info "LanguageTool directory '$LANGUAGETOOL_DIR' already exists, skipping download."
        return
    fi

    info "Downloading LanguageTool ${LANGUAGETOOL_VERSION}..."
    local tmpdir
    tmpdir="$(mktemp -d)"
    local archive="$tmpdir/languagetool.zip"

    if command -v curl &>/dev/null; then
        curl -fsSLo "$archive" "$LANGUAGETOOL_URL"
    else
        wget -qO "$archive" "$LANGUAGETOOL_URL"
    fi

    info "Extracting LanguageTool..."
    unzip -qo "$archive" -d "$tmpdir"

    local extracted_dir
    extracted_dir=("$tmpdir"/LanguageTool-*/)
    if [ ${#extracted_dir[@]} -eq 0 ] || [ ! -d "${extracted_dir[0]}" ]; then
        error "Extraction failed: no LanguageTool directory found."
        rm -rf "$tmpdir"
        exit 1
    fi

    mv "${extracted_dir[0]}" "$LANGUAGETOOL_DIR"
    rm -rf "$tmpdir"

    cat > "$LANGUAGETOOL_DIR/server.properties" <<-EOF
fasttextModel=../lid.176.ftz
fasttextBinary=../fastText/fasttext
EOF

    info "LanguageTool downloaded and configured."
}

setup_fasttext() {
    if [ -d "$FASTTEXT_DIR" ]; then
        info "fastText directory '$FASTTEXT_DIR' already exists, skipping."
        return
    fi

    info "Cloning fastText repository..."
    git clone --depth 1 "$FASTTEXT_REPO" "$FASTTEXT_DIR"

    info "Building fastText..."
    make -C "$FASTTEXT_DIR" -j"$(nproc)"

    info "fastText built successfully."
}

download_lid_model() {
    if [ -f "$LID_MODEL_FILE" ]; then
        info "lid.176.ftz already exists, skipping download."
        return
    fi

    info "Downloading language identification model (lid.176.ftz)..."

    curl -fsSLo "$LID_MODEL_FILE" "$LID_MODEL_URL"

    info "lid.176.ftz downloaded (size: $(du -h "$LID_MODEL_FILE" | cut -f1))."
}

main "$@"
