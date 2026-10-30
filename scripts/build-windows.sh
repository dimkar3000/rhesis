#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# --- Configuration ---
RUST_BUILD_TYPE="${RUST_BUILD_TYPE:-release-fast}"
BUILD_DIR="${BUILD_DIR:-$PROJECT_DIR/build}"
TARGET_DIR="${CARGO_TARGET_DIR:-$PROJECT_DIR/target}"
ARTIFACTS_DIR="$BUILD_DIR/artifacts/windows"
STAGE_PARENT="$BUILD_DIR/windows-stage"
STAGE_DIR="$STAGE_PARENT/Rhesis"
LANGUAGETOOL_ZIP_URL="${LANGUAGETOOL_ZIP_URL:-https://languagetool.org/download/LanguageTool-6.6.zip}"
LANGUAGETOOL_ZIP_DIR="LanguageTool-6.6"

# Pre-release tags embed the pre-release version in artifact names
# transiently (same rule as scripts/generate-metainfo.py).
VERSION=$(grep -m1 '^version' "$PROJECT_DIR/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')
NUMERIC_VERSION="$VERSION"
case "${GITHUB_REF_NAME:-}" in
    v*-*) VERSION="${GITHUB_REF_NAME#v}" ;;
esac

# --- Parse arguments ---
CLEAN_BUILD=false
BUILD_INSTALLER=true

usage() {
    echo "Usage: $(basename "$0") [OPTIONS]"
    echo ""
    echo "Builds the Windows app and packages it as a portable zip and an"
    echo "Inno Setup installer. Run from Git Bash."
    echo "Requires a Rust toolchain (MSVC), Qt 6.8+ (MSVC kit; QMAKE or qmake on PATH),"
    echo "a JDK 17+ with jlink (JAVA_HOME), and Inno Setup 6 (ISCC.exe) for the installer."
    echo ""
    echo "Environment:"
    echo "  RUST_BUILD_TYPE  Cargo profile: release, release-fast (default) or debug"
    echo ""
    echo "Options:"
    echo "  --no-installer  Only build the zip (skips the Inno Setup step)"
    echo "  --clean         Delete the Windows build artifacts and rebuild from scratch"
    echo "  --verbose       Show full command output (default: quiet)"
    echo "  --no-spinner    Disable spinner animation (plain output)"
    echo "  --help          Show this help message and exit"
    exit 0
}

source "$SCRIPT_DIR/common.sh"

while [[ $# -gt 0 ]]; do
    case $1 in
        --no-installer) BUILD_INSTALLER=false; shift ;;
        --clean) CLEAN_BUILD=true; shift ;;
        --verbose) VERBOSE=true; shift ;;
        --no-spinner) NO_SPINNER=true; shift ;;
        --help) usage ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done

# --- Locate the toolchain up front (steps may run in subshells) ---
find_qt_bin() {
    if [ -n "${QMAKE:-}" ]; then
        dirname "$(cygpath -u "$QMAKE")"
    elif command -v qmake &>/dev/null; then
        dirname "$(command -v qmake)"
    else
        echo "Error: qmake not found; install Qt 6.8+ (MSVC kit) and set QMAKE or add its bin to PATH" >&2
        return 1
    fi
}

find_jdk_dir() {
    if [ -n "${JAVA_HOME:-}" ]; then
        cygpath -u "$JAVA_HOME"
    elif command -v java &>/dev/null; then
        dirname "$(dirname "$(command -v java)")"
    else
        echo "Error: JDK not found. Install JDK 17+ or set JAVA_HOME" >&2
        return 1
    fi
}

find_iscc() {
    local candidate
    if command -v ISCC.exe &>/dev/null; then
        command -v ISCC.exe
        return 0
    fi
    for candidate in \
        "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" \
        "/c/Program Files/Inno Setup 6/ISCC.exe" \
        "${LOCALAPPDATA:+$(cygpath -u "$LOCALAPPDATA")/Programs/Inno Setup 6/ISCC.exe}"; do
        if [ -n "$candidate" ] && [ -x "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    echo "Error: Inno Setup 6 (ISCC.exe) not found; install it or pass --no-installer" >&2
    return 1
}

QT_BIN="$(find_qt_bin)"
JDK_DIR="$(find_jdk_dir)"
export QMAKE="${QMAKE:-$(cygpath -w "$QT_BIN")\\qmake.exe}"

case "$RUST_BUILD_TYPE" in
    debug|dev) CARGO_PROFILE=dev; PROFILE_DIR=debug ;;
    *) CARGO_PROFILE="$RUST_BUILD_TYPE"; PROFILE_DIR="$RUST_BUILD_TYPE" ;;
esac

# Windows-native tools (tar.exe, powershell, ISCC) need native paths.
winpath() { cygpath -w "$1"; }

# --- Compile the translation catalogs ---
# Never runs lupdate: like CI on Linux (SKIP_LUPDATE), packaging compiles the
# committed .ts sources and must not mutate them.
build_translations() {
    cd "$PROJECT_DIR"
    rm -f translations/*.qm
    "$QT_BIN/lrelease.exe" translations/*.ts

    local ts qm
    for ts in translations/*.ts; do
        qm="${ts%.ts}.qm"
        if grep -q "<message>" "$ts" && [ "$(wc -c < "$qm")" -le 64 ]; then
            echo "ERROR: $qm is a stub catalog; refusing to ship it" >&2
            return 1
        fi
    done
}

build_rust_app() {
    cd "$PROJECT_DIR"
    # Only the shipped binary: develop also builds rhesis-ui-runner for the tests.
    cargo build --profile "$CARGO_PROFILE" --bin rhesis
}

# --- Download and trim LanguageTool (same trimming as build-common.sh) ---
setup_languagetool() {
    local lt_dir="$ARTIFACTS_DIR/LanguageTool"

    if [ -f "$lt_dir/languagetool-server.jar" ]; then
        return
    fi

    local archive="$BUILD_DIR/$LANGUAGETOOL_ZIP_DIR.zip"
    if [ ! -f "$archive" ]; then
        mkdir -p "$BUILD_DIR"
        curl -fsSLo "$archive" "$LANGUAGETOOL_ZIP_URL"
    fi

    rm -rf "$lt_dir" "$ARTIFACTS_DIR/lt-extract"
    mkdir -p "$ARTIFACTS_DIR/lt-extract"
    powershell.exe -NoProfile -Command \
        "Expand-Archive -LiteralPath '$(winpath "$archive")' -DestinationPath '$(winpath "$ARTIFACTS_DIR/lt-extract")' -Force"
    mv "$ARTIFACTS_DIR/lt-extract/$LANGUAGETOOL_ZIP_DIR" "$lt_dir"
    rm -rf "$ARTIFACTS_DIR/lt-extract"

    rm -f "$lt_dir/languagetool.jar" "$lt_dir/languagetool-commandline.jar"
    rm -f "$lt_dir/libs/languagetool-core-tests.jar"
    rm -f "$lt_dir/libs/junit.jar" "$lt_dir/libs/hamcrest-core.jar"
    rm -rf "$lt_dir/META-INF/maven"
    rm -f "$lt_dir/CHANGES.md" "$lt_dir/CHANGES.txt" "$lt_dir/README.md"
}

# --- Create trimmed JRE using jlink (same module set as build-common.sh) ---
create_trimmed_jre() {
    local jre_dir="$ARTIFACTS_DIR/jre"

    if [ -d "$jre_dir" ]; then
        return
    fi

    if [ ! -x "$JDK_DIR/bin/jlink.exe" ]; then
        echo "Error: jlink not found at $JDK_DIR/bin/jlink.exe (a full JDK is required)" >&2
        return 1
    fi

    mkdir -p "$ARTIFACTS_DIR"
    "$JDK_DIR/bin/jlink.exe" \
        --module-path "$(winpath "$JDK_DIR/jmods")" \
        --add-modules java.base,java.logging,java.xml,java.naming,java.management,java.sql,jdk.httpserver,jdk.unsupported,java.desktop,java.net.http,java.scripting,java.compiler,java.prefs,java.rmi,java.security.jgss,java.security.sasl,java.instrument \
        --output "$(winpath "$jre_dir")" \
        --strip-debug \
        --compress=2
}

# --- Wrap the 256px PNG as a .ico (Vista+ ICO files may embed PNG data) ---
create_icon() {
    python - "$PROJECT_DIR/io.github.dimkar3000.rhesis.png" "$STAGE_DIR/rhesis.ico" <<'EOF'
import struct, sys

png = open(sys.argv[1], "rb").read()
width, height = struct.unpack(">II", png[16:24])
size = lambda v: 0 if v >= 256 else v
header = struct.pack("<HHH", 0, 1, 1)
entry = struct.pack("<BBBBHHII", size(width), size(height), 0, 0, 1, 32, len(png), 22)
open(sys.argv[2], "wb").write(header + entry + png)
EOF
}

# --- Assemble the install layout used by both the zip and the installer ---
#   Rhesis/rhesis.exe + Qt runtime, translations/, LanguageTool/, jre/
stage_payload() {
    rm -rf "$STAGE_PARENT"
    mkdir -p "$STAGE_DIR/translations"

    local binary="$TARGET_DIR/$PROFILE_DIR/rhesis.exe"
    if [ ! -f "$binary" ]; then
        echo "Error: binary not found at $binary" >&2
        return 1
    fi
    cp "$binary" "$STAGE_DIR/"

    "$QT_BIN/windeployqt.exe" \
        --qmldir "$(winpath "$PROJECT_DIR/src/interop/qml/windows")" \
        --no-translations \
        --no-system-d3d-compiler \
        --no-opengl-sw \
        "$(winpath "$STAGE_DIR/rhesis.exe")"

    cp "$PROJECT_DIR/translations/"*.qm "$STAGE_DIR/translations/"
    cp -r "$ARTIFACTS_DIR/LanguageTool" "$STAGE_DIR/LanguageTool"
    cp -r "$ARTIFACTS_DIR/jre" "$STAGE_DIR/jre"
    cp "$PROJECT_DIR/License.md" "$STAGE_DIR/"
    create_icon
}

build_zip() {
    local zip_path="$ARTIFACTS_DIR/rhesis-$VERSION-windows-x86_64.zip"
    rm -f "$zip_path"
    # bsdtar ships with Windows 10+ and writes zip with -a.
    "$(cygpath -u "${SYSTEMROOT:-C:\\Windows}")/System32/tar.exe" \
        -a -c -f "$(winpath "$zip_path")" -C "$(winpath "$STAGE_PARENT")" Rhesis
}

build_installer() {
    local iscc
    iscc="$(find_iscc)"
    MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL="*" "$iscc" \
        "/DAppVersion=$VERSION" \
        "/DNumericVersion=$NUMERIC_VERSION" \
        "/DSourceDir=$(winpath "$STAGE_DIR")" \
        "/DOutputDir=$(winpath "$ARTIFACTS_DIR")" \
        "/DOutputBaseFilename=rhesis-$VERSION-windows-x86_64-setup" \
        "$(winpath "$PROJECT_DIR/io.github.dimkar3000.rhesis.iss")"
}

# --- Main ---
main() {
    echo "=== Windows Build Script ==="
    echo "Version: $VERSION"
    echo "Cargo profile: $CARGO_PROFILE"
    echo "Qt: $QT_BIN"
    echo "JDK: $JDK_DIR"
    echo ""

    if [ "$CLEAN_BUILD" = true ]; then
        echo "Cleaning Windows build artifacts..."
        rm -rf "$ARTIFACTS_DIR" "$STAGE_PARENT"
    fi
    mkdir -p "$ARTIFACTS_DIR"

    step "Compiling translations" build_translations
    step "Building application" build_rust_app
    step "Setting up LanguageTool" setup_languagetool
    step "Creating trimmed JRE" create_trimmed_jre
    step "Staging application files" stage_payload
    step "Building portable zip" build_zip
    if [ "$BUILD_INSTALLER" = true ]; then
        step "Building installer" build_installer
    fi

    echo ""
    echo "=== Build Complete ==="
    ls -lh "$ARTIFACTS_DIR" | grep -E "\.(zip|exe)$" || true
}

main "$@"
