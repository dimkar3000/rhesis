#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# --- Configuration ---
RUST_BUILD_TYPE="${RUST_BUILD_TYPE:-release}"
INSTALL_PREFIX="${INSTALL_PREFIX:-/usr}"
BUILD_DIR="${BUILD_DIR:-$PROJECT_DIR/build}"

# --- Parse arguments ---
INCLUDE_JAVA=false
INCLUDE_LANGUAGETOOL=false
SKIP_RUST_BUILD=false
SKIP_FASTTEXT_BUILD=false
INSTALL_ROOT=""

parse_common_args() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            --include-java)
                INCLUDE_JAVA=true
                shift
                ;;
            --include-languagetool)
                INCLUDE_LANGUAGETOOL=true
                shift
                ;;
            --skip-rust-build)
                SKIP_RUST_BUILD=true
                shift
                ;;
            --skip-fasttext-build)
                SKIP_FASTTEXT_BUILD=true
                shift
                ;;
            --install-root)
                INSTALL_ROOT="$2"
                shift 2
                ;;
            *)
                echo "Unknown option: $1"
                exit 1
                ;;
        esac
    done
}

# --- Build Rust application ---
build_rust_app() {
    if [ "$SKIP_RUST_BUILD" = true ]; then
        echo "Skipping Rust build (--skip-rust-build)"
        return
    fi

    echo "Building application via CMake..."
    cd "$PROJECT_DIR"

    mkdir -p "$BUILD_DIR/cmake-build"
    cd "$BUILD_DIR/cmake-build"

    cmake -DCMAKE_INSTALL_PREFIX="$INSTALL_PREFIX" "$PROJECT_DIR"
    make -j$(nproc)

    cd "$PROJECT_DIR"
}

# --- Build fastText from source ---
build_fasttext() {
    if [ "$SKIP_FASTTEXT_BUILD" = true ]; then
        echo "Skipping fastText build (--skip-fasttext-build)"
        return
    fi

    echo "Building fastText..."
    cd "$PROJECT_DIR"

    if [ ! -d "fasttext-src" ]; then
        echo "Cloning fastText repository..."
        git clone --depth 1 --branch v0.9.2 https://github.com/facebookresearch/fastText.git fasttext-src
    fi

    cd fasttext-src
    make -j$(nproc) CXXFLAGS="-pthread -std=c++17 -march=native -include cstdint"
    cd "$PROJECT_DIR"
}

# --- Install application files ---
install_app() {
    local install_root="${1:-$BUILD_DIR/install-root}"
    local bin_dir="$install_root$INSTALL_PREFIX/bin"
    local share_dir="$install_root$INSTALL_PREFIX/share"

    echo "Installing application to $install_root..."
    mkdir -p "$bin_dir" "$share_dir"

    # Install binary from CMake build directory
    local cmake_build="$BUILD_DIR/cmake-build"
    if [ -f "$cmake_build/release/rhesis" ]; then
        cp "$cmake_build/release/rhesis" "$bin_dir/"
    elif [ -f "$cmake_build/rhesis" ]; then
        cp "$cmake_build/rhesis" "$bin_dir/"
    elif [ -f "$PROJECT_DIR/build/target/release/rhesis" ]; then
        cp "$PROJECT_DIR/build/target/release/rhesis" "$bin_dir/"
    else
        echo "Error: Binary not found in $cmake_build or build/target/release"
        exit 1
    fi

    # Install translations if they exist
    if [ -d "$cmake_build/release/translations" ]; then
        mkdir -p "$bin_dir/translations"
        cp "$cmake_build/release/translations/"*.qm "$bin_dir/translations/" 2>/dev/null || true
    elif [ -d "$cmake_build/translations" ]; then
        mkdir -p "$bin_dir/translations"
        cp "$PROJECT_DIR/build/target/release/translations/"*.qm "$bin_dir/translations/" 2>/dev/null || true
    fi

    # Install desktop file
    mkdir -p "$share_dir/applications"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.desktop" "$share_dir/applications/"

    # Install icon
    mkdir -p "$share_dir/icons/hicolor/256x256/apps"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.png" "$share_dir/icons/hicolor/256x256/apps/"

    # Install metainfo
    mkdir -p "$share_dir/metainfo"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.metainfo.xml" "$share_dir/metainfo/"

    # Install license
    mkdir -p "$share_dir/licenses/rhesis"
    cp "$PROJECT_DIR/License.md" "$share_dir/licenses/rhesis/"
}

# --- Setup LanguageTool ---
setup_languagetool() {
    local install_root="${1:-$BUILD_DIR/install-root}"
    local lt_dir="$install_root/usr/share/languagetool"

    if [ "$INCLUDE_LANGUAGETOOL" = false ]; then
        echo "Skipping LanguageTool setup (--include-languagetool not set)"
        return
    fi

    echo "Setting up LanguageTool..."
    mkdir -p "$lt_dir"

    # Download LanguageTool if not present
    local lt_archive="$BUILD_DIR/LanguageTool-6.6.zip"
    if [ ! -f "$lt_archive" ]; then
        echo "Downloading LanguageTool 6.6..."
        wget -q -O "$lt_archive" "https://languagetool.org/download/LanguageTool-6.6.zip"
    fi

    # Extract LanguageTool
    unzip -q -o "$lt_archive" -d "$lt_dir"
    mv "$lt_dir/LanguageTool-6.6/"* "$lt_dir/"
    rmdir "$lt_dir/LanguageTool-6.6" 2>/dev/null || true

    # Clean up unnecessary files
    rm -f "$lt_dir/languagetool.jar" "$lt_dir/languagetool-commandline.jar"
    rm -f "$lt_dir/libs/languagetool-core-tests.jar"
    rm -f "$lt_dir/libs/junit.jar" "$lt_dir/libs/hamcrest-core.jar"
    rm -rf "$lt_dir/META-INF/maven"
    rm -f "$lt_dir/CHANGES.md" "$lt_dir/CHANGES.txt" "$lt_dir/README.md"

    # Configure LanguageTool
    cat > "$lt_dir/server.properties" << EOF
fasttextModel=../lid.176.ftz
fasttextBinary=../fastText/fasttext
EOF

    # Install fastText binary
    mkdir -p "$install_root/usr/lib/fastText"
    cp "$PROJECT_DIR/fasttext-src/fasttext" "$install_root/usr/lib/fastText/"

    # Download language identification model
    local lid_model="$BUILD_DIR/lid.176.ftz"
    if [ ! -f "$lid_model" ]; then
        echo "Downloading language identification model..."
        wget -q -O "$lid_model" "https://dl.fbaipublicfiles.com/fasttext/supervised-models/lid.176.ftz"
    fi
    cp "$lid_model" "$install_root/usr/share/"
}

# --- Create trimmed JRE using jlink ---
create_trimmed_jre() {
    local install_root="${1:-$BUILD_DIR/install-root}"
    local jre_dir="$install_root/usr/lib/jre"

    if [ "$INCLUDE_JAVA" = false ]; then
        echo "Skipping JRE creation (--include-java not set)"
        return
    fi

    echo "Creating trimmed JRE..."
    rm -rf "$jre_dir"
    mkdir -p "$(dirname "$jre_dir")"

    # Find JDK
    local jdk_dir=""
    if [ -n "${JAVA_HOME:-}" ]; then
        jdk_dir="$JAVA_HOME"
    elif command -v java &>/dev/null; then
        jdk_dir="$(dirname "$(dirname "$(readlink -f "$(which java)")")")"
    fi

    if [ -z "$jdk_dir" ] || [ ! -d "$jdk_dir" ]; then
        echo "Error: JDK not found. Please install OpenJDK 17+ or set JAVA_HOME"
        exit 1
    fi

    if [ ! -x "$jdk_dir/bin/jlink" ]; then
        echo "Error: jlink not found at $jdk_dir/bin/jlink"
        echo "Please install a full JDK (not just JRE) with jlink support."
        echo "On Ubuntu/Debian: sudo apt install openjdk-17-jdk or openjdk-21-jdk"
        exit 1
    fi

    # Create trimmed JRE
    "$jdk_dir/bin/jlink" \
        --module-path "$jdk_dir/jmods" \
        --add-modules java.base,java.logging,java.xml,java.naming,java.management,java.sql,jdk.httpserver,jdk.unsupported,java.desktop,java.net.http,java.scripting,java.compiler,java.prefs,java.rmi,java.security.jgss,java.security.sasl,java.instrument \
        --output "$jre_dir" \
        --strip-debug \
        --compress=2
}

# --- Main function ---
main() {
    parse_common_args "$@"

    local install_root="${INSTALL_ROOT:-$BUILD_DIR/install-root}"

    echo "=== Common Build Script ==="
    echo "Install prefix: $INSTALL_PREFIX"
    echo "Build type: $RUST_BUILD_TYPE"
    echo "Include Java: $INCLUDE_JAVA"
    echo "Include LanguageTool: $INCLUDE_LANGUAGETOOL"
    echo "Install root: $install_root"
    echo ""

    # Build components
    build_rust_app
    build_fasttext
    install_app "$install_root"
    setup_languagetool "$install_root"
    create_trimmed_jre "$install_root"

    echo ""
    echo "=== Build Complete ==="
    echo "Installed to: $install_root"
}

# Run main function if script is executed directly
if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
    main "$@"
fi
