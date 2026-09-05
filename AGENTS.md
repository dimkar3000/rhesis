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

- Cargo target dir is `build/target` (not `target`) — set in `.cargo/config.toml`
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

## No tests, no linting

- **Zero tests** exist anywhere in the codebase
- **No Rustfmt config**, **no Clippy config**, **no pre-commit hooks**
- No CI checks for code formatting or linting

## Translations

Qt `.ts` files in `translations/`, compiled to `.qm` at build time. `qt6_add_translation` in `CMakeLists.txt`. Strings use `qsTr()` in QML.

## CI/CD

GitHub Actions workflow (`.github/workflows/build.yml`):

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