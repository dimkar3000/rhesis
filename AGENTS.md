# Rhesis — Agent Guide
You are a senior software engineer with rust expertize. Always optimize code for readablity and write comments only when they matter. Never write comments that references other files. Comments should methion the code the leave near to and explain any not obvious invariance that needs to be kept or assumptions that are made. Excessive commenting is bad.

## Always 
- Don't over comment. Only comment if there is a code invartiance that needs to be kept that is not obvious and only after asking for input
- Doc comments are allowed but they should describe the current state of the code and they should not be excesive.

## Setup (required before first build)

```sh
scripts/setup.sh          # downloads LanguageTool, builds fastText, downloads language model
```

## Build

CMake orchestrates the packaging build (not plain `cargo build`). Two profiles:

```sh
# dev: build BOTH binaries side by side first (the client spawns its
# sibling daemon; without it the UI boots into Disconnected and warns)
cargo build --workspace
cargo run

# full release (LTO, stripped) — builds everything payloads need
RUST_BUILD_TYPE=release scripts/build-common.sh
```

- Cargo target dir is `build/target` (not `target`) — set in `.cargo/config.toml` (gitignored; CI sets `CARGO_TARGET_DIR` explicitly)
- Uses `mold` linker on Linux (same config file)
- Dev run: `RUST_LOG=trace cargo run` for connection diagnostics; the spawned daemon logs to nowhere (detached stdio) — `RUST_LOG=trace ./build/target/debug/rhesis-service` watches it instead
- Packaging builds both binaries: CMake's `rhesis_build` runs the scoped
  `cargo build -p rhesis --bin rhesis --bin rhesis-service`; the
  QML-test runner binary (`rhesis-ui-runner`) is built by `cargo test` when needed

## Architecture

| Path | Role |
|------|------|
| `crates/rhesis-protocol` | Wire contract for the daemon↔client socket (`DaemonMsg`, `Recommendation`, postcard + `u32` length prefix); the client's request enum lives in `crates/rhesis-qt-client/src/connection.rs`, the daemon parses it via a mirror in `crates/rhesis-service/src/session.rs` (same variants, same order) |
| `crates/rhesis-constants` | Contract constants shared across crates — wire version (`PROTOCOL`), service socket name, factory LanguageTool port. Crate-internal knobs never move here |
| `crates/rhesis-settings` | The settings store the daemon owns (atomic writes, camelCase keys) plus the `Settings` type itself — the store schema and the wire snapshot in one definition |
| `crates/rhesis-service` | Headless daemon library: owns the LanguageTool server + all settings, one client session (`src/session.rs`, `daemon.rs`) |
| `crates/rhesis-platform` | OS surface (single-instance, autostart, process helpers, config paths, translations dir) — no Qt, no HTTP |
| `crates/rhesis-languagetool` | LanguageTool worker library (used by the daemon): server lifecycle, HTTP client; the completion boundary is the `CompletionProvider` trait (injected at `run()`; tiers implement it with fixture providers) |
| `crates/rhesis-qt-client` | The Qt client: CXX-Qt FFI bridge (`src/interop/bridge.rs`), QML UI (`src/interop/qml/`), custom highlighter, connection loop (`src/connection.rs`), client constants, boot sequence (`src/run.rs`), the in-process fixture daemon (`src/fake_daemon.rs`) and the Quick Test machinery (`src/ui_tests.rs` + C++ probes). Owns its `build.rs` (QmlModule, QuickTest probes); the qrc and icons live in `assets/` |
| `src/rhesis-qt-client.rs` | Client entrypoint shim: the GUI-subsystem attribute + `rhesis_qt_client::run()` |
| `src/rhesis-server.rs` | Headless daemon entrypoint: logging + the real `LanguageToolClient` provider into `deamon_run`; must not reference `rhesis_qt_client` (headless rule below) |
| `src/rhesis-ui-runner.rs` | Quick Test runner shim over `rhesis_qt_client::ui_tests::run_suites` |

- UI: Qt6/KDE Kirigami via CXX-Qt (Rust↔C++↔QML), QML files in `crates/rhesis-qt-client/src/interop/qml/`
- The UI is a thin client: the daemon binary owns the LanguageTool server and the
  settings store; the Qt process reads settings and drives the server only
  over a local socket (`interprocess` 2: abstract UDS / named pipe,
  `RHESIS_SERVICE_SOCKET` test seam) — one client slot, strict `Hello`
  handshake, replay-then-relay
- UI renders grammar suggestions with `CustomHighlighter`; text checks cross
  the wire as `Check` frames (the daemon debounces)
- LanguageTool still runs as a child of the daemon; daemon messaging is
  threaded-sync over crossbeam channels

## Testing

```sh
cargo test --workspace
```

Test targets are declared explicitly in `Cargo.toml` (`[[test]]`): test files
live in subdirectories (crate-local
`tests/unit/`), so cargo cannot auto-discover them. A new test file needs
its `[[test]]` entry or it silently never runs.

```sh
# One tier, while iterating
cargo test -p rhesis-qt-client --lib                   # in-module tests (Qt tier)
cargo test -p rhesis-languagetool --lib                # worker/provider internals
cargo test -p rhesis-platform --lib                    # autostart layout logic
cargo test -p rhesis-service --lib                     # daemon store/session internals
cargo test --workspace --test languagetool-service     # worker loop
cargo test --workspace --test daemon-session           # daemon↔client wire, store
cargo test --workspace --test qml-suites               # QML page suites
cargo test --workspace --test ui-boot                  # real Root.qml boot
```

Layout:

| Path | Contents |
|------|----------|
| `crates/rhesis-qt-client/src/**` | `#[cfg(test)]` modules for private-logic tests (highlighter fan-out/spy sink, connection backoff, snapshot parse→QVariant) |
| `tests/e2e/ui_boot.rs` | In-process boot tier, root-owned because it spawns the root `rhesis` bin (`CARGO_BIN_EXE_rhesis`); Qt crates sit in root `[dev-dependencies]` so they stay out of the shipped binaries' linkage |
| `crates/rhesis-qt-client/tests/ui/` | Quick Test page suites (run through the runner-subprocess tier) |
| `tests/e2e/common.rs` | Shared sandbox helpers, used by the boot tier (`mod common;`) |
| `tests/e2e/qml_suites.rs` | The runner-subprocess tier, root-owned because the runner bin is a root package bin |
| `crates/rhesis-service` | daemon-session tier under `tests/unit/` (wire semantics against a real store) |
| `crates/rhesis-languagetool` | `src/completion_provider.rs` in-module tests + tiers `languagetool_{client,dto,service}` under `tests/unit/` |
| `crates/rhesis-platform` | `autostart.rs` in-module tests + `single-instance` tier under `tests/unit/` |

Nothing is feature-gated: there is no `ui-harness` feature, and QuickTest is
compiled unconditionally by the `rhesis-qt-client` build script (the shipped binary
simply never references it — verified by the gate below).

Gates after every change:

- `cargo fmt`
- `cargo clippy --workspace --all-targets` — 0 warnings
- `cargo test --workspace` — clean, no skips
- shipped binaries' linkage: `ldd build/target/debug/rhesis | grep -c QuickTest` = `0`, and `ldd build/target/debug/rhesis-service` shows no Qt (enforced in CI's test.yml; the client's QuickTest gate stays a local check)

### Headless rule

The daemon bin (`src/rhesis-server.rs`) shares the root package with the
Qt client deps, but stays Qt-free by never referencing `rhesis_qt_client`: any use —
even a plain const — pulls the C++ bridge's `#[link]` registrations in, and
libQt6* lands in the daemon's NEEDED list (invisible on the rustc command
line; verified under bfd, which defaults to --no-as-needed). Its imports stay
on the service stack (`rhesis-service`, `rhesis-languagetool`,
`rhesis-constants`). CI's "Daemon binary links no Qt" ldd step fails the run
otherwise; the daemon is the process that may run with no UI around, and
every flavor ships it next to the client for that purpose.

### Hermeticity

Every tier is offline and side-effect free by construction, so no test needs
a LanguageTool installation or a running server:

- The completion provider is injected at `run()` via the `CompletionProvider`
  trait (see `crates/rhesis-languagetool/src/completion_provider.rs`): the
  live client answers `true` for `manages_local_server` and owns the JVM
  lifecycle; tier fakes answer `false` and every start/verify completes
  instantly. The fake-vs-real choice never crosses a process boundary — no
  test spawns a daemon binary, no environment variable carries it, and there
  are no feature gates or re-exports as substitutes. The daemon itself never
  builds a provider; its caller does.
- The Qt tiers never own a worker: they point their clients at the in-process
  fake daemon (`crates/rhesis-qt-client/src/fake_daemon.rs`), whose socket name is
  pinned through `RHESIS_SERVICE_SOCKET` ahead of every client (the env pin
  also disables detached-binary spawning in `connection.rs`).
- `tests/e2e/common.rs` (the boot tier's sandbox helpers)
  and `crates/rhesis-qt-client/src/ui_tests.rs :: isolate_environment` (QML runner)
  redirect every XDG home into `build/ui-tests/<tier>-env`, so autostart files
  and every configuration path stay in the sandbox.

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
- **Single instance**: deleted its whole-process tier (`tests/e2e/single_instance_e2e.rs`).
  The guard *logic* keeps its platform-crate unit tests; the boot-order wiring
  (guard bound before UI, stand-down exits without a UI) is untested — the
  component-test trade stated to the user.
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

- `test` mode: `cargo test --workspace --no-run` (every test target, the app and the QML
  runner, compiled once) plus `cargo clippy --workspace --all-targets`, so the lint
  pass's check-mode artifacts are cached with the build; `release` mode:
  `scripts/build-common.sh` (app, fastText, language model, LanguageTool,
  trimmed JRE)
- Restores `rhesis-<os>-<profile>-<Cargo.lock hash>` (plus fallback prefixes)
  and saves under `<that key>-<run id>-<attempt>`: the save key must be unique
  per run, otherwise a stable key gets pinned to the first commit with that
  lockfile and the consumer would restore a build from older sources.
  Consumers restore the exact `cache-key` output, so nothing is rebuilt twice.
- Cached paths: `~/.cargo/{registry,git}` plus `build/target` in test mode;
  `build/cmake-build` (CMake passes `--target-dir` there, so that is where
  the release Cargo tree lives), `build/artifacts`, fastText, LanguageTool
  and the language model in release mode. A cache miss is safe: the scripts
  rebuild. Consumers must list the same paths in the same order—
  `actions/cache` hashes the path list into the cache version, so reordering
  is a guaranteed miss even with the right key.
- `.cargo/` is gitignored, so CI has no target-dir/linker config; test jobs
  set `CARGO_TARGET_DIR=build/target` explicitly to match the cached path.
- CI sets `CARGO_INCREMENTAL=0` and disables debug info to keep the cache
  small; every consuming job repeats that env, otherwise cargo fingerprints
  change and the restored tree is rebuilt.

`test.yml` — the only place tests run in CI:

- Triggers: pushes to `main`/`develop`, PRs into them, manual dispatch
- Gates: `cargo fmt --check`, the "Daemon binary links no Qt" check
  (`ldd build/target/debug/rhesis-service` must show no Qt/QuickTest — the
  headless rule above), and `cargo test --workspace -- --skip qml_suites_pass`;
  clippy runs in the shared build (test mode) because its check-mode
  artifacts are a different compiler pipeline from the test binaries and
  must be cached there, or every run rechecks the whole dependency graph.
- The QML page suites (`crates/rhesis-qt-client/tests/ui/*.qml`) are local-only: their
  purpose is browser-free screenshots for manual inspection, so CI skips that
  one target. The boot E2E also skips its evidence screenshot when `CI` is
  set, so a CI run writes no images at all. Run the suites locally with
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
  the actions, not the cache round-trip between jobs. The release packaging
  job uploads artifacts, which needs act's local artifact server:
  `act workflow_dispatch -W .github/workflows/build.yml --artifact-server-path /tmp/act-artifacts`.

`build.yml` — releases only:

- Triggers: tags matching `v*` or manual dispatch
- Custom Docker image (`ghcr.io/<owner>/rhesis-ci:kde-6.11`)
- All packaged builds use the `release` profile; the tag format only marks
  pre-releases: `v*-*` skips the version check (see Releasing)
- Builds Flatpak + AppImage + desktop tarballs via one `build-linux` step (wrapper script `scripts/build-linux.sh` around `build-flatpak.sh`, `build-appimage.sh` and `build-desktop.sh` — global and `--local`), and the Windows installer + zip via `scripts/build-windows.sh`; every flavor ships both binaries (`rhesis` client + `rhesis-service` daemon in one `bin/`; the daemon links no Qt), uploads to release on version tags
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