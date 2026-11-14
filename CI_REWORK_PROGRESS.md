# CI rework — progress

One flow (`pipeline.yml`) replaces `test.yml` + `build.yml`. Tests are steps
of the common Linux job, right after the cache save; the release path (Linux
release payload, Windows staged payload, five packaging jobs, the GitHub
release) only runs on tag pushes (or a dispatch that asks for it).

## Checklist

- [x] 1. Rename `build-app.yml` → `build-linux.yml` (git mv), internal job id
      `build` → `build-linux`, header comment rewritten, `jobs.build.*` output
      refs updated
- [x] 2. `build-linux.yml` test mode: inline the three test steps from
      test.yml after `Save build cache` (Formatting, ldd gate, Tests);
      release mode: add `Check version consistency` step
- [x] 3. `scripts/build-windows.sh`: `--stage-only` / `--zip-only` /
      `--installer-only` modes, lazy Qt/JDK discovery, staged-payload guards,
      vcredist stays with the installer mode; no-flag local behavior unchanged
      (`bash -n` clean)
- [x] 4. New `pipeline.yml`: common `linux` job; gated `build-linux` +
      `build-windows` (windows: Qt/JDK, cargo cache incl. `target/`, payload
      cache, `payload-cache-key` output); five packagers
      (flatpak/appimage/desktop/windows-zip/windows-installer) each uploading
      one artifact; `release` collects on tags
- [x] 5. Delete `test.yml` and `build.yml`
- [x] 6. AGENTS.md CI/CD rewrite (two files, new graph, act commands) +
      README.md CI/CD section + two stale refs (AGENTS.md linkage bullet,
      `scripts/build-linux.sh` help text)
- [x] 7. Validate: `act --list`, dry-runs (push-to-main, dispatch
      ±package, dispatch+package with a dummy windows platform)
- [ ] 8. GitHub run to validate what act cannot: windows jobs on real
      runners (act 0.2.89 cannot run `windows-latest` — `install-qt-action`'s
      composite steps break under act) and cache round-trips between jobs
      (act shares one workspace).

## act validation results

- `act --list`: 9 jobs in the designed stages (linux → build-linux +
  build-windows → 5 packagers → release); reusable-workflow call expands.
- `act push -n --defaultbranch main`: only `linux` runs — feature/main
  pushes stop after the common job. ✓
- `act workflow_dispatch -n` (no input): only `linux`. ✓
- `act workflow_dispatch -n --input package=true`: gate fires —
  `build-linux` (release mode) and the three Linux packagers run with all
  steps; `build-windows` runs but fails inside `Install Qt` (act limitation:
  composite action steps `$/action` unsupported), and `windows-zip`,
  `windows-installer`, `release` are then correctly suppressed by
  needs-on-failed-dependency propagation. ✓ semantics, not runnable locally.
- Tag-push simulation (`act push --ref refs/tags/v0.1.0`): act cannot
  simulate tag pushes (known limitation); the
  `startsWith(github.ref, 'refs/tags/v')` gate is standard GHA.

## Decisions locked in

- Tests gate artifacts structurally: every packaging job
  `needs` the common `linux` job plus its platform build, so no artifact is
  generated before the tests are green.
- The two platform builds also `needs: [linux]` (as in the approved step
  diagram): on a tag, a red test step stops the release before any release
  binary is built. Trimming `needs` on `build-linux`/`build-windows` would
  parallelize them with the tests at the cost of wasted release builds on
  bad tags.
- Payload handoff windows-build → packaging: `actions/cache`
  (`build/windows-stage` + `build/LanguageTool-6.6.zip`), exact-key restore
  in the two windows packagers — no prefix fallback, stale binaries must
  never be packaged.
- Windows cargo cache includes `target/` (accepted LRU churn risk); it only
  benefits the next `build-windows` run.
- The build job does NOT restore the payload cache: `stage_payload` wipes
  the stage dir anyway, and restoring it only to delete it (plus the
  path-list-version rule) buys nothing — the stage job re-downloads the
  LanguageTool zip, same as the old build.yml did every run.
- `check-versions.sh` lives in the two build jobs (final releases only);
  packagers no longer re-check. `release` stays tags-only;
  `workflow_dispatch` gains a `package` input to exercise the release path
  without a tag.
- main/develop pushes behave like feature branches (common job only).
- Push + PR triggers both kept: a same-repo PR gets duplicate runs
  (different refs, different concurrency groups) — same trade as before.

## Notes

- Same-run freshness: every save key ends in `…-<run id>-<attempt>`;
  consumers restore the exact key their build job reported.
- Cache path lists must be byte-identical (paths + order) between each save
  and every restore of that cache — `actions/cache` hashes the list into
  the cache version.
- Artifact names: `flatpak`, `appimage`, `desktop`, `windows-zip`,
  `windows-installer` — one per packaging job; `release` collects all five
  plus `runner.sh` from the checkout.
- Everything is uncommitted on top of `809c9b92` ("small cleanup").
