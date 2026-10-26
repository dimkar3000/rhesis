#!/usr/bin/env bash
set -euo pipefail

# Flatpak builds are PINNED to the Qt of the manifest's org.kde.Platform
# (currently 6.11). The manifest is "buildsystem": simple — it ships the
# host-built binary as-is — so the Qt used to build the artifacts must be the
# same Qt the runtime provides, or the bundle dies at startup with
# "version `Qt_6.x` not found". Hosts whose Qt matches build locally; rollback
# hosts pin the build against the Qt dev tree that ships inside the flatpak
# org.kde.Sdk itself (installed next to the runtime) — no container needed.

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

BUILD_DIR="$PROJECT_DIR/build"
FLATPAK_OUTPUT="$BUILD_DIR/flatpak-build-dir"
FLATPAK_STATE="$BUILD_DIR/flatpak-state"
ARTIFACTS_DIR="$BUILD_DIR/artifacts"
FLATPAK_ARTIFACT_DIR="$ARTIFACTS_DIR/flatpak"

# --- Parse arguments ---
CLEAN_BUILD=false
INSTALL_RUNTIMES=true

source "$SCRIPT_DIR/common.sh"

while [[ $# -gt 0 ]]; do
    case $1 in
        --clean) CLEAN_BUILD=true; shift ;;
        --verbose) VERBOSE=true; shift ;;
        --no-spinner) NO_SPINNER=true; shift ;;
        --no-runtimes) INSTALL_RUNTIMES=false; shift ;;
        --help)
            echo "Usage: $(basename "$0") [OPTIONS]"
            echo ""
            echo "Builds a Flatpak pinned to the Qt of the manifest's org.kde.Platform."
            echo ""
            echo "Host Qt matches the runtime: artifacts are built locally (build-common.sh)."
            echo "Otherwise artifacts build against the Qt dev tree inside the flatpak"
            echo "org.kde.Sdk of the pinned runtime — same Qt as the runtime, no container."
            echo "Cached build output from a foreign Qt is wiped in that mode, so expect a"
            echo "full release rebuild."
            echo ""
            echo "Options:"
            echo "  --clean         Clean and rebuild artifacts from scratch, then build the Flatpak"
            echo "  --verbose       Show full command output (default: quiet)"
            echo "  --no-spinner    Disable spinner animation (plain output)"
            echo "  --no-runtimes   Skip installing flatpak runtimes (use when already installed)"
            echo "  --help          Show this help message and exit"
            exit 0
            ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done

# --- Runtime / toolchain pinning ---------------------------------------------

# The org.kde.Platform version the Flatpak targets: taken from the manifest, so
# one bump of runtime-version moves the pin everywhere (script included).
runtime_version() {
    awk -F'"' '/"runtime-version"/{print $4; exit}' "$PROJECT_DIR/io.github.dimkar3000.rhesis.json"
}

# Qt the host would build against ("" when no Qt toolchain is installed).
host_qt_version() {
    qmake6 -query QT_VERSION 2>/dev/null || qmake -query QT_VERSION 2>/dev/null || true
}

build_artifacts_host() {
    "$SCRIPT_DIR/build-common.sh" "${COMMON_ARGS[@]}"
}

# Build the artifacts inside an org.kde.Sdk flatpak sandbox of the pinned
# runtime. The SDK carries the dev files of the exact Qt the runtime offers,
# so headers/toolchain/shlibs are coherent — the same pairing CI gets from its
# Docker image, without podman and without leaking SDK libs into host tools.
# Rust comes from the SDK's rust extension (the same pin the CI image uses);
# the cargo cache is shared through the mounted $HOME/.cargo.
build_artifacts_sdk() {
    local rt="$1" extver="24.08" build_root

    if ! command -v flatpak >/dev/null 2>&1; then
        echo "Error: flatpak is required to pin the build to Qt $rt (host Qt differs or is missing)." >&2
        exit 1
    fi

    # Register the refs the sandbox needs; no-ops when already installed.
    echo "Ensuring pinned build SDK refs: org.kde.Sdk//$rt + rust extension//$extver (no-op when installed) ..."
    flatpak install --user --noninteractive flathub \
        "org.kde.Sdk//$rt" "org.kde.Platform//$rt" \
        "org.freedesktop.Sdk.Extension.rust-stable//${extver}" \
        || { echo "Error: failed to install the pinned SDK references from flathub." >&2; exit 1; }

    build_root="$BUILD_DIR/flatpak-sdk-build"
    rm -rf "$build_root"
    flatpak build-init "$build_root" io.github.dimkar3000.rhesis "org.kde.Sdk" "org.kde.Platform" "$rt"

    echo "Pinned build: launching the org.kde.Sdk//$rt sandbox ..."
    echo "Pinned build: (it starts by wiping build/cmake-build and build/target — stale foreign-Qt fingerprints; expect quiet minutes) ..."

    flatpak build \
        --share=network --share=ipc \
        --filesystem="$HOME/.cargo" \
        --filesystem="$PROJECT_DIR" \
        --env=HOME=/tmp/build-home \
        --env=CARGO_HOME=$HOME/.cargo \
        --env=PROJECT_DIR="$PROJECT_DIR" \
        --env=BUILD_COMMON_ARGS="${COMMON_ARGS[*]}" \
        --env=SKIP_LUPDATE=1 \
        --env=RUST_BUILD_TYPE="${RUST_BUILD_TYPE:-}" \
        "$build_root" sh -c '
            export PATH="$PATH:/usr/lib/sdk/rust-stable/bin"
            export LANG=C.UTF-8
            mkdir -p /tmp/build-home
            cd "$PROJECT_DIR"
            rm -rf build/cmake-build build/target
            echo "[pinned] caches wiped; running build-common ..."
            ./scripts/build-common.sh ${BUILD_COMMON_ARGS}
        '
}

# --- Main ---
main() {
    COMMON_ARGS=()
    [ "$VERBOSE" = true ] && COMMON_ARGS+=(--verbose)
    [ "$NO_SPINNER" = true ] && COMMON_ARGS+=(--no-spinner)
    [ "$CLEAN_BUILD" = true ] && COMMON_ARGS+=(--clean)

    local runtime host_qt host_mm
    runtime="$(runtime_version)"
    host_qt="$(host_qt_version)"
    host_mm="$(printf '%s' "$host_qt" | cut -d. -f1-2)"

    echo "=== Flatpak Build ==="
    echo "Runtime pin : org.kde.Platform//${runtime}"
    if [ -n "$host_mm" ] && [ "$host_mm" = "$runtime" ]; then
        echo "Toolchain   : host Qt ${host_qt} matches the runtime; building artifacts locally"
        step "Building artifacts" build_artifacts_host
    else
        echo "Toolchain   : host Qt '${host_qt:-not found}' differs from ${runtime}; pinning the build against the flatpak SDK's Qt"
        step "Building artifacts (pinned Qt ${runtime})" build_artifacts_sdk "$runtime"
    fi

    echo ""
    if [ "$INSTALL_RUNTIMES" = true ]; then
        step "Installing flatpak runtimes" install_runtimes
    fi
    step "Building flatpak" build_flatpak
    step "Exporting to local repo" export_bundle

    echo ""
    echo "Bundle created: $FLATPAK_ARTIFACT_DIR/rhesis.flatpak"
}

install_runtimes() {
    flatpak remote-add --user --if-not-exists flathub https://flathub.org/repo/flathub.flatpakrepo
    flatpak install --user --noninteractive flathub "org.kde.Platform//$(runtime_version)"
}

build_flatpak() {
    flatpak-builder \
        --user \
        --disable-rofiles-fuse \
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
