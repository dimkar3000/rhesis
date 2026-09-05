#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

BUILD_DIR="$PROJECT_DIR/build"
APPDIR="$BUILD_DIR/appimage/AppDir"
TOOLS_DIR="$BUILD_DIR/tools/appimage"
ARTIFACTS_DIR="$BUILD_DIR/artifacts"
VERSION=$(grep '^version' "$PROJECT_DIR/Cargo.toml" | head -1 | sed 's/.*"\(.*\)".*/\1/')
export INSTALL_PREFIX="/"

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
    mkdir -p "$APPDIR/app"

    cp -r "$ARTIFACTS_DIR/app/"* "$APPDIR/app/"

    install -Dm755 "$ARTIFACTS_DIR/fastText/fasttext" "$APPDIR/app/bin/fasttext"
    install -Dm644 "$ARTIFACTS_DIR/lid.176.ftz" "$APPDIR/app/share/rhesis/lid.176.ftz"

    mkdir -p "$APPDIR/app/share/rhesis"
    cp -r "$ARTIFACTS_DIR/LanguageTool" "$APPDIR/app/share/rhesis/LanguageTool"
    cat > "$APPDIR/app/share/rhesis/LanguageTool/server.properties" << EOF
fasttextModel=../../../share/rhesis/lid.176.ftz
fasttextBinary=../../../bin/fasttext
EOF

    ln -sf app/share/applications/io.github.dimkar3000.rhesis.desktop "$APPDIR/"
    ln -sf app/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png "$APPDIR/"
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
        mkdir -p "$APPDIR/app/lib/qt6/qml"
        cp -r "$qml_src/"Qt* "$qml_src/org" "$APPDIR/app/lib/qt6/qml/" 2>/dev/null || true
    fi

    export PATH="$TOOLS_DIR:$PATH"
    export QMAKE="$(command -v qmake6 || command -v qmake || true)"
    # linuxdeploy's bundled strip doesn't understand modern ELF sections (.relr.dyn)
    NO_STRIP=1 "$LINUXDEPLOY" --appdir "$APPDIR" \
        --executable "$APPDIR/app/bin/rhesis" \
        --desktop-file "$APPDIR/app/share/applications/io.github.dimkar3000.rhesis.desktop" \
        --icon-file "$APPDIR/app/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png"

    # Bundle the JRE last so linuxdeploy doesn't scan its internal
    # shared objects (e.g. libjvm.so), which it cannot resolve
    mkdir -p "$APPDIR/app/lib"
    cp -r "$ARTIFACTS_DIR/java/jre" "$APPDIR/app/lib/"
    ln -sf ../lib/jre/bin/java "$APPDIR/app/bin/java"

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
        mkdir -p "$APPDIR/app/lib/qt6/plugins/platforms"
        for plat in libqxcb.so libqwayland*.so; do
            found=$(find "$qt_plugin_dir/platforms" -maxdepth 1 -name "$plat" -type f 2>/dev/null || true)
            [ -n "$found" ] && cp "$found" "$APPDIR/app/lib/qt6/plugins/platforms/" 2>/dev/null
        done
        for subdir in platforminputcontexts platformthemes xcbglintegrations imageformats tls networkinformation wayland-shell-integration; do
            [ -d "$qt_plugin_dir/$subdir" ] && {
                mkdir -p "$APPDIR/app/lib/qt6/plugins/$subdir"
                cp "$qt_plugin_dir/$subdir/"*.so "$APPDIR/app/lib/qt6/plugins/$subdir/" 2>/dev/null || true
            }
        done
    fi
}

# Qt >= 6.5 loads libxcb-cursor (plus other xcb-util and XcbQpa libraries)
# from libqxcb.so, but plain linuxdeploy only scans the main executable above
# and never sees these manually copied plugins. Deploy every library the
# bundled plugins AND bundled QML modules need (QML plugins like
# libqtquickdialogsplugin.so have their own Qt-private dependencies, e.g.
# libQt6QuickDialogs2QuickImpl, which fail the same way when resolved from a
# different host Qt), except core system libraries and GPU drivers (those
# must always come from the host) and the libraries that
# remove_bundled_system_libs() deliberately drops afterwards. Target dir is
# usr/lib, which AppRun puts on LD_LIBRARY_PATH.
#
# Pairing rule: libxkbcommon and libxkbcommon-x11 are lockstep-coupled (the
# latter calls the former's internals) and must come from the SAME source.
# Since remove_bundled_system_libs() drops libxkbcommon.so.0, this step must
# not deploy libxkbcommon-x11.so.0 either — a mixed bundled/host pair
# segfaults inside xkb_state_update_mask during xcb init. Both resolve from
# the host instead.
#
# Same for the font stack below: it must stay host-paired with the host's
# /etc/fonts, so libfontconfig/libfreetype/libexpat are excluded here and
# deleted in remove_bundled_system_libs().
#
# A plugin whose dependencies cannot be resolved on the build host (e.g. an
# optional kimg_* image format plugin needing an uninstalled codec library)
# is removed again with a loud warning: Qt logs and continues without that
# feature. The only exception is platforms/: without a working platform
# plugin the application cannot start at all, so that fails the build.
deploy_qt_plugin_deps() {
    local lib_dir="$APPDIR/usr/lib"
    mkdir -p "$lib_dir"

    local deny_re='^(ld-linux|libc\.so|libm\.so|libpthread\.so|libdl\.so|librt\.so|libnsl\.so|libutil\.so|libresolv\.so|libnss_|libGL\.so|libEGL\.so|libGLX\.so|libGLdispatch\.so|libOpenGL\.so|libdrm\.so|libvulkan\.so|libgbm\.so|libxkbcommon\.so|libxkbcommon-x11\.so|libwayland-cursor\.so|libwayland-server\.so|libfontconfig\.so|libfreetype\.so|libexpat\.so)'

    local fatal=0
    local plugin
    local -a scan_dirs=("$APPDIR/app/lib/qt6/plugins")
    [ -d "$APPDIR/app/lib/qt6/qml" ] && scan_dirs+=("$APPDIR/app/lib/qt6/qml")
    while IFS= read -r plugin; do
        [ -n "$plugin" ] || continue
        local line name rest path
        local -a missing_names=()
        local -a wanted_files=()
        while IFS= read -r line; do
            # strip leading whitespace; ldd prints e.g.
            #   libfoo.so.0 => /path/libfoo.so.0 (0x...)
            #   libfoo.so.0 => not found
            #   /lib64/ld-linux-x86-64.so.2 (0x...)
            #   linux-vdso.so.1 (0x...)
            line="${line#"${line%%[![:space:]]*}"}"
            case "$line" in
                linux-vdso*|"") continue ;;
            esac
            if [[ "$line" == *"=> not found"* ]]; then
                missing_names+=("${line%% *}")
                continue
            fi
            [[ "$line" != *"=>"* ]] && continue
            name="${line%% *}"
            rest="${line#*=> }"
            path="${rest%% *}"
            [ -e "$lib_dir/$name" ] && continue
            [[ "$name" =~ $deny_re ]] && continue
            if [ -e "$APPDIR/app/lib/$name" ]; then
                continue
            fi
            wanted_files+=("$name:$path")
        done < <(ldd "$plugin" 2>/dev/null)

        if [ "${#missing_names[@]}" -gt 0 ]; then
            if [[ "$plugin" == */platforms/* ]]; then
                echo "ERROR: $plugin needs missing libraries (${missing_names[*]}), and no platform means no application" >&2
                fatal=1
            else
                echo "WARNING: dropping $plugin, its dependencies are not on the build host (${missing_names[*]})" >&2
                rm -f "$plugin"
            fi
            continue
        fi

        local entry
        for entry in ${wanted_files[@]+"${wanted_files[@]}"}; do
            name="${entry%%:*}"
            path="${entry#*:}"
            cp -L "$path" "$lib_dir/$name"
        done
    done < <(find "${scan_dirs[@]}" \( -type f -o -type l \) -name "*.so*" 2>/dev/null || true)

    if [ "$fatal" -ne 0 ]; then
        echo "ERROR: Qt platform plugins have unresolved dependencies, refusing to build a broken AppImage" >&2
        return 1
    fi
}

write_apprun() {
    cat > "$APPDIR/AppRun" << 'APPRUN'
#!/bin/bash
HERE=$(dirname "$(readlink -f "$0")")
export PATH="${HERE}/app/bin:${PATH}"
# linuxdeploy keeps bundled system libs in usr/lib; the binary's
# RPATH ($ORIGIN/../usr/lib) no longer resolves from app/bin
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH:-}"
export QT_QPA_PLATFORM="${QT_QPA_PLATFORM:-xcb}"
export QT_PLUGIN_PATH="${HERE}/app/lib/qt6/plugins"
export QML2_IMPORT_PATH="${HERE}/app/lib/qt6/qml"
export RHESIS_LANGUAGETOOL_DIR="${HERE}/app/share/rhesis/LanguageTool"
exec "${HERE}/app/bin/rhesis" "$@"
APPRUN
    chmod +x "$APPDIR/AppRun"
}

remove_bundled_system_libs() {
    local lib_dir="$APPDIR/usr/lib"
    # Dropped deliberately so the host versions are used instead:
    # - wayland/xkbcommon: must stay paired with host counterparts (see above)
    # - font stack (fontconfig/freetype/expat): an older bundled libfontconfig
    #   cannot parse newer host /etc/fonts configs (e.g. xsi:nil syntax) and
    #   spams warnings, while a newer host fontconfig against an older bundled
    #   freetype risks missing symbols. The host triple is always consistent,
    #   and any system able to run this AppImage (glibc floor) ships it.
    for lib in libxkbcommon.so.0 libwayland-cursor.so.0 libwayland-server.so.0 \
               libfontconfig.so.1 libfreetype.so.6 libexpat.so.1; do
        rm -f "$lib_dir/$lib"
    done
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

    "$SCRIPT_DIR/build-common.sh" "${COMMON_ARGS[@]}"

    echo "=== AppImage Build ==="
    echo ""

    download_tool "https://github.com/linuxdeploy/linuxdeploy/releases/download/continuous/linuxdeploy-x86_64.AppImage" "$LINUXDEPLOY"
    download_tool "https://github.com/linuxdeploy/linuxdeploy-plugin-qt/releases/download/continuous/linuxdeploy-plugin-qt-x86_64.AppImage" "$TOOLS_DIR/linuxdeploy-plugin-qt-x86_64.AppImage"
    download_tool "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage" "$APPIMAGETOOL"

    step "Creating AppDir" create_appdir
    step "Writing AppRun" write_apprun
    step "Bundling Qt and KDE dependencies" bundle_qt
    step "Deploying Qt plugin dependencies" deploy_qt_plugin_deps
    step "Removing incompatible bundled system libraries" remove_bundled_system_libs
    step "Creating AppImage" create_appimage

    echo ""
    echo "AppImage created: $ARTIFACTS_DIR/appimage/rhesis-${VERSION}-x86_64.AppImage"
}

main "$@"
