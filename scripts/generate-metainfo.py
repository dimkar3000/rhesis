#!/usr/bin/env python3
"""Regenerate the <releases> block of the AppStream metainfo file from CHANGELOG.md.

Single-source rule: CHANGELOG.md is the only hand-edited changelog.
Release dates come from git tags (``git log -1 --format=%cs vX.Y.Z``).

Release / pre-release rules:
  - The output is a function of the *commit*, not of the triggering tag name.
  - If any final tag (vX.Y.Z) points at the resolved commit -> release build:
    finals only, even when pre-release tags co-point at the same commit.
  - Else, if a pre-release tag (vX.Y.Z-suffix) applies -> pre-release build:
    a transient vX.Y.Z-suffix entry is prepended (build output only, the
    committed file keeps finals). Content comes from the CHANGELOG.md
    [X.Y.Z-suffix] section if present, else the [X.Y.Z] section.

Usage:
  scripts/generate-metainfo.py [--release-tag v0.4.0-test1] [--no-validate]
"""

import argparse
import os
import re
import subprocess
import sys
from datetime import date
from xml.sax.saxutils import escape

PROJECT_DIR = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

HEADING_RE = re.compile(
    r"^##\s+\[?(\d+\.\d+\.\d+(?:-[0-9A-Za-z.\-]+)?)\]?"
    r"\s*(?:-\s*(\d{4}-\d{2}-\d{2}))?\s*$"
)
BULLET_RE = re.compile(r"^\s*[-*]\s+(.*\S)\s*$")
FINAL_TAG_RE = re.compile(r"^v?(\d+\.\d+\.\d+)$")
PRERELEASE_TAG_RE = re.compile(r"^v?(\d+\.\d+\.\d+-.+)$")
DATE_RE = re.compile(r"^\d{4}-\d{2}-\d{2}$")

BEGIN_MARKER = "<!-- BEGIN GENERATED RELEASES - do not edit, from CHANGELOG.md -->"
END_MARKER = "<!-- END GENERATED RELEASES -->"


def warn(msg):
    print(f"generate-metainfo: warning: {msg}", file=sys.stderr)


def git(args, repo):
    try:
        out = subprocess.run(
            ["git", "-C", repo] + args,
            capture_output=True, text=True, check=False,
        )
    except OSError:
        return None
    if out.returncode != 0:
        return None
    return out.stdout.strip()


def parse_changelog(path):
    """Return {version: {"bullets": [...], "paragraphs": [...], "date": str|None}}."""
    sections = {}
    current = None
    with open(path, encoding="utf-8") as f:
        for raw in f:
            line = raw.rstrip("\n")
            m = HEADING_RE.match(line.strip())
            if m:
                current = m.group(1)
                sections[current] = {
                    "bullets": [], "paragraphs": [], "date": m.group(2),
                }
                continue
            if current is None:
                continue
            stripped = line.strip()
            if not stripped:
                continue
            b = BULLET_RE.match(line)
            if b:
                sections[current]["bullets"].append(b.group(1).strip())
            else:
                sections[current]["paragraphs"].append(stripped)
    return sections


def version_key(version):
    base = version.split("-")[0]
    return tuple(int(p) for p in base.split("."))


def existing_release_dates(metainfo_text):
    found = {}
    for m in re.finditer(
        r'<release\s+version="([^"]+)"\s+date="([^"]+)"', metainfo_text
    ):
        found[m.group(1)] = m.group(2)
    return found


def existing_details_base(metainfo_text):
    m = re.search(r'<url type="details">([^<]+)</url>', metainfo_text)
    if m:
        return m.group(1).split("/releases/tag/")[0]
    return "https://github.com/dimkar/rhesis"


def render_release(display_version, bullets, paragraphs, date_str, url):
    lines = [
        f'    <release version="{escape(display_version)}" date="{date_str}">',
        f'      <url type="details">{escape(url)}</url>',
        "      <description>",
        f"        <p>Version {escape(display_version)}</p>",
    ]
    for p in paragraphs:
        lines.append(f"        <p>{escape(p)}</p>")
    if bullets:
        lines.append("        <ul>")
        for b in bullets:
            lines.append(f"          <li>{escape(b)}</li>")
        lines.append("        </ul>")
    lines.append("      </description>")
    lines.append("    </release>")
    return "\n".join(lines)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--changelog", default=os.path.join(PROJECT_DIR, "CHANGELOG.md"))
    parser.add_argument(
        "--metainfo",
        default=os.path.join(
            PROJECT_DIR, "io.github.dimkar3000.rhesis.metainfo.xml"
        ),
    )
    parser.add_argument(
        "--release-tag",
        default=os.environ.get("GITHUB_REF_NAME"),
        help="Tag being built (defaults to $GITHUB_REF_NAME). "
        "Non-tag values such as branch names are ignored.",
    )
    parser.add_argument("--repo", default=PROJECT_DIR)
    parser.add_argument("--no-validate", action="store_true")
    args = parser.parse_args()

    sections = parse_changelog(args.changelog)
    if not sections:
        print("generate-metainfo: error: no version sections found in "
              f"{args.changelog}", file=sys.stderr)
        return 1

    with open(args.metainfo, encoding="utf-8") as f:
        metainfo_text = f.read()
    old_dates = existing_release_dates(metainfo_text)
    url_base = existing_details_base(metainfo_text)

    tag = (args.release_tag or "").strip()
    if tag.startswith("refs/tags/"):
        tag = tag[len("refs/tags/"):]

    commit = git(["rev-parse", "HEAD"], args.repo)
    tag_commit = git(["rev-parse", f"{tag}^{{commit}}"], args.repo) if tag else None
    if tag and not tag_commit:
        # Branch names (workflow_dispatch) and unknown refs land here: ignore.
        tag = None
    resolve_commit = tag_commit or commit
    tags_at = git(["tag", "--points-at", resolve_commit], args.repo)
    tags_at = tags_at.splitlines() if tags_at else []

    final_at_commit = sorted(
        {m.group(1) for t in tags_at if (m := FINAL_TAG_RE.match(t))}
    )
    prerelease_at_commit = sorted(
        {m.group(1) for t in tags_at if (m := PRERELEASE_TAG_RE.match(t))},
        key=version_key,
    )

    prerelease_version = None
    if not final_at_commit:
        tag_m = PRERELEASE_TAG_RE.match(tag) if tag else None
        if tag_m and tag_m.group(1) in prerelease_at_commit:
            prerelease_version = tag_m.group(1)
        elif tag_m:
            # Explicit tag given but not pointing here (e.g. shallow clone
            # without tags): trust it, resolve its own date below.
            prerelease_version = tag_m.group(1)
        elif prerelease_at_commit:
            prerelease_version = prerelease_at_commit[-1]
            if len(prerelease_at_commit) > 1:
                warn(f"multiple pre-release tags at {resolve_commit}, "
                     f"using {prerelease_version}")

    # Final entries: CHANGELOG sections without a pre-release suffix,
    # newest first.
    finals = sorted(
        (v for v in sections if "-" not in v), key=version_key, reverse=True
    )
    entries = []
    for version in finals:
        sec = sections[version]
        tag_date = git(["log", "-1", "--format=%cs", f"v{version}"], args.repo)
        if tag_date and DATE_RE.match(tag_date):
            date_str = tag_date
        elif sec["date"]:
            date_str = sec["date"]
        elif version in old_dates:
            date_str = old_dates[version]
        else:
            date_str = date.today().isoformat()
            warn(f"no git tag v{version} found, using today ({date_str})")
        entries.append(
            render_release(
                version, sec["bullets"], sec["paragraphs"], date_str,
                f"{url_base}/releases/tag/v{version}",
            )
        )

    if prerelease_version:
        base = prerelease_version.split("-")[0]
        sec = sections.get(prerelease_version, sections.get(base))
        if sec is None:
            warn(f"no CHANGELOG section for {prerelease_version} or {base}, "
                 "skipping pre-release entry")
        else:
            pre_tag = next(
                (t for t in ([tag] if tag else []) + tags_at
                 if PRERELEASE_TAG_RE.match(t)
                 and PRERELEASE_TAG_RE.match(t).group(1) == prerelease_version),
                f"v{prerelease_version}",
            )
            pre_date = git(["log", "-1", "--format=%cs", pre_tag], args.repo)
            if not (pre_date and DATE_RE.match(pre_date)):
                pre_date = git(["log", "-1", "--format=%cs", resolve_commit],
                               args.repo)
            if not (pre_date and DATE_RE.match(pre_date)):
                pre_date = date.today().isoformat()
                warn(f"could not resolve date for {pre_tag}, using today")
            entries.insert(
                0,
                render_release(
                    prerelease_version,
                    sec["bullets"], sec["paragraphs"], pre_date,
                    f"{url_base}/releases/tag/v{prerelease_version}",
                ),
            )

    block = "\n".join(entries)
    generated = f"  {BEGIN_MARKER}\n{block}\n  {END_MARKER}"

    if BEGIN_MARKER in metainfo_text and END_MARKER in metainfo_text:
        new_text = re.sub(
            r"  <!-- BEGIN GENERATED RELEASES.*?END GENERATED RELEASES -->",
            lambda _: generated,
            metainfo_text,
            count=1,
            flags=re.DOTALL,
        )
    else:
        # First run: replace the whole <releases> body and insert markers.
        new_text, n = re.subn(
            r"(<releases>).*?(</releases>)",
            lambda m: f"{m.group(1)}\n{generated}\n  {m.group(2)}",
            metainfo_text,
            count=1,
            flags=re.DOTALL,
        )
        if n == 0:
            print("generate-metainfo: error: no <releases> block found in "
                  f"{args.metainfo}", file=sys.stderr)
            return 1

    if new_text != metainfo_text:
        with open(args.metainfo, "w", encoding="utf-8", newline="\n") as f:
            f.write(new_text)
        print(f"generate-metainfo: updated releases in {args.metainfo}")
    else:
        print("generate-metainfo: already up to date")

    if not args.no_validate:
        import shutil
        if shutil.which("appstreamcli"):
            out = subprocess.run(
                ["appstreamcli", "validate", "--no-net", args.metainfo],
                capture_output=True, text=True, check=False,
            )
            output = (out.stdout + out.stderr).strip()
            if out.returncode != 0:
                warn(f"appstreamcli validate reported issues:\n{output}")
            elif output:
                print(output)
    return 0


if __name__ == "__main__":
    sys.exit(main())
