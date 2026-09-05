#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# --- Configuration ---
RUST_BUILD_TYPE="${RUST_BUILD_TYPE:-release-fast}"
INSTALL_PREFIX="${INSTALL_PREFIX:-}"
BUILD_DIR="${BUILD_DIR:-$PROJECT_DIR/build}"

# --- Parse arguments ---
CLEAN_BUILD=false

usage() {
    echo "Usage: $(basename "$0") [OPTIONS]"
    echo ""
    echo "Regenerates translations, then builds the Rust application, fastText, LanguageTool, and a trimmed JRE."
    echo "Requires CMake, Qt6, a Rust toolchain, and a JDK 17+ with jlink."
    echo ""
    echo "Options:"
    echo "  --clean         Delete the build directory and rebuild everything from scratch"
    echo "  --verbose       Show full command output (default: quiet)"
    echo "  --no-spinner    Disable spinner animation (plain output)"
    echo "  --help          Show this help message and exit"
    exit 0
}

source "$SCRIPT_DIR/common.sh"

while [[ $# -gt 0 ]]; do
    case $1 in
        --clean) CLEAN_BUILD=true; shift ;;
        --verbose) VERBOSE=true; shift ;;
        --no-spinner) NO_SPINNER=true; shift ;;
        --help) usage ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done

# --- Locate a Qt6 linguist tool, looking beyond PATH ---
# lupdate/lrelease often live in a Qt libexec dir (e.g. /usr/lib/qt6/bin)
# without being on PATH. Check command names first, then known locations.
find_qt_linguist_tool() {
    local name dir
    for name in "$@"; do
        if command -v "$name" &>/dev/null; then
            command -v "$name"
            return 0
        fi
    done
    for dir in /usr/lib/qt6/bin /usr/lib/x86_64-linux-gnu/qt6/bin /usr/lib64/qt6/bin /usr/local/lib/qt6/bin; do
        for name in "$@"; do
            if [ -x "$dir/$name" ]; then
                echo "$dir/$name"
                return 0
            fi
        done
    done
    return 1
}

# --- Refresh .ts sources from QML and recompile .qm, unconditionally ---
# Always runs (not only when inputs look stale) so newly added qsTr()
# strings always end up in the shipped translation files.
regenerate_translations() {
    cd "$PROJECT_DIR"

    local lupdate_cmd lrelease_cmd
    lupdate_cmd="$(find_qt_linguist_tool lupdate6 lupdate || true)"
    lrelease_cmd="$(find_qt_linguist_tool lrelease6 lrelease || true)"

    if [ -z "$lupdate_cmd" ] || [ -z "$lrelease_cmd" ]; then
        echo "Warning: Qt linguist tools (lupdate/lrelease) not found; translation files will not be regenerated" >&2
        return 0
    fi

    local qml_files=()
    while IFS= read -r f; do
        qml_files+=("$f")
    done < <(find "$PROJECT_DIR/src/interop/qml" -name '*.qml' | sort)
    if [ "${#qml_files[@]}" -eq 0 ]; then
        echo "Warning: no QML files found; skipping translation regeneration" >&2
        return 0
    fi

    shopt -s nullglob
    local ts_files=(translations/*.ts)
    shopt -u nullglob
    if [ "${#ts_files[@]}" -eq 0 ]; then
        echo "Warning: no .ts files in translations/; skipping translation regeneration" >&2
        return 0
    fi

    # Drop stale outputs so regeneration is unconditional, then refresh the
    # .ts sources from QML and recompile .qm next to them (.qm is gitignored;
    # local `cargo run` also picks these up via the ./translations fallback).
    rm -f translations/*.qm "$BUILD_DIR/cmake-build/"*.qm
    if ! "$lupdate_cmd" "${qml_files[@]}" -ts "${ts_files[@]}"; then
        echo "Warning: lupdate reported errors; .ts files may be incomplete, continuing with lrelease" >&2
    fi
    "$lrelease_cmd" "${ts_files[@]}"
}

# --- Main ---
main() {
    local install_root="$BUILD_DIR/artifacts/app"

    echo "=== Common Build Script ==="
    echo "Build type: $RUST_BUILD_TYPE"
    echo "Install root: $install_root"
    echo "Profile: $RUST_BUILD_TYPE"
    echo ""

    if [ "$CLEAN_BUILD" = true ]; then
        echo "Cleaning build directory..."
        rm -rf "$BUILD_DIR"
    fi

    step "Regenerating translations" regenerate_translations
    step "Building application" build_rust_app
    step "Building fastText" build_fasttext
    step "Downloading language model" setup_lid_model
    step "Regenerating metainfo releases" regenerate_metainfo
    step "Installing application files" install_app "$install_root"
    step "Setting up LanguageTool" setup_languagetool

    local java_install_root="$BUILD_DIR/artifacts/java"
    step "Creating trimmed JRE" create_trimmed_jre "$java_install_root"

    echo ""
    echo "=== Build Complete ==="
    echo "Installed to: $install_root"
}

build_rust_app() {
    cd "$PROJECT_DIR"
    mkdir -p "$BUILD_DIR/cmake-build"
    cd "$BUILD_DIR/cmake-build"

    if [ -n "${SDK:-}" ] && [ -d "$SDK/lib/x86_64-linux-gnu" ]; then
        export LD_LIBRARY_PATH="${SDK}/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi

    cmake -DCARGO_BUILD_PROFILE="$RUST_BUILD_TYPE" \
          -DCMAKE_INSTALL_PREFIX="$INSTALL_PREFIX" \
          "$PROJECT_DIR"
    make -j$(nproc)

    cd "$PROJECT_DIR"
}

build_fasttext() {
    if [ -f "$BUILD_DIR/fasttext-src/fasttext" ]; then
        return
    fi

    if [ ! -f "$BUILD_DIR/fasttext-src/Makefile" ]; then
        rm -rf "$BUILD_DIR/fasttext-src"
        git clone --depth 1 --branch v0.9.2 https://github.com/facebookresearch/fastText.git "$BUILD_DIR/fasttext-src"
    fi

    make -C "$BUILD_DIR/fasttext-src" -j$(nproc) CXXFLAGS="-pthread -std=c++17 -march=native -include cstdint"
}

# --- Regenerate metainfo releases from CHANGELOG.md ---
# CHANGELOG.md is the single source of truth; the <releases> block in the
# metainfo file is derived (dates from git tags). Honors $GITHUB_REF_NAME,
# so pre-release builds embed the pre-release version transiently.
regenerate_metainfo() {
    python3 "$PROJECT_DIR/scripts/generate-metainfo.py"
}

# --- Install application files ---
install_app() {
    local install_root="${1:-$BUILD_DIR/artifacts/app}"
    local bin_dir="$install_root/bin"
    local share_dir="$install_root/share"

    mkdir -p "$bin_dir" "$share_dir"

    local cmake_build="$BUILD_DIR/cmake-build"
    local binary_dir="$cmake_build/$RUST_BUILD_TYPE"
    if [ -f "$binary_dir/rhesis" ]; then
        cp "$binary_dir/rhesis" "$bin_dir/"
    elif [ -f "$cmake_build/rhesis" ]; then
        cp "$cmake_build/rhesis" "$bin_dir/"
    else
        echo "Error: Binary not found in $binary_dir or $cmake_build" >&2
        return 1
    fi

    local translations_installed=false
    local trans_dir="$share_dir/rhesis/translations"
    # Fresh .qm files regenerated next to the .ts sources by
    # regenerate_translations() take precedence over anything cached.
    if compgen -G "$PROJECT_DIR/translations/*.qm" > /dev/null; then
        mkdir -p "$trans_dir"
        cp "$PROJECT_DIR/translations/"*.qm "$trans_dir/" && translations_installed=true
    fi
    # Fall back to cmake-built outputs (e.g. when linguist tools are missing).
    if [ "$translations_installed" = false ]; then
        local qm_dir
        for qm_dir in "$binary_dir/translations" "$cmake_build/translations" "$cmake_build"; do
            if compgen -G "$qm_dir/*.qm" > /dev/null; then
                mkdir -p "$trans_dir"
                cp "$qm_dir/"*.qm "$trans_dir/" && translations_installed=true
                break
            fi
        done
    fi
    if [ "$translations_installed" = false ]; then
        echo "Warning: no translation (.qm) files found; shipping without translations" >&2
    fi

    mkdir -p "$share_dir/applications"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.desktop" "$share_dir/applications/"
    mkdir -p "$share_dir/icons/hicolor/256x256/apps"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.png" "$share_dir/icons/hicolor/256x256/apps/"
    mkdir -p "$share_dir/metainfo"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.metainfo.xml" "$share_dir/metainfo/"
    mkdir -p "$share_dir/licenses/rhesis"
    cp "$PROJECT_DIR/License.md" "$share_dir/licenses/rhesis/"
}

# --- Setup LanguageTool ---
setup_languagetool() {
    local lt_dir="$BUILD_DIR/artifacts/LanguageTool"

    if [ -f "$lt_dir/languagetool-server.jar" ]; then
        return
    fi

    rm -rf "$lt_dir"
    mkdir -p "$lt_dir"

    local lt_archive="$BUILD_DIR/LanguageTool-6.6.zip"
    if [ ! -f "$lt_archive" ]; then
        wget -q -O "$lt_archive" "https://languagetool.org/download/LanguageTool-6.6.zip"
    fi

    unzip -q -o "$lt_archive" -d "$lt_dir"
    mv "$lt_dir/LanguageTool-6.6/"* "$lt_dir/"
    rmdir "$lt_dir/LanguageTool-6.6" 2>/dev/null || true

    rm -f "$lt_dir/languagetool.jar" "$lt_dir/languagetool-commandline.jar"
    rm -f "$lt_dir/libs/languagetool-core-tests.jar"
    rm -f "$lt_dir/libs/junit.jar" "$lt_dir/libs/hamcrest-core.jar"
    rm -rf "$lt_dir/META-INF/maven"
    rm -f "$lt_dir/CHANGES.md" "$lt_dir/CHANGES.txt" "$lt_dir/README.md"

    local fasttext_dir="$BUILD_DIR/artifacts/fastText"
    mkdir -p "$fasttext_dir"
    cp "$BUILD_DIR/fasttext-src/fasttext" "$fasttext_dir/fasttext"
}

# --- Download fastText language model ---
setup_lid_model() {
    local lid_artifact="$BUILD_DIR/artifacts/lid.176.ftz"

    if [ -f "$lid_artifact" ]; then
        return
    fi

    local lid_model="$BUILD_DIR/lid.176.ftz"
    if [ ! -f "$lid_model" ]; then
        wget -q -O "$lid_model" "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz"
    fi
    mkdir -p "$BUILD_DIR/artifacts"
    cp "$lid_model" "$lid_artifact"
}

# --- Create trimmed JRE using jlink ---
create_trimmed_jre() {
    local install_root="${1:-$BUILD_DIR/artifacts/java}"
    local jre_dir="$install_root/jre"

    if [ -d "$jre_dir" ]; then
        return
    fi

    mkdir -p "$(dirname "$jre_dir")"

    local jdk_dir=""
    if [ -n "${JAVA_HOME:-}" ]; then
        jdk_dir="$JAVA_HOME"
    elif command -v java &>/dev/null; then
        jdk_dir="$(dirname "$(dirname "$(readlink -f "$(which java)")")")"
    fi

    if [ -z "$jdk_dir" ] || [ ! -d "$jdk_dir" ]; then
        echo "Error: JDK not found. Please install OpenJDK 17+ or set JAVA_HOME" >&2
        return 1
    fi

    if [ ! -x "$jdk_dir/bin/jlink" ]; then
        echo "Error: jlink not found at $jdk_dir/bin/jlink" >&2
        return 1
    fi

    "$jdk_dir/bin/jlink" \
        --module-path "$jdk_dir/jmods" \
        --add-modules java.base,java.logging,java.xml,java.naming,java.management,java.sql,jdk.httpserver,jdk.unsupported,java.desktop,java.net.http,java.scripting,java.compiler,java.prefs,java.rmi,java.security.jgss,java.security.sasl,java.instrument \
        --output "$jre_dir" \
        --strip-debug \
        --compress=2

    ln -sf "jre/bin/java" "$install_root/java"
}

# Run main function if script is executed directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
