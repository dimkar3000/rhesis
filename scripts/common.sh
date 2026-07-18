# --- Global flags ---
# Overridable via --verbose and --no-spinner respectively
VERBOSE=false
NO_SPINNER=false
[ -t 1 ] && NO_SPINNER=true

# --- Step runner with spinner ---
# Requires: VERBOSE, NO_SPINNER set before calling step()

step() {
    local msg="$1"
    shift

    if [ "$VERBOSE" = true ]; then
        echo "$msg"
        "$@"
        local rc=$?
        if [ $rc -ne 0 ]; then
            echo "❌ $msg"
            return $rc
        fi
        echo "✅ $msg"
        return 0
    fi

    if [ "${NO_SPINNER:-false}" != true ]; then
        local flag=""
        flag=$(mktemp -u /tmp/rhesis-step.XXXXXXXX 2>/dev/null) || flag=$(mktemp -u 2>/dev/null) || flag="/tmp/rhesis-step.$$"
        (
            local rc=0
            "$@" >/dev/null 2>&1 || rc=$?
            echo "$rc" > "$flag"
        ) &
        local pid=$!
        local chars='⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏'
        local i=0
        while [ ! -f "$flag" ]; do
            printf "\r%s %s" "${chars:$i:1}" "$msg"
            i=$(( (i + 1) % ${#chars} ))
            sleep 0.1
        done
        wait "$pid" 2>/dev/null || true
        local rc=0
        read -r rc < "$flag" || true
        rm -f "$flag"
        if [ "$rc" -ne 0 ]; then
            printf "\r❌ %s\n" "$msg"
            return "$rc"
        fi
        printf "\r✅ %s\n" "$msg"
    else
        echo "$msg ..."
        local rc=0
        "$@" >/dev/null 2>&1 || rc=$?
        if [ $rc -ne 0 ]; then
            echo "❌ $msg"
            return $rc
        fi
        echo "✅ $msg"
    fi
}
