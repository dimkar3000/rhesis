#!/usr/bin/env bash
# Fail loudly when Cargo.toml / CHANGELOG.md disagree with the release tag.
#
# Enforced on final release tags only (vX.Y.Z). Pre-release tags (vX.Y.Z-*)
# and branch builds skip silently — test builds never fail on versions.
#
# In GitHub Actions the tag comes from $GITHUB_REF_NAME automatically.
# Locally: ./scripts/check-versions.sh --tag v0.4.0
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

TAG="${GITHUB_REF_NAME:-}"

while [[ $# -gt 0 ]]; do
    case $1 in
        --tag) TAG="${2:-}"; shift 2 ;;
        --help)
            echo "Usage: $(basename "$0") [--tag vX.Y.Z]"
            echo ""
            echo "Checks Cargo.toml and CHANGELOG.md against a final release tag."
            echo "Non-final tags and branch names skip silently (exit 0)."
            exit 0
            ;;
        *) echo "Unknown option: $1"; echo "Use --help for available options"; exit 1 ;;
    esac
done
TAG="${TAG#refs/tags/}"

if [[ ! "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "check-versions: not a final release tag (got '${TAG:-<none>}'), skipping"
    exit 0
fi
EXPECTED="${TAG#v}"

CARGO_VER="$(grep -m1 '^version' "$PROJECT_DIR/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')"
CHANGELOG_VER="$(sed -n 's/^## *\[*\([0-9][0-9.]*\).*/\1/p' "$PROJECT_DIR/CHANGELOG.md" | head -1)"

if [ "$CARGO_VER" != "$EXPECTED" ] || [ "$CHANGELOG_VER" != "$EXPECTED" ]; then
    cat >&2 <<EOF
ERROR: version mismatch for release $TAG:
  Cargo.toml:    ${CARGO_VER:-<missing>}
  CHANGELOG.md:  ${CHANGELOG_VER:-<missing>}
  tag:           $EXPECTED
Fix: set version = "$EXPECTED" in Cargo.toml, add a ## [$EXPECTED] section
to CHANGELOG.md (metainfo releases regenerate via scripts/generate-metainfo.py),
commit, and re-tag.
EOF
    exit 1
fi

echo "check-versions: OK ($EXPECTED)"
