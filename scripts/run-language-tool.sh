#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# --- Main ---
default_port=8081

usage() {
    echo "Usage: $(basename "$0") [PORT]"
    echo ""
    echo "Starts a standalone LanguageTool HTTP server, the same component"
    echo "Rhesis connects to when the 'External (remote server)' option is selected."
    echo ""
    echo "  PORT            Port to listen on (default: ${default_port})"
    echo "  RHESIS_LT_DIR   LanguageTool installation directory to use; defaults to"
    echo "                  the first of: $PROJECT_DIR/build/LanguageTool,"
    echo "                  $PROJECT_DIR/build/artifacts/LanguageTool,"
    echo "                  /usr/share/rhesis/LanguageTool, \$HOME/.local/share/rhesis/LanguageTool"
    echo ""
    echo "Example: $(basename "$0") 8082"
    exit 0
}

[[ $# -le 1 && "${1:-}" != "--help" && "${1:-}" != "-h" ]] || usage
for arg in "$@"; do
    if [[ "$arg" =~ ^[[:digit:]]+$ && "$arg" -ge 1 && "$arg" -le 65534 ]]; then
        port="$arg"
    else
        echo "Invalid port \"$arg\" (must be in [1-65534])" >&2
        exit 1
    fi
done
port="${port:-$default_port}"
lt_dir="${RHESIS_LT_DIR:-}"

# Locate languagetool-server.jar the same way the app does (src/languagetool/service.rs).
if [[ -z "$lt_dir" ]]; then
    for candidate in \
        "$PROJECT_DIR/build/LanguageTool" \
        "$PROJECT_DIR/build/artifacts/LanguageTool" \
        "/usr/share/rhesis/LanguageTool" \
        "$HOME/.local/share/rhesis/LanguageTool"; do
        if [[ -f "$candidate/languagetool-server.jar" ]]; then
            lt_dir="$candidate"
            break
        fi
    done
fi

if [[ -z "$lt_dir" ]]; then
    echo "LanguageTool is not installed (languagetool-server.jar not found); run scripts/setup.sh to download it" >&2
    exit 1
fi

command -v java >/dev/null || { echo "java not found; install a JDK 17+" >&2; exit 1; }

# Fail fast when something else already owns the port instead of letting the JVM
# grind on a bind error. Connect attempt only succeeds if the port is taken.
if timeout 1 bash -c "</dev/tcp/127.0.0.1/$port" 2>/dev/null; then
    echo "port $port is already in use; stop the process holding it first" >&2
    exit 1
fi

if [[ ! -f "$lt_dir/server.properties" ]]; then
    echo "Warning: server.properties is missing in $lt_dir; starting without language-detection config" >&2
fi

echo "Starting LanguageTool at http://localhost:$port (working dir: $lt_dir)"
# exec replaces the shell so Ctrl+C / kill go straight to the JVM, never leaving
# an orphaned server behind. The jar and server.properties are relative paths,
# so the JVM must run from the installation dir (same as the embedded spawn).
cd "$lt_dir"
exec java -cp languagetool-server.jar org.languagetool.server.HTTPServer \
    --config server.properties --port "$port" --allow-origin
