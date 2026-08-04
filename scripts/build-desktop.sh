#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

BUILD_DIR="$PROJECT_DIR/build"
ARTIFACTS_DIR="$BUILD_DIR/artifacts"
VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

# --- Parse arguments ---
LOCAL_INSTALL=false
CLEAN_BUILD=false

source "$SCRIPT_DIR/common.sh"

usage() {
    echo "Usage: $(basename "$0") [OPTIONS]"
    echo ""
    echo "Builds a native desktop tarball from pre-built artifacts."
    echo "Run build-common.sh first to produce artifacts."
    echo ""
    echo "The tarball contains the final folder layout: extract the global"
    echo "tarball as root to '/' or the --local tarball to \$HOME."
    echo ""
    echo "Options:"
    echo "  --local         Per-user layout (~/.local); global layout by default"
    echo "  --clean         Clean and rebuild artifacts from scratch, then build the tarball"
    echo "  --verbose       Show full command output (default: quiet)"
    echo "  --no-spinner    Disable spinner animation (plain output)"
    echo "  --help          Show this help message and exit"
    exit 0
}

while [[ $# -gt 0 ]]; do
    case $1 in
        --local) LOCAL_INSTALL=true; shift ;;
        --clean) CLEAN_BUILD=true; shift ;;
        --verbose) VERBOSE=true; shift ;;
        --no-spinner) NO_SPINNER=true; shift ;;
        --help) usage ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done

# --- Install the app payload into a single folder (bin/, lib/, LanguageTool/, translations/) ---
install_payload() {
    local prefix="$1"
    local bin_dir="$prefix/bin"
    local lib_dir="$prefix/lib"
    local trans_dir="$prefix/translations"

    mkdir -p "$bin_dir" "$lib_dir" "$trans_dir"

    cp "$ARTIFACTS_DIR/app/bin/rhesis" "$bin_dir/"
    cp "$ARTIFACTS_DIR/fastText/fasttext" "$bin_dir/"

    mkdir -p "$lib_dir/jre"
    cp -r "$ARTIFACTS_DIR/java/jre/." "$lib_dir/jre/"
    ln -sf ../lib/jre/bin/java "$bin_dir/java"

    cp -r "$ARTIFACTS_DIR/LanguageTool" "$prefix/LanguageTool"
    cat > "$prefix/LanguageTool/server.properties" << EOF
fasttextModel=../lid.176.ftz
fasttextBinary=../bin/fasttext
EOF
    cp "$ARTIFACTS_DIR/lid.176.ftz" "$prefix/"

    if [ -d "$ARTIFACTS_DIR/app/share/rhesis/translations" ]; then
        cp "$ARTIFACTS_DIR/app/share/rhesis/translations/"*.qm "$trans_dir/"
    fi
}

install_desktop_files() {
    # Desktop entry template, icon, metainfo and license all live inside the
    # rhesis folder; only the desktop entry is also written to the XDG
    # applications directory by the installer (with an absolute Icon= path).
    local prefix="$1"
    cp "$ARTIFACTS_DIR/app/share/applications/io.github.dimkar3000.rhesis.desktop" "$prefix/"
    cp "$ARTIFACTS_DIR/app/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png" "$prefix/"
    cp "$ARTIFACTS_DIR/app/share/metainfo/io.github.dimkar3000.rhesis.metainfo.xml" "$prefix/"
    cp "$ARTIFACTS_DIR/app/share/licenses/rhesis/License.md" "$prefix/"
}

install_launcher() {
    local path="$1"
    mkdir -p "$(dirname "$path")"
    cp "$SCRIPT_DIR/runner.sh" "$path"
    chmod +x "$path"
}

# --- Main ---
main() {
    COMMON_ARGS=()
    [ "$VERBOSE" = true ] && COMMON_ARGS+=(--verbose)
    [ "$NO_SPINNER" = true ] && COMMON_ARGS+=(--no-spinner)
    [ "$CLEAN_BUILD" = true ] && COMMON_ARGS+=(--clean)

    "$SCRIPT_DIR/build-common.sh" "${COMMON_ARGS[@]}"

    echo "=== Native Desktop Build ==="
    echo ""

    local staging="$BUILD_DIR/desktop-staging"
    rm -rf "$staging"

    local output_name=""
    if [ "$LOCAL_INSTALL" = true ]; then
        step "Assembling per-user layout" assemble_local "$staging"
        output_name="rhesis-${VERSION}-x86_64-local.tar.gz"
    else
        step "Assembling global layout" assemble_global "$staging"
        output_name="rhesis-${VERSION}-x86_64.tar.gz"
    fi

    local output_dir="$ARTIFACTS_DIR/desktop"
    mkdir -p "$output_dir"
    step "Creating tarball" tar -C "$staging" -czf "$output_dir/$output_name" .
    rm -rf "$staging"

    echo ""
    if [ "$LOCAL_INSTALL" = true ]; then
        echo "Per-user tarball created: $output_dir/$output_name"
        echo "Install with: RHESIS_ARCHIVE_SRC=$output_dir scripts/runner.sh --install local"
    else
        echo "Global tarball created: $output_dir/$output_name"
        echo "Install with: sudo RHESIS_ARCHIVE_SRC=$output_dir scripts/runner.sh --install"
    fi
}

assemble_global() {
    local staging="$1"

    install_payload "$staging/usr/share/rhesis"
    install_desktop_files "$staging/usr/share/rhesis"
    install_launcher "$staging/usr/bin/rhesis"
}

assemble_local() {
    local staging="$1"

    install_payload "$staging/.local/share/rhesis"
    install_desktop_files "$staging/.local/share/rhesis"
    install_launcher "$staging/.local/bin/rhesis"
}

main "$@"
