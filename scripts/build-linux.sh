#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# --- Parse arguments ---
CLEAN_BUILD=false
APPIMAGE_ARGS=()

usage() {
    echo "Usage: $(basename "$0") [OPTIONS]"
    echo ""
    echo "Builds every Linux package in one go: Flatpak, AppImage, and the"
    echo "desktop tarball (global and per-user --local variants)."
    echo "Requires the shared build artifacts (run scripts/build-common.sh first"
    echo "or restore them from the build-app.yml CI cache) plus Flatpak."
    echo ""
    echo "Environment:"
    echo "  RUST_BUILD_TYPE  Must match the profile the shared build used"
    echo ""
    echo "Options:"
    echo "  --clean          Clean the corresponding build trees before packaging"
    echo "  --verbose        Show full command output (default: quiet)"
    echo "  --no-spinner     Disable spinner animation (plain output)"
    echo "  --linuxdeploy PATH     Pre-installed linuxdeploy AppImage (AppImage packaging)"
    echo "  --appimagetool PATH    Pre-installed appimagetool AppImage (AppImage packaging)"
    echo "  --help           Show this help message and exit"
    exit 0
}

source "$SCRIPT_DIR/common.sh"

while [[ $# -gt 0 ]]; do
    case $1 in
        --clean) CLEAN_BUILD=true; shift ;;
        --verbose) VERBOSE=true; shift ;;
        --no-spinner) NO_SPINNER=true; shift ;;
        --linuxdeploy) APPIMAGE_ARGS+=(--linuxdeploy "$2"); shift 2 ;;
        --appimagetool) APPIMAGE_ARGS+=(--appimagetool "$2"); shift 2 ;;
        --help) usage ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done

FORWARD_ARGS=()
[ "$CLEAN_BUILD" = true ] && FORWARD_ARGS+=(--clean)
[ "$VERBOSE" = true ] && FORWARD_ARGS+=(--verbose)
[ "$NO_SPINNER" = true ] && FORWARD_ARGS+=(--no-spinner)

# The CI image preinstalls the flatpak org.kde.Sdk runtime at image build
# time (Dockerfile), so packaging never downloads it again; locals install
# it once with `flatpak install` themselves.
step "Build Flatpak" \
    "$SCRIPT_DIR/build-flatpak.sh" "${FORWARD_ARGS[@]+"${FORWARD_ARGS[@]}"}" --no-runtimes

step "Build AppImage" \
    "$SCRIPT_DIR/build-appimage.sh" "${FORWARD_ARGS[@]+"${FORWARD_ARGS[@]}"}" \
        "${APPIMAGE_ARGS[@]+"${APPIMAGE_ARGS[@]}"}"

step "Build desktop tarball" \
    "$SCRIPT_DIR/build-desktop.sh" "${FORWARD_ARGS[@]+"${FORWARD_ARGS[@]}"}"

step "Build desktop tarball (local)" \
    "$SCRIPT_DIR/build-desktop.sh" "${FORWARD_ARGS[@]+"${FORWARD_ARGS[@]}"}" --local
