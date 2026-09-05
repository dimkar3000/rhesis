#!/usr/bin/env python3
"""Generate GitHub release notes from CHANGELOG.md for a given version.

Usage:
    python3 scripts/release-body.py CHANGELOG.md <version>

<version> accepts an optional leading 'v' (e.g. 'v0.4.0' from
$GITHUB_REF_NAME). Pre-release versions fall back to their base section
(e.g. 'v0.4.0-test1' -> '## [0.4.0]') when no exact section exists,
mirroring generate-metainfo.py.

Exits non-zero when no section matches, so CI fails loudly instead of
publishing empty release notes.
"""

import re
import sys

HEADING_RE = re.compile(r"^##\s+\[?([0-9][0-9A-Za-z.\-]*)\]?\s*(?:-\s*(.*))?\s*$")
BULLET_RE = re.compile(r"^\s*[-*]\s+(.*\S)\s*$")


def section_lines(path, version):
    candidates = [version]
    base = version.split("-")[0]
    if base != version:
        candidates.append(base)

    with open(path, encoding="utf-8") as f:
        lines = f.read().splitlines()

    for wanted in candidates:
        body = []
        inside = False
        for line in lines:
            m = HEADING_RE.match(line.strip())
            if m:
                if inside:
                    break
                if m.group(1) == wanted:
                    inside = True
                continue
            if inside:
                body.append(line)
        if inside:
            return wanted, body
    return None, []


def main(argv):
    if len(argv) != 3:
        print(f"Usage: {argv[0]} CHANGELOG.md <version>", file=sys.stderr)
        return 2
    path, version = argv[1], argv[2].strip().lstrip("v")

    wanted, body = section_lines(path, version)
    if wanted is None:
        print(f"release-body: error: no '## [{version}]' section in {path}",
              file=sys.stderr)
        return 1

    out = [f"Version {wanted}"]
    for line in body:
        stripped = line.strip()
        if not stripped:
            continue
        bullet = BULLET_RE.match(line)
        out.append(f"- {bullet.group(1).strip()}" if bullet else stripped)
    print("\n".join(out))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
