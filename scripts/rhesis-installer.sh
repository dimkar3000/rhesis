#!/bin/bash
# Rhesis installer / launcher / uninstaller
#
# A single script installed as /usr/bin/rhesis (global install) or
# ~/.local/bin/rhesis (per-user install). It runs the application by
# default; the same file also acts as the installer:
#
#   rhesis            - runs the application
#   rhesis --hidden   - runs it hidden to the tray (how the login autostart
#                       entry launches it; needs a system tray, otherwise
#                       the window opens normally)
#   rhesis --install [global|local]  - installs; when no location is given,
#                       asks global/local and defaults to global. Finds the
#                       release archive and extracts it
#   rhesis --uninstall            - removes all Rhesis files and itself
#   rhesis --help                 - prints this help
#
# Any other argument is unexpected: it prints the help and exits with an
# error instead of being forwarded anywhere.
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

usage() {
    cat <<EOF
Usage: ${0##*/} [OPTION]

Runs Rhesis when called without an option.

Options:
  --hidden                  Run hidden to the tray (used by the login
                            autostart entry; requires a system tray).
  --install [global|local]  Install Rhesis and create the application-menu
                            entry; asks for the location and defaults to
                            global when omitted.
  --uninstall               Remove the Rhesis files and this launcher.
  -h, --help                Show this help and exit.

Any other argument is unexpected and is rejected with this help.
EOF
}

uninstall() {
    echo "Removing Rhesis files..."
    # A live instance survives file deletion on Linux and rewrites state
    # after this uninstall finishes: an open settings page re-commits the
    # autostart entry on Apply and the daemon re-creates its settings
    # store. Stop this payload's processes before touching the files.
    stop_running_instances
    rm -rf "$PREFIX"
    rm -f "$0"
    if [ "$SCRIPT_DIR" = "$USER_HOME/.local/bin" ]; then
        rm -f "$USER_HOME/.local/share/applications/io.github.dimkar3000.rhesis.desktop"
    else
        rm -f /usr/share/applications/io.github.dimkar3000.rhesis.desktop
    fi
    # Releases before the autostart path fix wrote the entry straight into
    # the config root, where no session ever reads it; drop the junk.
    rm -f "${XDG_CONFIG_HOME:-$USER_HOME/.config}/io.github.dimkar3000.rhesis.desktop"
    # The autostart entry is written by the app (Settings toggle), but one
    # left behind by THIS flavor would keep failing at every login (systemd
    # 203/EXEC once the launcher is gone). Remove it when its Exec= points
    # at this launcher OR at the direct binary of this install: entries of
    # other flavors (flatpak run ...) survive a native uninstall.
    autostart="${XDG_CONFIG_HOME:-$USER_HOME/.config}/autostart/io.github.dimkar3000.rhesis.desktop"
    local launcher_exec="$SCRIPT_DIR/$(basename "$0") --hidden"
    local binary_exec=""
    if [ "$SCRIPT_DIR" = "$USER_HOME/.local/bin" ]; then
        binary_exec="$USER_HOME/.local/share/rhesis/bin/rhesis --hidden"
    elif [ "$SCRIPT_DIR" = "/usr/bin" ]; then
        binary_exec="/usr/share/rhesis/bin/rhesis --hidden"
    fi
    if [ -f "$autostart" ]; then
        # POSIX && and || share one precedence level and associate left, so
        # the matcher is grouped explicitly: an entry written for this
        # launcher dies, or one pointing at this install's direct binary
        # does. Ungrouped, the launcher match is discarded and the binary
        # grep alone decides — launcher entries would survive the uninstall.
        if grep -Fqs "Exec=$launcher_exec" "$autostart" \
            || { [ -n "$binary_exec" ] && grep -Fqs "Exec=$binary_exec" "$autostart"; }; then
            rm -f "$autostart"
        fi
    fi
    echo "Rhesis uninstalled."
}

stop_running_instances() {
    # pkill -f matches the client and the daemon alike: the daemon's path
    # ($PREFIX/bin/rhesis-service) carries the client's as a prefix.
    pkill -TERM -f "$PREFIX/bin/rhesis" 2>/dev/null || true
    local waited=0
    while [ "$waited" -lt 10 ] && pgrep -f "$PREFIX/bin/rhesis" >/dev/null 2>&1; do
        sleep 0.2
        waited=$((waited + 1))
    done
    if pgrep -f "$PREFIX/bin/rhesis" >/dev/null 2>&1; then
        pkill -KILL -f "$PREFIX/bin/rhesis" 2>/dev/null || true
    fi
    # Older daemons (pre PDEATHSIG) leave the bundled LanguageTool JVM
    # orphaned when they die. It belongs to this payload only when its
    # working directory sits inside it (the worker starts java with the
    # LanguageTool dir as cwd); SIGTERM is the JVM's graceful shutdown.
    for proc in /proc/[0-9]*; do
        case "$(readlink "$proc/cwd" 2>/dev/null)" in
            "$PREFIX"/LanguageTool*) kill -TERM "${proc##*/}" 2>/dev/null || true ;;
        esac
    done
}

# User-level paths (per-user launcher, autostart entry, menu entry) belong
# to the account that invoked this run, but elevation swaps HOME to root's:
# an elevated re-exec carries the account explicitly, plain sudo/doas leak
# it through the environment.
invoking_home() {
    local home="${RHESIS_USER_HOME:-}"
    local user="${RHESIS_USER:-${SUDO_USER:-${DOAS_USER:-}}}"
    if [ -z "$home" ] && [ -n "$user" ] && [ "$user" != "root" ]; then
        home="$(getent passwd "$user" 2>/dev/null | cut -d: -f6)"
        [ -n "$home" ] || home="/home/$user"
    fi
    if [ -n "$home" ]; then
        printf '%s\n' "$home"
    else
        printf '%s\n' "${HOME:-}"
    fi
}

install_desktop_entry() {
    # $1 = global|local, $2 = payload dir (the rhesis folder). Creates the
    # application-menu entry in the standard XDG applications directory,
    # rewriting Icon= to an absolute path into the rhesis folder (the icon is
    # not part of an icon theme) and Exec= to this launcher's absolute path:
    # menus resolve bare Exec= names only through the session PATH, which
    # misses ~/.local/bin (rooted user-installs would never launch).
    local apps_dir icon exec_path src
    if [ "$1" = "global" ]; then
        apps_dir="/usr/share/applications"
        icon="$2/io.github.dimkar3000.rhesis.png"
        exec_path="/usr/bin/rhesis"
    else
        apps_dir="${HOME:-}/.local/share/applications"
        icon="$2/io.github.dimkar3000.rhesis.png"
        exec_path="${HOME:-}/.local/bin/rhesis"
    fi
    src="$2/io.github.dimkar3000.rhesis.desktop"
    [ -f "$src" ] || return 0
    mkdir -p "$apps_dir"
    sed -e "s|^Icon=.*|Icon=$icon|" -e "s|^Exec=.*|Exec=$exec_path|" \
        "$src" > "$apps_dir/io.github.dimkar3000.rhesis.desktop"
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

# Try to elevate privileges for global install using available GUI tools.
# Returns 0 if already root or elevation succeeded, 1 otherwise.
maybe_elevate() {
    if [ "$(id -u)" -eq 0 ]; then
        return 0
    fi

    # Resolve RHESIS_ARCHIVE_SRC to absolute path so it works after pkexec
    # changes the working directory.
    local archive_src="${RHESIS_ARCHIVE_SRC:-}"
    if [ -n "$archive_src" ] && [ "${archive_src#/}" = "$archive_src" ]; then
        # Relative path — resolve from current directory
        archive_src="$(cd "$(dirname "$archive_src")" && pwd)/$(basename "$archive_src")"
    fi

    # pkexec strips the environment; explicitly pass needed vars. The
    # invoking account rides along so the elevated run resolves user-level
    # paths (autostart entry) against the right home.
    local pkexec_env="env RHESIS_ARCHIVE_SRC=${archive_src} RHESIS_REPO=${RHESIS_REPO:-} RHESIS_USER=$(id -un) RHESIS_USER_HOME=${HOME:-}"

    # Use ORIGINAL_ARGS to re-execute the exact original command line
    local script_path="$(readlink -f "$0")"

    # Try pkexec first (policykit, works on most desktops)
    if command -v pkexec >/dev/null 2>&1; then
        pkexec $pkexec_env "$script_path" "${ORIGINAL_ARGS[@]}" && return 0 || return 1
    fi

    # Try kdialog (KDE)
    if command -v kdialog >/dev/null 2>&1; then
        kdialog --title "Rhesis" --yesno "Global install or uninstall requires root privileges. Allow?" && \
        pkexec $pkexec_env "$script_path" "${ORIGINAL_ARGS[@]}" && return 0 || return 1
    fi

    # Try zenity (GNOME/GTK)
    if command -v zenity >/dev/null 2>&1; then
        zenity --question --title="Rhesis" --text="Global install or uninstall requires root privileges. Allow?" && \
        pkexec $pkexec_env "$script_path" "${ORIGINAL_ARGS[@]}" && return 0 || return 1
    fi

    # Fallback: plain sudo (works in terminal), preserve env with -E
    if command -v sudo >/dev/null 2>&1; then
        sudo -E "$script_path" "${ORIGINAL_ARGS[@]}" && return 0 || return 1
    fi

    return 1
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
        # Not root — try to elevate via GUI or sudo
        maybe_elevate
        exit $?
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

launch() {
    # $1 = normal|hidden. Resolves the install PREFIX for this launcher's
    # location and execs the payload. Beyond starting the binary, the
    # wrapper puts the payload's bin/ first on PATH so the daemon (spawned
    # as a sibling of the client from the same folder) and the embedded
    # server can find the bundled java/fasttext, and exports RHESIS_LAUNCHER
    # so the autostart writer points the login entry back at this launcher
    # by reading the environment instead of probing for it.
    if [ "$SCRIPT_DIR" = "${HOME:-}/.local/bin" ]; then
        PREFIX="${HOME}/.local/share/rhesis"
    elif [ "$SCRIPT_DIR" = "/usr/bin" ]; then
        PREFIX="/usr/share/rhesis"
    elif [ -x "${HOME:-}/.local/share/rhesis/bin/rhesis" ]; then
        # Not installed as `rhesis` (e.g. run from the repo):
        # fall back to an existing per-user install so
        # ./scripts/rhesis-installer.sh keeps working during development.
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
    export RHESIS_LAUNCHER="$SCRIPT_DIR/$(basename "$0")"
    if [ "$1" = "hidden" ]; then
        exec "$PREFIX/bin/rhesis" --hidden
    fi
    exec "$PREFIX/bin/rhesis"
}

SCRIPT_DIR="$(cd "$(dirname "$(readlink -f "$0")")" && pwd)"

# Save original arguments for re-execution during elevation
ORIGINAL_ARGS=("$@")

USER_HOME="$(invoking_home)"

case "${1:-}" in
    --install)
        tmp="$(mktemp -d /tmp/rhesis-install.XXXXXX)"
        trap 'rm -rf "$tmp"' EXIT HUP INT TERM
        install "${2:-}"
        ;;
    --uninstall|uninstall)
        # Refuse to uninstall from an unrecognized location: the
        # uninstaller deletes this script itself ($0), so running it
        # from a random copy (e.g. the repo) would delete the copy
        # instead of the installed rhesis.
        if [ "$SCRIPT_DIR" = "$USER_HOME/.local/bin" ]; then
            PREFIX="$USER_HOME/.local/share/rhesis"
            uninstall
        elif [ "$SCRIPT_DIR" = "/usr/bin" ]; then
            PREFIX="/usr/share/rhesis"
            # Payload and menu entry need root; without elevation set -e
            # kills the run mid-rm, before the user-level cleanup.
            if [ "$(id -u)" -ne 0 ]; then
                maybe_elevate
                exit $?
            fi
            uninstall
        else
            echo "Error: uninstall must be run from the installed rhesis script" >&2
            echo "       run: $USER_HOME/.local/bin/rhesis --uninstall (or /usr/bin/rhesis --uninstall)" >&2
            exit 1
        fi
        ;;
    --help|-h)
        usage
        ;;
    "")
        launch normal
        ;;
    --hidden)
        # The login autostart contract: linux/autostart.rs writes
        # Exec=<launcher> --hidden. Accept only the bare flag.
        if [ "$#" -ne 1 ]; then
            echo "Error: unrecognized argument: $2" >&2
            usage >&2
            exit 1
        fi
        launch hidden
        ;;
    *)
        echo "Error: unrecognized argument: $1" >&2
        usage >&2
        exit 1
        ;;
esac
