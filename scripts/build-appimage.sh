#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

BUILD_DIR="$PROJECT_DIR/build"
APPDIR="$BUILD_DIR/appimage/AppDir"
TOOLS_DIR="$BUILD_DIR/tools/appimage"
ARTIFACTS_DIR="$BUILD_DIR/artifacts"
VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

LINUXDEPLOY="$TOOLS_DIR/linuxdeploy-x86_64.AppImage"
APPIMAGETOOL="$TOOLS_DIR/appimagetool-x86_64.AppImage"

# --- Parse arguments ---
CLEAN_BUILD=false

source "$SCRIPT_DIR/common.sh"

while [[ $# -gt 0 ]]; do
    case $1 in
        --clean) CLEAN_BUILD=true; shift ;;
        --verbose) VERBOSE=true; shift ;;
        --no-spinner) NO_SPINNER=true; shift ;;
        --linuxdeploy) LINUXDEPLOY="$2"; shift 2 ;;
        --appimagetool) APPIMAGETOOL="$2"; shift 2 ;;
        --help)
            echo "Usage: $(basename "$0") [OPTIONS]"
            echo ""
            echo "Builds an AppImage from pre-built artifacts."
            echo "Run build-common.sh first to produce artifacts."
            echo ""
            echo "Options:"
            echo "  --clean              Clean and rebuild artifacts from scratch, then build the AppImage"
            echo "  --verbose            Show full command output (default: quiet)"
            echo "  --no-spinner         Disable spinner animation (plain output)"
            echo "  --linuxdeploy PATH   Use pre-installed linuxdeploy AppImage at PATH"
            echo "  --appimagetool PATH  Use pre-installed appimagetool AppImage at PATH"
            echo "  --help               Show this help message and exit"
            exit 0
            ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done

download_tool() {
    local url="$1" path="$2"
    if [ ! -f "$path" ]; then
        mkdir -p "$(dirname "$path")"
        wget -q -O "$path" "$url"
        chmod +x "$path"
    fi
}

create_appdir() {
    rm -rf "$APPDIR"
    mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/lib"

    cp "$ARTIFACTS_DIR/app/rhesis" "$APPDIR/usr/bin/"
    cp -r "$ARTIFACTS_DIR/app/translations" "$APPDIR/usr/" 2>/dev/null || true
    cp -r "$ARTIFACTS_DIR/app/share" "$APPDIR/usr/"

    cp -r "$ARTIFACTS_DIR/app/LanguageTool" "$APPDIR/"
    cp "$ARTIFACTS_DIR/app/lid.176.ftz" "$APPDIR/"
    cp -r "$ARTIFACTS_DIR/java/jre" "$APPDIR/"
    cp "$ARTIFACTS_DIR/fastText/fasttext" "$APPDIR/fasttext"
    ln -sf "../../jre/bin/java" "$APPDIR/usr/bin/java"

    ln -sf usr/share/applications/io.github.dimkar3000.rhesis.desktop "$APPDIR/"
    ln -sf usr/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png "$APPDIR/"
}

bundle_qt() {
    local qml_src=""
    if command -v qmake6 &>/dev/null; then
        qml_src=$(qmake6 -query QT_INSTALL_QML 2>/dev/null || true)
    elif command -v qmake &>/dev/null; then
        qml_src=$(qmake -query QT_INSTALL_QML 2>/dev/null || true)
    fi
    if [ -z "$qml_src" ] || [ ! -d "$qml_src" ]; then
        qml_src=""
        for d in /usr/lib/qt6/qml /usr/lib64/qt6/qml \
                 /usr/lib/qml \
                 "${SDK:-}/lib/x86_64-linux-gnu/qt6/qml" \
                 "${SDK:-}/lib/qml"; do
            [ -d "$d" ] && { qml_src="$d"; break; }
        done
    fi

    if [ -n "$qml_src" ]; then
        mkdir -p "$APPDIR/usr/lib/qt6/qml"
        cp -r "$qml_src/"Qt* "$qml_src/org" "$APPDIR/usr/lib/qt6/qml/" 2>/dev/null || true
    fi

    export PATH="$TOOLS_DIR:$PATH"
    export QMAKE="$(command -v qmake6 || command -v qmake || true)"
    # linuxdeploy's bundled strip doesn't understand modern ELF sections (.relr.dyn)
    NO_STRIP=1 "$LINUXDEPLOY" --appdir "$APPDIR" \
        --executable "$APPDIR/usr/bin/rhesis" \
        --desktop-file "$APPDIR/usr/share/applications/io.github.dimkar3000.rhesis.desktop" \
        --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png"

    local qt_plugin_dir=""
    if command -v qmake6 &>/dev/null; then
        qt_plugin_dir=$(qmake6 -query QT_INSTALL_PLUGINS 2>/dev/null || true)
    elif command -v qmake &>/dev/null; then
        qt_plugin_dir=$(qmake -query QT_INSTALL_PLUGINS 2>/dev/null || true)
    fi
    if [ -z "$qt_plugin_dir" ] || [ ! -d "$qt_plugin_dir" ]; then
        qt_plugin_dir=""
        for d in /usr/lib/qt6/plugins /usr/lib64/qt6/plugins \
                 /usr/lib/x86_64-linux-gnu/qt6/plugins \
                 /usr/lib/plugins \
                 "${SDK:-}/lib/x86_64-linux-gnu/qt6/plugins" \
                 "${SDK:-}/lib/plugins"; do
            [ -d "$d" ] && { qt_plugin_dir="$d"; break; }
        done
    fi

    if [ -n "$qt_plugin_dir" ]; then
        mkdir -p "$APPDIR/usr/lib/qt6/plugins/platforms"
        for plat in libqxcb.so libqwayland*.so; do
            found=$(find "$qt_plugin_dir/platforms" -maxdepth 1 -name "$plat" -type f 2>/dev/null || true)
            [ -n "$found" ] && cp "$found" "$APPDIR/usr/lib/qt6/plugins/platforms/" 2>/dev/null
        done
        for subdir in platforminputcontexts platformthemes xcbglintegrations imageformats tls networkinformation wayland-shell-integration; do
            [ -d "$qt_plugin_dir/$subdir" ] && {
                mkdir -p "$APPDIR/usr/lib/qt6/plugins/$subdir"
                cp "$qt_plugin_dir/$subdir/"*.so "$APPDIR/usr/lib/qt6/plugins/$subdir/" 2>/dev/null || true
            }
        done
    fi
}

write_apprun() {
    cat > "$APPDIR/AppRun" << 'APPRUN'
#!/bin/bash
HERE=$(dirname "$(readlink -f "$0")")
export PATH="${HERE}/usr/bin:${PATH}"
export QT_QPA_PLATFORM="${QT_QPA_PLATFORM:-xcb}"
export QT_PLUGIN_PATH="${HERE}/usr/lib/qt6/plugins"
export QML2_IMPORT_PATH="${HERE}/usr/lib/qt6/qml"
export RHESIS_LANGUAGETOOL_DIR="${HERE}/LanguageTool"
exec "${HERE}/usr/bin/rhesis" "$@"
APPRUN
    chmod +x "$APPDIR/AppRun"
}

create_appimage() {
    mkdir -p "$ARTIFACTS_DIR/appimage"
    ARCH=x86_64 "$APPIMAGETOOL" --no-appstream "$APPDIR" \
        "$ARTIFACTS_DIR/appimage/rhesis-${VERSION}-x86_64.AppImage" \
        || true
}

# --- Main ---
main() {
    COMMON_ARGS=()
    [ "$VERBOSE" = true ] && COMMON_ARGS+=(--verbose)
    [ "$NO_SPINNER" = true ] && COMMON_ARGS+=(--no-spinner)
    [ "$CLEAN_BUILD" = true ] && COMMON_ARGS+=(--clean)

    if [ ! -d "$ARTIFACTS_DIR/app" ] || [ "$CLEAN_BUILD" = true ]; then
        "$SCRIPT_DIR/build-common.sh" "${COMMON_ARGS[@]}"
    fi

    echo "=== AppImage Build ==="
    echo ""

    download_tool "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage" "$LINUXDEPLOY"
    download_tool "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage" "$TOOLS_DIR/linuxdeploy-plugin-qt-x86_64.AppImage"
    download_tool "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage" "$APPIMAGETOOL"

    step "Creating AppDir" create_appdir
    step "Writing AppRun" write_apprun
    step "Bundling Qt and KDE dependencies" bundle_qt
    step "Creating AppImage" create_appimage

    echo ""
    echo "AppImage created: $ARTIFACTS_DIR/appimage/rhesis-${VERSION}-x86_64.AppImage"
}

main "$@"
