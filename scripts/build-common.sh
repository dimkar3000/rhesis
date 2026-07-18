#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# --- Configuration ---
RUST_BUILD_TYPE="${RUST_BUILD_TYPE:-release}"
INSTALL_PREFIX="${INSTALL_PREFIX:-}"
BUILD_DIR="${BUILD_DIR:-$PROJECT_DIR/build}"

# --- Parse arguments ---
CLEAN_BUILD=false

usage() {
    echo "Usage: $(basename "$0") [OPTIONS]"
    echo ""
    echo "Builds the Rust application, fastText, LanguageTool, and a trimmed JRE."
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

# --- Main ---
main() {
    local install_root="$BUILD_DIR/artifacts/app"

    echo "=== Common Build Script ==="
    echo "Build type: $RUST_BUILD_TYPE"
    echo "Install root: $install_root"
    echo ""

    if [ "$CLEAN_BUILD" = true ]; then
        echo "Cleaning build directory..."
        rm -rf "$BUILD_DIR"
    fi

    step "Building application" build_rust_app
    step "Building fastText" build_fasttext
    step "Installing application files" install_app "$install_root"
    step "Setting up LanguageTool" setup_languagetool "$install_root"

    local java_install_root="$BUILD_DIR/artifacts/java"
    step "Creating trimmed JRE" create_trimmed_jre "$java_install_root"

    echo ""
    echo "=== Build Complete ==="
    echo "Installed to: $install_root"
}

build_rust_app() {
    local binary=""
    if [ -f "$BUILD_DIR/cmake-build/release/rhesis" ]; then
        binary="$BUILD_DIR/cmake-build/release/rhesis"
    elif [ -f "$BUILD_DIR/cmake-build/rhesis" ]; then
        binary="$BUILD_DIR/cmake-build/rhesis"
    elif [ -f "$PROJECT_DIR/build/target/release/rhesis" ]; then
        binary="$PROJECT_DIR/build/target/release/rhesis"
    fi

    if [ -n "$binary" ]; then
        return
    fi

    cd "$PROJECT_DIR"
    mkdir -p "$BUILD_DIR/cmake-build"
    cd "$BUILD_DIR/cmake-build"

    if [ -n "${SDK:-}" ] && [ -d "$SDK/lib/x86_64-linux-gnu" ]; then
        export LD_LIBRARY_PATH="${SDK}/lib/x86_64-linux-gnu${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
    fi

    cmake -DCMAKE_INSTALL_PREFIX="$INSTALL_PREFIX" "$PROJECT_DIR"
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

# --- Install application files ---
install_app() {
    local install_root="${1:-$BUILD_DIR/artifacts/app}"
    local bin_dir="$install_root"
    local share_dir="$install_root/share"

    mkdir -p "$bin_dir" "$share_dir"

    local cmake_build="$BUILD_DIR/cmake-build"
    if [ -f "$cmake_build/release/rhesis" ]; then
        cp "$cmake_build/release/rhesis" "$bin_dir/"
    elif [ -f "$cmake_build/rhesis" ]; then
        cp "$cmake_build/rhesis" "$bin_dir/"
    elif [ -f "$PROJECT_DIR/build/target/release/rhesis" ]; then
        cp "$PROJECT_DIR/build/target/release/rhesis" "$bin_dir/"
    else
        echo "Error: Binary not found in $cmake_build or build/target/release" >&2
        return 1
    fi

    local translations_installed=false
    if [ -d "$cmake_build/release/translations" ]; then
        mkdir -p "$bin_dir/translations"
        cp "$cmake_build/release/translations/"*.qm "$bin_dir/translations/" 2>/dev/null && translations_installed=true
    elif [ -d "$cmake_build/translations" ]; then
        mkdir -p "$bin_dir/translations"
        cp "$cmake_build/translations/"*.qm "$bin_dir/translations/" 2>/dev/null && translations_installed=true
    fi
    if [ "$translations_installed" = false ] && [ -d "$PROJECT_DIR/translations" ]; then
        mkdir -p "$bin_dir/translations"
        local lrelease_cmd="$(command -v lrelease6 || command -v lrelease || echo "")"
        if [ -n "$lrelease_cmd" ]; then
            for ts_file in "$PROJECT_DIR"/translations/*.ts; do
                [ -f "$ts_file" ] || continue
                "$lrelease_cmd" -silent "$ts_file" -qm "$bin_dir/translations/$(basename "${ts_file%.ts}.qm")"
            done
            translations_installed=true
        else
            cp "$PROJECT_DIR/translations/"*.qm "$bin_dir/translations/" 2>/dev/null || true
        fi
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
    local install_root="${1:-$BUILD_DIR/artifacts/app}"
    local lt_dir="$install_root/LanguageTool"

    if [ -f "$lt_dir/server.properties" ]; then
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

    cat > "$lt_dir/server.properties" << EOF
fasttextModel=../lid.176.ftz
fasttextBinary=../fasttext
EOF

    local fasttext_dir="$BUILD_DIR/artifacts/fastText"
    mkdir -p "$fasttext_dir"
    cp "$BUILD_DIR/fasttext-src/fasttext" "$fasttext_dir/fasttext"

    local lid_model="$BUILD_DIR/lid.176.ftz"
    if [ ! -f "$lid_model" ]; then
        wget -q -O "$lid_model" "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz"
    fi
    cp "$lid_model" "$install_root/"
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
