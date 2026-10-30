# Rhesis — Agent Guide

## Setup (required before first build)

```sh
scripts/setup.sh          # downloads LanguageTool, builds fastText, downloads language model
```

## Build

CMake orchestrates the Rust build (not plain `cargo build`). Two profiles:

```sh
# default (thin LTO, fast iteration)
cargo run

# full release (LTO, stripped)
RUST_BUILD_TYPE=release scripts/build-common.sh
```

- Cargo target dir is `build/target` (not `target`) — set in `.cargo/config.toml` (gitignored; CI sets `CARGO_TARGET_DIR` explicitly)
- Uses `mold` linker on Linux (same config file)
- Dev run: `RUST_LOG=trace cargo run` for full LanguageTool debug output

## Architecture

| Path | Role |
|------|------|
| `src/main.rs` | Entrypoint, `#[tokio::main]`, creates QApplication + QQmlApplicationEngine |
| `src/interop/` | CXX-Qt FFI bridge (`bridge.rs`), QML UI (`qml/`), custom highlighter |
| `src/languagetool/` | HTTP client to local LanguageTool server, `client.rs`, `models.rs`, `service.rs` |

- UI: Qt6/KDE Kirigami via CXX-Qt (Rust↔C++↔QML), QML files in `src/interop/qml/`
- Rust app POSTs text to `localhost:2689/v2/check`, renders grammar suggestions with `CustomHighlighter`
- LanguageTool runs as a child process; messaging is async via tokio channels (debounced 300ms)

## Testing

One command runs everything: `cargo test`. QML suites included — the
`qml-suites` target launches the `rhesis-ui-runner` binary, so `cargo test`
covers the full stack from DTO parsing to painted highlights.

Test targets are declared explicitly in `Cargo.toml` (`[[test]]`): test files
no longer live directly in `tests/`, so cargo cannot auto-discover them. A new
file under `tests/unit/` or `tests/e2e/` needs its `[[test]]` entry or it
silently never runs.

```sh
# The whole stack (unit + E2E + QML suites; ~20s)
cargo test

# One tier, while iterating
cargo test --lib                                     # in-module tests (~22)
cargo test --test languagetool-service               # worker loop
cargo test --test qml-suites                         # tests/ui/*.qml
cargo test --test ui-boot                            # real Root.qml boot
cargo test --test single-instance-e2e                # two real launches
```

Layout:

| Path | Contents |
|------|----------|
| `src/**` | `#[cfg(test)]` modules for private-logic tests (worker, autostart, provider, highlighter) |
| `tests/unit/` | Public-API unit tiers: client, models, recommendation, helper, guard |
| `tests/e2e/` | Whole-process tiers: `ui_boot.rs`, `single_instance_e2e.rs`, `qml_suites.rs` |
| `tests/ui/` | Qt Quick Test suites (`tst_*.qml`) + `harness.js` |
| `tests/common/` | Shared sandbox/seed helpers, symlinked into the E2E tier (`tests/e2e/common`) |

Nothing is feature-gated: there is no `ui-harness` feature, and QuickTest is
compiled unconditionally by `build.rs` (the shipped binary simply never
references it).

Gates after every change:

- `cargo fmt`
- `cargo clippy --all-targets` — 0 warnings
- `cargo test` — clean, no skips
- shipped binary must not link QuickTest: `ldd build/target/debug/rhesis | grep -c QuickTest` = `0`

### Hermeticity

Every tier is offline and side-effect free by construction, so no test needs
a LanguageTool installation or a running server:

- Worker requests are answered by the recorded fixture (`RHESIS_FAKE_LT`,
  resolved by `CompletionProvider::chosen`) — never java, never the network.
- `tests/common/mod.rs` (in-process tiers) and `src/ui_tests.rs :: isolate_environment`
  (QML runner) redirect every XDG home into `build/ui-tests/<tier>-env`, so
  autostart files and `QSettings` never touch the real user configuration.

### Tier notes

- **Boot E2E** (`tests/e2e/ui_boot.rs`): one `#[test]`, in-process, mirroring
  production boot exactly. QML access via C++ probes (no bridge changes):
  expressions evaluated in Root.qml's scope; plus `findChild` by objectName,
  property writes, window grab, last-error capture. Pump =
  `QEventLoop::process_all_events_until::<()>`; predicate returns
  `Result<Option<String>, String>` — `Ok(None)` settled, `Ok(Some(state))` not
  settled with diagnostic; timeout panic carries the last state.
- **QML suites** (`tests/e2e/qml_suites.rs`): Quick Test owns `QGuiApplication`
  and parses its own argv, so it runs in a subprocess
  (`rhesis-ui-runner`), not in-process. Fails if the runner exits non-zero
  *or* reports no suite execution. The runner installs the app's
  `org.kde.desktop` style and embedded icon theme before Quick Test starts,
  otherwise screenshots show the fallback Qt style (split-looking switches,
  empty icon buttons).
- **Screenshots**: `build/tests/<suite>_<test>[_<label>]_<PASS|FAIL>.png`,
  truncated at every runner launch (the directory always mirrors the latest
  run). `<test>` is the QML function that called `capture`
  (`qtest_results.functionName`), so every image names its source test;
  `_label` is only needed when one test takes two shots. `Harness.step`
  captures *before* asserting, because a failing `verify()` aborts the test
  function — otherwise exactly the failing steps would leave no image
  behind. `capture` grabs once to force a frame, waits
  for the UI thread to polish, grabs again, and fails the test on a null or
  still-blank image: a blank `_PASS` hides the very state it documents.
- **Single instance** (`tests/e2e/single_instance_e2e.rs`): two real binary
  launches; needs the socket preconditions from `tests/common`.
- Lessons: inverted settle contract (fixed), QQmlSettings 500ms write timer
  (poll-until-file-contains pattern), phantom generic turbofish,
  rerun-if-changed gap in build.rs, `grabImage` needs a `QQuickItem*`
  (`window.contentItem`, not the window — Qt ≥ 6.12 rejects the coercion),
  `verify()` returns nothing and aborts on failure (never `var ok = verify(...)`),
  an item reached by `objectName` must be resolved from a live parent
  (`Harness.findItem(page, ...)`) — `page.someId` is `undefined`, layer
  push/pop animate with a render-thread `YAnimator` (the settings suite
  zeroes the transitions, or a capture catches the page half-slid), and
  `findAllItems` must push children in reverse to keep child order (`rows[3]`
  otherwise addresses the wrong rule row).

### Visibility rule

Tests do not exist to expose internals. A helper stays private and its tests
stay in the same module's `#[cfg(test)]` block. Moving a test to `tests/`
requires the helper to be genuinely part of the module's API — "the test needs
it" is not a reason to write `pub`.

## Translations

Qt `.ts` files in `translations/`, compiled to `.qm` at build time. `qt6_add_translation` in `CMakeLists.txt`. Strings use `qsTr()` in QML.
- Local builds run `lupdate` (new `qsTr()` strings land in the `.ts`) then `lrelease`; the build fails loudly if `lupdate` drops messages or a `.qm` comes out as a stub.
- CI sets `SKIP_LUPDATE=1`: it only ever compiles the committed `.ts` files, never mutates them (the container's `lupdate` lacks QML support).

## CI/CD

Three workflows, deliberately separate: one shared build, one decides
correctness, one packages releases.

`build-app.yml` — the common build (`workflow_call`), called first by both
other workflows:

- `test` mode: `cargo test --no-run` (every test target, the app and the QML
  runner, compiled once); `release` mode: `scripts/build-common.sh` (app,
  fastText, language model, LanguageTool, trimmed JRE)
- Restores `rhesis-<os>-<profile>-<Cargo.lock hash>` (plus fallback prefixes)
  and saves under `<that key>-<run id>-<attempt>`: the save key must be unique
  per run, otherwise a stable key gets pinned to the first commit with that
  lockfile and the consumer would restore a build from older sources.
  Consumers restore the exact `cache-key` output, so nothing is rebuilt twice.
- Cached paths: `~/.cargo/{registry,git}` plus `build/target` in test mode;
  `build/cmake-build` (CMake passes `--target-dir` there, so that is where
  the release Cargo tree lives), `build/artifacts`, fastText, LanguageTool
  and the language model in release mode. A cache miss is safe: the scripts
  rebuild.
- `.cargo/` is gitignored, so CI has no target-dir/linker config; test jobs
  set `CARGO_TARGET_DIR=build/target` explicitly to match the cached path.
- CI sets `CARGO_INCREMENTAL=0` and disables debug info to keep the cache
  small; every consuming job repeats that env, otherwise cargo fingerprints
  change and the restored tree is rebuilt.

`test.yml` — the only place tests run in CI:

- Triggers: pushes to `main`/`develop`, PRs into them, manual dispatch
- Gates: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test -- --skip qml_suites_pass`
- The QML page suites (`tests/ui/*.qml`) are local-only: their purpose is
  browser-free screenshots for manual inspection, so CI skips that one
  target. The boot E2E also skips its evidence screenshot when `CI` is set,
  so a CI run writes no images at all. Run the suites locally with
  `cargo test --test qml-suites`.
- Custom Docker image (`ghcr.io/<owner>/rhesis-ci:kde-6.11`) — has Qt 6.11
  QuickTest, the `QtTest` QML module, `org.kde.desktop` style, offscreen
  platform plugin and Kirigami (verified by running the whole suite there).
- Validate workflow edits locally with `act` (`act push -W
  .github/workflows/test.yml --defaultbranch main`). On a podman-only host,
  start the user socket and point act at it:
  `DOCKER_HOST=unix://$XDG_RUNTIME_DIR/podman/podman.sock`. `act` supports
  local reusable workflows and `actions/cache`, so both jobs run for real;
  note it shares one workspace across jobs, so this validates the YAML and
  the actions, not the cache round-trip between jobs.

`build.yml` — releases only:

- Triggers: tags matching `v*` or manual dispatch
- Custom Docker image (`ghcr.io/<owner>/rhesis-ci:kde-6.11`)
- Tag format determines profile: `v*-*` (pre-release) → `release-fast`, stable tag → `release`
- Builds Flatpak + AppImage + desktop tarballs (`build-desktop.sh`, global and `--local`), uploads to release on version tags
- Release includes `runner.sh` alongside the desktop tarballs for download-and-install

## Releasing

- `CHANGELOG.md` is the single source of truth for release notes — `## [X.Y.Z] - YYYY-MM-DD` sections with the pinned release date. The `<releases>` block in `io.github.dimkar3000.rhesis.metainfo.xml` is committed empty (markers only) and regenerated from the CHANGELOG on every build (`scripts/generate-metainfo.py`, dates from git tags with CHANGELOG dates as fallback) — never hand-edit it.
- Before tagging: add the `## [X.Y.Z] - <date>` section, set the same version in `Cargo.toml`, and run `python3 scripts/generate-metainfo.py` to sanity-check the output (CI rebuilds it anyway; commit only the CHANGELOG/Cargo changes).
- Tagging: final `vX.Y.Z` (CI fails the build if `Cargo.toml`/`CHANGELOG.md` disagree with the tag — check locally with `./scripts/check-versions.sh --tag vX.Y.Z`); pre-release `vX.Y.Z-*` embeds the pre-release version in build artifacts transiently and skips the version check. A commit tagged as both is treated as a release.
- GitHub release notes come straight from the CHANGELOG (`scripts/release-body.py CHANGELOG.md <tag>`); CI checks out full history (`fetch-depth: 0`) so historic release dates resolve from git tags.

## QML language server

Config in `.qmlls.ini`:
```ini
buildDir=build/target/cxxqt/qml_modules
no-cmake-calls=true
```