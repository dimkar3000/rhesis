#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

BUILD_DIR="$PROJECT_DIR/build"
FLATPAK_OUTPUT="$BUILD_DIR/flatpak-build-dir"
FLATPAK_STATE="$BUILD_DIR/flatpak-state"
ARTIFACTS_DIR="$BUILD_DIR/artifacts"
FLATPAK_ARTIFACT_DIR="$ARTIFACTS_DIR/flatpak"

# --- Parse arguments ---
CLEAN_BUILD=false

source "$SCRIPT_DIR/common.sh"

while [[ $# -gt 0 ]]; do
    case $1 in
        --clean) CLEAN_BUILD=true; shift ;;
        --verbose) VERBOSE=true; shift ;;
        --no-spinner) NO_SPINNER=true; shift ;;
        --help)
            echo "Usage: $(basename "$0") [OPTIONS]"
            echo ""
            echo "Builds a Flatpak from pre-built artifacts."
            echo "Run build-common.sh first to produce artifacts."
            echo ""
            echo "Options:"
            echo "  --clean         Clean and rebuild artifacts from scratch, then build the Flatpak"
            echo "  --verbose       Show full command output (default: quiet)"
            echo "  --no-spinner    Disable spinner animation (plain output)"
            echo "  --help          Show this help message and exit"
            exit 0
            ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done

# --- Main ---
main() {
    COMMON_ARGS=()
    [ "$VERBOSE" = true ] && COMMON_ARGS+=(--verbose)
    [ "$NO_SPINNER" = true ] && COMMON_ARGS+=(--no-spinner)

    if [ "$CLEAN_BUILD" = true ]; then
        "$SCRIPT_DIR/build-common.sh" --clean "${COMMON_ARGS[@]}"
    fi

    if [ ! -d "$ARTIFACTS_DIR/app" ]; then
        "$SCRIPT_DIR/build-common.sh" "${COMMON_ARGS[@]}"
    fi

    echo "=== Flatpak Build ==="
    echo ""

    step "Installing flatpak runtimes" install_runtimes
    step "Building flatpak" build_flatpak
    step "Exporting to local repo" export_bundle

    echo ""
    echo "Bundle created: $FLATPAK_ARTIFACT_DIR/rhesis.flatpak"
}

install_runtimes() {
    flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
    flatpak install --user --noninteractive flathub org.kde.Platform//6.11
}

build_flatpak() {
    flatpak-builder \
        --user \
        --force-clean \
        --state-dir "$FLATPAK_STATE" \
        "$FLATPAK_OUTPUT" \
        "$PROJECT_DIR/io.github.dimkar3000.rhesis.json"
}

export_bundle() {
    flatpak build-export "$BUILD_DIR/rhesis-master" "$FLATPAK_OUTPUT"
    mkdir -p "$FLATPAK_ARTIFACT_DIR"
    flatpak build-bundle "$BUILD_DIR/rhesis-master" "$FLATPAK_ARTIFACT_DIR/rhesis.flatpak" io.github.dimkar3000.rhesis
}

main "$@"
