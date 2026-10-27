# Rhesis

A Qt/KDE desktop writing assistant powered by LanguageTool, written in Rust.

Rhesis runs a local [LanguageTool](https://languagetool.org) HTTP server and communicates with it over loopback to provide grammar and style checking.

## Architecture

A Rust Qt6/KDE desktop client that communicates over HTTP with a local
LanguageTool server (Java, runs on a trimmed JRE inside the flatpak).
LanguageTool uses a bundled fastText binary + model for language
identification.

## Installation

All three flavors are built automatically and attached to every
[release](https://github.com/dimkar3000/rhesis/releases).

### Option 1: Flatpak

Sandboxed, bundles everything, integrates with your software center.

```sh
# download the latest release
wget https://github.com/dimkar3000/rhesis/releases/latest/download/rhesis.flatpak

# install (drop --user for a system-wide install)
flatpak install --user rhesis.flatpak

# run
flatpak run io.github.dimkar3000.rhesis
```

### Option 2: AppImage

Single file, no installation required. Download
`rhesis-<version>-x86_64.AppImage` from the releases page, then:

```sh
chmod +x rhesis-<version>-x86_64.AppImage
./rhesis-<version>-x86_64.AppImage
```

### Option 3: Native install (desktop tarball + runner.sh)

Installs a launcher, desktop entry, bundled JRE and LanguageTool to `/usr`
(global) or `~/.local` (per-user). `runner.sh` automatically downloads the
matching tarball from the latest release; if you placed a
`rhesis-<version>-x86_64[-local].tar.gz` in the current directory, it is
used instead.

```sh
# download the installer
wget https://github.com/dimkar3000/rhesis/releases/latest/download/runner.sh
chmod +x runner.sh

# system-wide install (writes to /, requires root)
sudo ./runner.sh --install

# or per-user install (no root required)
./runner.sh --install local
```

Uninstall with `sudo rhesis --uninstall` for global installations or `rhesis --uninstall`
for a per-user install.

## Completed/Pending Features
- [x] Basic suggestions features
- [x] Flatpak distribution
- [x] I18N - Localization beyond English
- [x] External LanguageTool server - Support for remote LanguageTool instances
- [x] Better UI/UX - Improved syntax highlighting in the editor
- [x] Alternative distribution methods - Native packages beyond Flatpak
- [x] Ability to run in the background, allowing the server to be used by other apps
- [ ] Windows build?

## Local Development

### Prerequisites

- Rust toolchain, CMake, a C++ compiler
- Qt6 and KDE Frameworks development packages. You can follow the [Kirigami Guide from KDE docs](https://develop.kde.org/docs/getting-started/kirigami/) it has most of what you need.
- Java 17+ JDK
- [fastText](https://github.com/facebookresearch/fastText)

### Setup & Run

```sh
# Downloads LanguageTool release, builds fastText, downloads lid.176.ftz
scripts/setup.sh

# Builds and runs the app; LanguageTool server starts automatically
cargo run

# Enable full LanguageTool debug output
RUST_LOG=trace cargo run
```

## Building the Flatpak

### Building

```sh
scripts/build-flatpak.sh
```

The build is **pinned to the Qt of the manifest's runtime**
(`org.kde.Platform//6.11`).  The manifest ships the host-built binary
as-is (`"buildsystem": "simple"`), so the artifacts must be compiled
against exactly the Qt the runtime provides — a bundle built with a
different Qt dies at startup with `version 'Qt_6.x' not found`.

How the pin is achieved (chosen automatically from `runtime-version`
in the manifest, the single source of truth):

- **Host Qt matches the runtime** → artifacts are built locally with
  `scripts/build-common.sh`.
- **Host Qt differs (or is missing)** → artifacts are built inside an
  `org.kde.Sdk//<runtime>` flatpak sandbox, which carries the dev tree
  of exactly that Qt.  No container engine is needed: the sandbox runs
  as your own user, Rust comes from the SDK's `rust-stable` extension,
  and the cargo cache is shared through `~/.cargo`.  Stale
  `build/cmake-build`/`build/target` output from the foreign Qt is
  wiped first, so expect a full release rebuild in this mode.

Once the artifacts are ready, the script generates `cargo-sources.json`
from `Cargo.lock` and runs `flatpak-builder` (`cargo-sources.json` is
not tracked in git — it is generated on every build).  LanguageTool,
fastText, and the lid model are downloaded from their original sources
at build time via the manifest.  The resulting bundle is exported to
`build/artifacts/flatpak/rhesis.flatpak`.

### CI/CD

A single workflow, `.github/workflows/build.yml`, is triggered on
version tags (`v*`) or manually:

- Checks version consistency (`Cargo.toml`/`CHANGELOG.md` vs tag) for
  final releases only, via `scripts/check-versions.sh`.
- The tag format selects the build profile: `v*-*` (pre-release) →
  `release-fast`, stable tag → `release`.
- Builds all three distribution flavors — Flatpak, AppImage, and desktop
  tarballs (global and `--local`) — plus `runner.sh`.
- On version tags, everything is attached to the GitHub release, with
  release notes generated from `CHANGELOG.md` (`scripts/release-body.py`).

The CI runs in a custom container image (`Dockerfile`, `kde-6.11` tag)
pre-installed with the flatpak SDK extensions and appimage tooling.
See [Building/Updating the CI container](#buildingupdating-the-ci-container)
for how to build and push it. The image is only needed by CI — local
flatpak builds pin against the flatpak SDK directly (see
[Building](#building)), no container required.

### Testing

```sh
flatpak run --user --env=RUST_LOG=trace io.github.dimkar3000.rhesis
```

The LanguageTool server output (both stdout and stderr) is forwarded to the
Rust logging system under the `[LanguageTool]:` prefix.

If startup logs `port 2689 is already in use`, a leftover LanguageTool
instance is holding the port — clear it with
`pkill -f "languagetool[-]server.jar"` (the bracketed pattern keeps
pkill from matching itself), then start the app again.

## Scripts

### `scripts/build-flatpak.sh`

Wrapper for building the flatpak. Reads `runtime-version` from the
manifest as its single source of truth, builds the artifacts against a
matching Qt — locally when the host Qt matches, otherwise inside an
`org.kde.Sdk` flatpak sandbox of the pinned runtime (no container
required) — then installs the required flatpak runtimes, runs
`flatpak-builder` and exports `build/artifacts/flatpak/rhesis.flatpak`.

Useful options: `--verbose` (full output), `--clean` (rebuild artifacts
from scratch), `--no-runtimes` (skip runtime install when already
present), `--no-spinner` (plain output).

### `scripts/setup.sh`

Downloads and prepares dependencies for local development:

1. Downloads the LanguageTool distribution ZIP and extracts it to `build/LanguageTool/`
2. Clones and builds `fastText` from source
3. Downloads the `lid.176.ftz` language-identification model

Run this from the project root before doing local development.

### `scripts/run-language-tool.sh`

Starts a standalone LanguageTool HTTP server on the given port (default 8081):
`scripts/run-language-tool.sh [PORT]`. Useful for testing Rhesis against an
"External (remote server)" instance without repackaging; stops with Ctrl+C.

### `scripts/create-jre.sh`

Creates a trimmed Java runtime image using `jlink` for bundling in the flatpak. 
Runs during the flatpak build (in `io.github.dimkar3000.rhesis.json`) after `installjdk.sh`.

Uses a fixed module list in `JLINK_MODULES`. If LanguageTool throws a
`ClassNotFoundException` at runtime, add the missing module here and rebuild.

Using `-verbose:class` argument when starting LanguageTool we can take a list of all the dependencies used.

### `io.github.dimkar3000.rhesis.json`

Flatpak manifest that builds and bundles everything. Sources are defined
inline — LanguageTool is downloaded as an archive, fastText is built from
source via git, and the lid model is fetched as a file. All source URLs
are checksummed for reproducibility.

- **mold**: Fast linker for Rust compilation
- **corrosion**: CMake integration for Rust
- **openjdk**: Full JDK (needed only for jlink)
- **rhesis**: The app itself (Rust, built via CMake/Corrosion)


## Key Components

| Component | Language | Role |
|-----------|----------|------|
| `rhesis` | Rust | Qt6/KDE desktop UI, HTTP client to LanguageTool |
| `LanguageTool/` | Java | Language checking server (HTTP API on :2689) |
| `fastText` | C++ | Language identification (binary, called by LT) |
| `lid.176.ftz` | data | fastText language model (938 KB) |
| `jre/` | Java | Trimmed JRE (48 MB, built by jlink at flatpak time) |
| `server.properties` | config | LanguageTool config (fastText paths, port) |

## Building/Updating the CI container

```sh
podman build -t ghcr.io/dimkar3000/rhesis-ci:kde-6.11 .
podman push ghcr.io/dimkar3000/rhesis-ci:kde-6.11
```

## Licensing

- Rhesis — MIT
- LanguageTool — LGPL 2.1+ 
