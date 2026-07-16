#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# --- Configuration ---
BUILD_DIR="${BUILD_DIR:-$PROJECT_DIR/build}"
APPIMAGE_DIR="$BUILD_DIR/appimage"
APPDIR="$APPIMAGE_DIR/AppDir"
TOOLS_DIR="$BUILD_DIR/tools/appimage"

# Version from Cargo.toml
VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')

# --- Parse arguments ---
VARIANT="full-bundle"
LINUXDEPLOY_PATH=""
SKIP_BUILD=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --full-bundle)
            VARIANT="full-bundle"
            shift
            ;;
        --minimal)
            VARIANT="minimal"
            shift
            ;;
        --linuxdeploy)
            LINUXDEPLOY_PATH="$2"
            shift 2
            ;;
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        *)
            echo "Unknown option: $1"
            echo "Usage: $0 [--full-bundle|--minimal] [--linuxdeploy /path/to/linuxdeploy] [--skip-build]"
            exit 1
            ;;
    esac
done

# --- Resolve linuxdeploy ---
resolve_linuxdeploy() {
    if [ -n "$LINUXDEPLOY_PATH" ]; then
        if [ ! -f "$LINUXDEPLOY_PATH" ]; then
            echo "Error: linuxdeploy not found at $LINUXDEPLOY_PATH"
            exit 1
        fi
        echo "Using linuxdeploy from: $LINUXDEPLOY_PATH"
        return
    fi

    if [ -n "${LINUXDEPLOY:-}" ] && [ -f "$LINUXDEPLOY" ]; then
        LINUXDEPLOY_PATH="$LINUXDEPLOY"
        echo "Using linuxdeploy from LINUXDEPLOY env: $LINUXDEPLOY_PATH"
        return
    fi

    local tool_path="$TOOLS_DIR/linuxdeploy-x86_64.AppImage"
    if [ -f "$tool_path" ]; then
        LINUXDEPLOY_PATH="$tool_path"
        echo "Using linuxdeploy from: $LINUXDEPLOY_PATH"
        return
    fi

    # Auto-download
    echo "linuxdeploy not found. Downloading to $TOOLS_DIR..."
    mkdir -p "$TOOLS_DIR"

    wget -q -O "$TOOLS_DIR/linuxdeploy-x86_64.AppImage" \
        "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage"
    chmod +x "$TOOLS_DIR/linuxdeploy-x86_64.AppImage"

    wget -q -O "$TOOLS_DIR/linuxdeploy-plugin-qt-x86_64.AppImage" \
        "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage"
    chmod +x "$TOOLS_DIR/linuxdeploy-plugin-qt-x86_64.AppImage"

    wget -q -O "$TOOLS_DIR/appimagetool-x86_64.AppImage" \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$TOOLS_DIR/appimagetool-x86_64.AppImage"

    LINUXDEPLOY_PATH="$TOOLS_DIR/linuxdeploy-x86_64.AppImage"
    echo "Downloaded linuxdeploy to: $LINUXDEPLOY_PATH"
}

# --- Resolve linuxdeploy-plugin-qt ---
resolve_linuxdeploy_qt() {
    local linuxdeploy_dir
    linuxdeploy_dir="$(dirname "$LINUXDEPLOY_PATH")"
    local plugin_path="$linuxdeploy_dir/linuxdeploy-plugin-qt-x86_64.AppImage"

    if [ -f "$plugin_path" ]; then
        echo "Using linuxdeploy-plugin-qt from: $plugin_path"
        return
    fi

    if [ -n "${LINUXDEPLOY_QT:-}" ] && [ -f "$LINUXDEPLOY_QT" ]; then
        echo "Using linuxdeploy-plugin-qt from LINUXDEPLOY_QT env: $LINUXDEPLOY_QT"
        return
    fi

    echo "linuxdeploy-plugin-qt not found. Downloading..."
    wget -q -O "$plugin_path" \
        "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage"
    chmod +x "$plugin_path"
    echo "Downloaded linuxdeploy-plugin-qt to: $plugin_path"
}

# --- Resolve appimagetool ---
resolve_appimagetool() {
    local linuxdeploy_dir
    linuxdeploy_dir="$(dirname "$LINUXDEPLOY_PATH")"
    APPIMAGETOOL_PATH="$linuxdeploy_dir/appimagetool-x86_64.AppImage"

    if [ -f "$APPIMAGETOOL_PATH" ]; then
        echo "Using appimagetool from: $APPIMAGETOOL_PATH"
        return
    fi

    echo "appimagetool not found. Downloading..."
    wget -q -O "$APPIMAGETOOL_PATH" \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$APPIMAGETOOL_PATH"
    echo "Downloaded appimagetool to: $APPIMAGETOOL_PATH"
}

# --- Create AppDir structure ---
create_appdir_structure() {
    echo "Creating AppDir structure..."
    rm -rf "$APPDIR"
    mkdir -p "$APPDIR/usr/bin"
    mkdir -p "$APPDIR/usr/lib"
    mkdir -p "$APPDIR/usr/share/applications"
    mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"
    mkdir -p "$APPDIR/usr/share/metainfo"
}

# --- Build the application ---
build_application() {
    if [ "$SKIP_BUILD" = true ]; then
        echo "Skipping build (--skip-build)"
        return
    fi

    echo "Building application..."
    cd "$PROJECT_DIR"

    local common_args=()
    if [ "$VARIANT" = "full-bundle" ]; then
        common_args+=("--include-java" "--include-languagetool")
    fi

    "$SCRIPT_DIR/build-common.sh" "${common_args[@]}" --install-root "$APPDIR"
}

# --- Create desktop wrapper script ---
create_wrapper_script() {
    local wrapper_path="$APPDIR/usr/bin/rhesis-wrapper"
    cat > "$wrapper_path" << 'EOF'
#!/bin/bash
SELF="$(readlink -f "$0")"
SELF_DIR="$(cd "$(dirname "$SELF")" && pwd)"

# Detect app root based on our location
if [ -f "$SELF_DIR/rhesis" ]; then
    # We're in usr/bin/
    APP_DIR="$(cd "$SELF_DIR/../.." && pwd)"
elif [ -f "$SELF_DIR/usr/bin/rhesis" ]; then
    # We're at AppDir root (AppRun)
    APP_DIR="$SELF_DIR"
else
    APP_DIR="$SELF_DIR"
fi

export LD_LIBRARY_PATH="$APP_DIR/usr/lib:$LD_LIBRARY_PATH"

# Prevent Qt from logging "No shell integration" warnings when falling back from Wayland to X11
if [ -z "${QT_QPA_PLATFORM:-}" ]; then
    export QT_QPA_PLATFORM=xcb
fi

if [ -d "$APP_DIR/usr/plugins" ]; then
    export QT_PLUGIN_PATH="$APP_DIR/usr/plugins"
fi

if [ -d "$APP_DIR/usr/lib/qml" ]; then
    export QML2_IMPORT_PATH="$APP_DIR/usr/lib/qml"
fi

if [ -d "$APP_DIR/usr/lib/jre" ]; then
    export JAVA_HOME="$APP_DIR/usr/lib/jre"
    export PATH="$JAVA_HOME/bin:$PATH"
elif [ -z "${JAVA_HOME:-}" ]; then
    if ! command -v java &>/dev/null; then
        echo "Error: Java not found."
        echo "Please install OpenJDK 17 or later."
        echo "On Ubuntu/Debian: sudo apt install openjdk-17-jdk"
        echo "On Fedora: sudo dnf install java-17-openjdk-devel"
        echo "On Arch: sudo pacman -S jdk17-openjdk"
        exit 1
    fi
fi

if [ -d "$APP_DIR/usr/share/languagetool" ]; then
    export RHESIS_LANGUAGETOOL_DIR="$APP_DIR/usr/share/languagetool"
fi

if [ -d "$APP_DIR/usr/lib/fastText" ]; then
    export PATH="$APP_DIR/usr/lib/fastText:$PATH"
fi

exec "$APP_DIR/usr/bin/rhesis" "$@"
EOF
    chmod +x "$wrapper_path"
}

# --- Copy desktop integration files ---
copy_desktop_files() {
    echo "Copying desktop integration files..."
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.desktop" "$APPDIR/usr/share/applications/"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.png" "$APPDIR/usr/share/icons/hicolor/256x256/apps/"
    cp "$PROJECT_DIR/io.github.dimkar3000.rhesis.metainfo.xml" "$APPDIR/usr/share/metainfo/"

    # Copy desktop/icon to AppDir root for appimagetool
    cp "$APPDIR/usr/share/applications/io.github.dimkar3000.rhesis.desktop" "$APPDIR/"
    cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png" "$APPDIR/"
}

# --- Bundle Qt dependencies ---
bundle_qt() {
    echo "Bundling Qt dependencies..."

    export QMAKE="$(which qmake6 2>/dev/null || which qmake 2>/dev/null || echo "")"

    # Run linuxdeploy to deploy shared libraries
    "$LINUXDEPLOY_PATH" \
        --appdir "$APPDIR" \
        --executable "$APPDIR/usr/bin/rhesis" \
        --desktop-file "$APPDIR/usr/share/applications/io.github.dimkar3000.rhesis.desktop" \
        --icon-file "$APPDIR/usr/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png" || true

    # Manually copy required Qt plugins
    local qt_plugin_dir="/usr/lib/qt6/plugins"
    [ -d "$qt_plugin_dir" ] || qt_plugin_dir="/usr/lib64/qt6/plugins"

    if [ -d "$qt_plugin_dir" ]; then
        echo "Copying Qt plugins from $qt_plugin_dir..."

        # Platform plugins (required for Qt to start)
        for dir in platforms platforminputcontexts platformthemes xcbglintegrations; do
            if [ -d "$qt_plugin_dir/$dir" ]; then
                mkdir -p "$APPDIR/usr/plugins/$dir"
                cp "$qt_plugin_dir/$dir/"*.so "$APPDIR/usr/plugins/$dir/" 2>/dev/null || true
            fi
        done

        # Image format plugins
        if [ -d "$qt_plugin_dir/imageformats" ]; then
            mkdir -p "$APPDIR/usr/plugins/imageformats"
            cp "$qt_plugin_dir/imageformats/"*.so "$APPDIR/usr/plugins/imageformats/" 2>/dev/null || true
        fi

        # TLS/network plugins
        for dir in tls networkinformation; do
            if [ -d "$qt_plugin_dir/$dir" ]; then
                mkdir -p "$APPDIR/usr/plugins/$dir"
                cp "$qt_plugin_dir/$dir/"*.so "$APPDIR/usr/plugins/$dir/" 2>/dev/null || true
            fi
        done

        # Copy Qt QML modules
        local qt_qml_dir="/usr/lib/qt6/qml"
        [ -d "$qt_qml_dir" ] || qt_qml_dir="/usr/lib64/qt6/qml"
        if [ -d "$qt_qml_dir" ]; then
            echo "Copying QML modules from $qt_qml_dir..."
            mkdir -p "$APPDIR/usr/lib/qml"
            cp -r "$qt_qml_dir/"* "$APPDIR/usr/lib/qml/" 2>/dev/null || true
        fi

        echo "Qt plugins copied."
    else
        echo "Warning: Qt plugin directory not found at $qt_plugin_dir"
    fi
}

# --- Create AppImage ---
create_appimage() {
    echo "Creating AppImage..."

    local appimage_name
    if [ "$VARIANT" = "full-bundle" ]; then
        appimage_name="rhesis-${VERSION}-x86_64.AppImage"
    else
        appimage_name="rhesis-${VERSION}-minimal-x86_64.AppImage"
    fi

    local output_path="$BUILD_DIR/$appimage_name"
    rm -f "$output_path"

    # Copy AppRun to AppDir root
    cp "$APPDIR/usr/bin/rhesis-wrapper" "$APPDIR/AppRun"
    chmod +x "$APPDIR/AppRun"

    # Create symlinks for desktop file and icon in AppDir root
    ln -sf "usr/share/applications/io.github.dimkar3000.rhesis.desktop" "$APPDIR/io.github.dimkar3000.rhesis.desktop"
    ln -sf "usr/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png" "$APPDIR/io.github.dimkar3000.rhesis.png"

    # Use appimagetool directly (ARCH env var propagates correctly this way)
    ARCH=x86_64 "$APPIMAGETOOL_PATH" --no-appstream "$APPDIR" "$output_path"

    echo "AppImage created: $output_path"
}

# --- Print AppImage info ---
print_info() {
    local appimage_name
    if [ "$VARIANT" = "full-bundle" ]; then
        appimage_name="rhesis-${VERSION}-x86_64.AppImage"
    else
        appimage_name="rhesis-${VERSION}-minimal-x86_64.AppImage"
    fi

    local output_path="$BUILD_DIR/$appimage_name"

    echo ""
    echo "=== AppImage Build Complete ==="
    echo "Variant: $VARIANT"
    echo "Output: $output_path"
    echo "Size: $(du -h "$output_path" | cut -f1)"
    echo ""
    echo "To run:"
    echo "  chmod +x $output_path"
    echo "  $output_path"
    echo ""
    echo "To extract and inspect:"
    echo "  $output_path --appimage-extract"
}

# --- Main function ---
main() {
    echo "=== Rhesis AppImage Builder ==="
    echo "Variant: $VARIANT"
    echo ""

    resolve_linuxdeploy
    resolve_linuxdeploy_qt
    resolve_appimagetool
    echo ""

    create_appdir_structure
    build_application
    create_wrapper_script
    copy_desktop_files
    bundle_qt
    create_appimage
    print_info
}

main
