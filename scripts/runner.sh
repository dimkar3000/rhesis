#!/bin/sh
# Rhesis installer / launcher / uninstaller
#
# A single script installed as /usr/bin/rhesis (global install) or
# ~/.local/bin/rhesis (per-user install). It runs the application by
# default; the same file also acts as the installer:
#
#   rhesis            - runs the application
#   rhesis --install [global|local]  - installs; when no location is given,
#                       asks global/local and defaults to global. Finds the
#                       release archive and extracts it
#   rhesis --uninstall- removes all Rhesis files and itself
#
# All Rhesis files live in a single folder (/usr/share/rhesis for a global
# install, ~/.local/share/rhesis for a per-user install): binaries, bundled
# JRE, LanguageTool, translations, license, icon and metainfo. Only two files
# live elsewhere, both created and removed by this script:
#   - the launcher itself (this file, installed as rhesis in /usr/bin or
#     ~/.local/bin)
#   - the desktop entry in /usr/share/applications or
#     ~/.local/share/applications (required there so the app shows up in the
#     application menu; its Icon= points into the rhesis folder)
# Uninstalling deletes exactly the rhesis folder plus those two files; no
# other folders are ever removed.
#
# The installer looks for the release archive in this order:
#   1. $RHESIS_ARCHIVE_SRC (if set, and errors out when the archive is missing)
#   2. the current working directory
#   3. the latest GitHub release (asset named rhesis-<version>-x86_64[-local].tar.gz)
# Override the GitHub repo with RHESIS_REPO (default: dimkar3000/rhesis).

set -e

uninstall() {
    echo "Removing Rhesis files..."
    rm -rf "$PREFIX"
    rm -f "$0"
    if [ "$SCRIPT_DIR" = "${HOME:-}/.local/bin" ]; then
        rm -f "${HOME:-}/.local/share/applications/io.github.dimkar3000.rhesis.desktop"
    else
        rm -f /usr/share/applications/io.github.dimkar3000.rhesis.desktop
    fi
    echo "Rhesis uninstalled."
}

install_desktop_entry() {
    # $1 = global|local, $2 = payload dir (the rhesis folder). Creates the
    # application-menu entry in the standard XDG applications directory,
    # rewriting Icon= to an absolute path into the rhesis folder (the icon is
    # not part of an icon theme).
    local apps_dir icon src
    if [ "$1" = "global" ]; then
        apps_dir="/usr/share/applications"
        icon="$2/io.github.dimkar3000.rhesis.png"
    else
        apps_dir="${HOME:-}/.local/share/applications"
        icon="$2/io.github.dimkar3000.rhesis.png"
    fi
    src="$2/io.github.dimkar3000.rhesis.desktop"
    [ -f "$src" ] || return 0
    mkdir -p "$apps_dir"
    sed "s|^Icon=.*|Icon=$icon|" "$src" > "$apps_dir/io.github.dimkar3000.rhesis.desktop"
}

find_archive() {
    # $1 = global|local, $2 = temp dir for downloads; prints the path of the
    # matching release archive. Search order: $RHESIS_ARCHIVE_SRC (if set),
    # the current directory, then the latest GitHub release.
    local install_type="$1"
    local tmp_dir="$2"
    local suffix=""
    [ "$install_type" = "local" ] && suffix="-local"

    if [ -n "${RHESIS_ARCHIVE_SRC:-}" ]; then
        set -- "$RHESIS_ARCHIVE_SRC"/rhesis-*-x86_64${suffix}.tar.gz
        if [ -f "$1" ]; then
            printf '%s\n' "$1"
            return 0
        fi
        echo "Error: no rhesis-*-x86_64${suffix}.tar.gz archive in $RHESIS_ARCHIVE_SRC" >&2
        echo "       unset RHESIS_ARCHIVE_SRC to check the current directory or download from GitHub" >&2
        exit 1
    fi

    set -- "$PWD"/rhesis-*-x86_64${suffix}.tar.gz
    if [ -f "$1" ]; then
        printf '%s\n' "$1"
        return 0
    fi

    download_archive "$install_type" "$tmp_dir"
}

download_archive() {
    # $1 = global|local, $2 = destination directory; downloads the matching
    # archive from the latest GitHub release and prints its path.
    local suffix=""
    [ "$1" = "local" ] && suffix="-local"

    echo "No archive in the current directory; checking the latest GitHub release..." >&2

    local json=""
    json="$(curl -fsSL "https://api.github.com/repos/${RHESIS_REPO:-dimkar3000/rhesis}/releases/latest" 2>/dev/null)" || {
        echo "Error: could not fetch the latest release from GitHub" >&2
        exit 1
    }

    local asset_url=""
    asset_url="$(printf '%s\n' "$json" | sed -n "s|.*\"browser_download_url\": *\"\([^\"]*rhesis-[^\"]*x86_64${suffix}\.tar\.gz\)\".*|\1|p" | head -1)"

    if [ -z "$asset_url" ]; then
        echo "Error: no rhesis-*-x86_64${suffix}.tar.gz asset in the latest release" >&2
        exit 1
    fi

    local dest="$2/${asset_url##*/}"
    echo "Downloading $asset_url" >&2
    curl -fsSL -o "$dest" "$asset_url" || {
        echo "Error: download failed" >&2
        exit 1
    }
    printf '%s\n' "$dest"
}

install() {
    INSTALL_TYPE="${1:-}"
    if [ -z "$INSTALL_TYPE" ]; then
        printf 'Install type (global/local) [global]: '
        read -r INSTALL_TYPE
        INSTALL_TYPE="${INSTALL_TYPE:-global}"
    fi
    case "$INSTALL_TYPE" in
        global|g) INSTALL_TYPE=global ;;
        local|l) INSTALL_TYPE=local ;;
        *)
            echo "Error: invalid install type: $INSTALL_TYPE" >&2
            exit 1
            ;;
    esac

    if [ "$INSTALL_TYPE" = "global" ] && [ "$(id -u)" -ne 0 ]; then
        echo "Error: a global install writes to / and requires root" >&2
        echo "       rerun with sudo, or choose a local install" >&2
        exit 1
    fi

    local target="/"
    [ "$INSTALL_TYPE" = "local" ] && target="$HOME"

    echo "Installing Rhesis ($INSTALL_TYPE) to $target ..."

    local archive
    archive="$(find_archive "$INSTALL_TYPE" "$tmp")"

    if [ "${archive#$tmp/}" = "$archive" ]; then
        cp "$archive" "$tmp/" # local file; downloads already land in $tmp
    fi
    tar -xzf "$tmp/${archive##*/}" --no-same-owner --no-overwrite-dir -C "$target"

    if [ "$INSTALL_TYPE" = "global" ]; then
        install_desktop_entry global "/usr/share/rhesis"
        echo "Rhesis installed. Run: rhesis"
    else
        install_desktop_entry local "${HOME:-}/.local/share/rhesis"
        echo "Rhesis installed. Run: ~/.local/bin/rhesis"
    fi
}

case "${1:-}" in
    --install)
        tmp="$(mktemp -d /tmp/rhesis-install.XXXXXX)"
        trap 'rm -rf "$tmp"' EXIT HUP INT TERM
        install "${2:-}"
        ;;
    *)
        SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"

        case "${1:-}" in
            --uninstall|uninstall)
                # Refuse to uninstall from an unrecognized location: the
                # uninstaller deletes this script itself ($0), so running it
                # from a random copy (e.g. the repo) would delete the copy
                # instead of the installed rhesis.
                if [ "$SCRIPT_DIR" = "${HOME:-}/.local/bin" ]; then
                    PREFIX="${HOME}/.local/share/rhesis"
                    uninstall
                elif [ "$SCRIPT_DIR" = "/usr/bin" ]; then
                    PREFIX="/usr/share/rhesis"
                    uninstall
                else
                    echo "Error: uninstall must be run from the installed rhesis script" >&2
                    echo "       run: ${HOME:-}/.local/bin/rhesis --uninstall (or /usr/bin/rhesis --uninstall)" >&2
                    exit 1
                fi
                ;;
            *)
                if [ "$SCRIPT_DIR" = "${HOME:-}/.local/bin" ]; then
                    PREFIX="${HOME}/.local/share/rhesis"
                elif [ "$SCRIPT_DIR" = "/usr/bin" ]; then
                    PREFIX="/usr/share/rhesis"
                elif [ -x "${HOME:-}/.local/share/rhesis/bin/rhesis" ]; then
                    # Not installed as `rhesis` (e.g. run from the repo):
                    # fall back to an existing per-user install so
                    # ./scripts/runner.sh keeps working during development.
                    PREFIX="${HOME}/.local/share/rhesis"
                elif [ -x /usr/share/rhesis/bin/rhesis ]; then
                    PREFIX="/usr/share/rhesis"
                else
                    echo "Error: Rhesis is not installed. Run: ${0##*/} --install" >&2
                    exit 1
                fi

                if [ ! -x "$PREFIX/bin/rhesis" ]; then
                    echo "Rhesis is not installed yet. Run: ${0##*/} --install" >&2
                    exit 1
                fi
                export PATH="$PREFIX/bin:${PATH}"
                exec "$PREFIX/bin/rhesis" "$@"
                ;;
        esac
        ;;
esac
