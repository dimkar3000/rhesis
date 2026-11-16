# Rhesis

A Qt/KDE desktop writing assistant powered by LanguageTool, written in Rust.

Rhesis hosts a local [LanguageTool](https://languagetool.org) HTTP server inside a headless daemon and talks to it over loopback to provide grammar and style checking.

## Architecture

Two binaries: a headless daemon (`rhesis-service`) that owns the LanguageTool
server and all settings, and the Qt6/KDE desktop client (`rhesis`) which
mirrors settings, submits text checks and renders suggestions over a local
socket to it. LanguageTool (Java, on a trimmed JRE inside the packages) uses
a bundled fastText binary + model for language identification. The daemon
outlives the client, so the server keeps running when the window closes.

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

### Option 3: Native install (desktop tarball + rhesis-installer.sh)

Installs a launcher, desktop entry, both binaries (client + daemon), a
bundled JRE and LanguageTool to `/usr` (global) or `~/.local` (per-user).
`rhesis-installer.sh` automatically downloads the
matching tarball from the latest release; if you placed a
`rhesis-<version>-x86_64[-local].tar.gz` in the current directory, it is
used instead.

```sh
# download the installer
wget https://github.com/dimkar3000/rhesis/releases/latest/download/rhesis-installer.sh
chmod +x rhesis-installer.sh

# system-wide install (writes to /, requires root)
sudo ./rhesis-installer.sh --install

# or per-user install (no root required)
./rhesis-installer.sh --install local
```

Uninstall with `sudo rhesis --uninstall` for global installations or `rhesis --uninstall`
for a per-user install.

### Option 4: Windows

Download from the [releases page](https://github.com/dimkar3000/rhesis/releases):

- `rhesis-<version>-windows-x86_64-setup.exe`: installer (per user by default,
  no admin rights needed; Start menu entry and uninstaller included).
- `rhesis-<version>-windows-x86_64.zip`: portable. Extract anywhere and run
  `Rhesis\rhesis.exe`.

Both bundle LanguageTool and a trimmed Java runtime, so nothing else is
required. Settings live in `%APPDATA%\rhesis\service.json`. Automatic language
detection uses LanguageTool's built-in detector (the fastText model is not
bundled on Windows).

## Completed/Pending Features
- [x] Basic suggestions features
- [x] Flatpak distribution
- [x] I18N - Localization beyond English
- [x] External LanguageTool server - Support for remote LanguageTool instances
- [x] Better UI/UX - Improved syntax highlighting in the editor
- [x] Alternative distribution methods - Native packages beyond Flatpak
- [x] Ability to run in the background, allowing the server to be used by other apps
- [x] Start at login - creates an XDG autostart entry (Settings toggle; launches hidden to the tray)
- [x] Windows build (Fluent UI, zip + installer)

## Local Development

### Prerequisites

- Rust toolchain, CMake, a C++ compiler
- Qt6 and KDE Frameworks development packages. You can follow the [Kirigami Guide from KDE docs](https://develop.kde.org/docs/getting-started/kirigami/) it has most of what you need.
- Java 17+ JDK
- [fastText](https://github.com/facebookresearch/fastText)

### Windows

Needs Rust (MSVC toolchain) with the Visual Studio C++ Build Tools, Qt 6.8+
(MSVC 2022 64-bit kit) and a JDK 17+. Windows uses its own Kirigami-free QML in
`crates/rhesis-qt-client/src/interop/qml/windows/`, so KDE Frameworks are not needed.

```sh
# QMAKE must point at the Qt kit, and its bin dir must be on PATH to run
export QMAKE='C:\Qt\6.8.3\msvc2022_64\bin\qmake.exe'
export PATH="/c/Qt/6.8.3/msvc2022_64/bin:$PATH"

# Compile the translations once (the .qm files are gitignored)
lrelease translations/*.ts

# Extract LanguageTool-6.6.zip (https://languagetool.org/download/) so that
# build/LanguageTool/languagetool-server.jar exists. Without a Java on PATH or
# a bundled jre\ the daemon cannot start the embedded server.

# Build the client and daemon side by side, then run
cargo build --workspace
cargo run

# Portable zip + installer into build/artifacts/windows (Git Bash; the installer
# step needs Inno Setup 6, pass --no-installer to skip it)
scripts/build-windows.sh
```

### Setup & Run (Linux)

```sh
# Downloads LanguageTool release, builds fastText, downloads lid.176.ftz
scripts/setup.sh

# Builds the client and its daemon side by side, then runs the client;
# it starts the daemon as a sibling process, which boots the LanguageTool
# server automatically
cargo build --workspace
cargo run

# Connection diagnostics on the client (the detached daemon logs nowhere;
# run it yourself with the same variable to see its logs)
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

One workflow, `.github/workflows/pipeline.yml`, drives everything:

- Every push, PR and manual dispatch runs two parallel test jobs —
  `test linux` and `test windows` (the same tier minus the
  Linux-only E2E sandboxes on Windows) — each banking its own platform
  cache.
- Each platform track then proceeds independently: `build linux` → the
  three Linux packaging jobs, `build windows` → the two Windows packaging
  jobs; the tracks meet again only at the GitHub release. The release
  path only runs on version tags (`v*`), or on a manual dispatch that
  asks for it, and no packaging job runs before its own tier is green.
- Version consistency (`Cargo.toml`/`CHANGELOG.md` vs tag) is checked in
  the build jobs for final releases only, via
  `scripts/check-versions.sh`; `v*-*` tags are pre-releases, which skip
  the check.
- All flavors build with the `release` profile. Flatpak, AppImage and
  desktop tarballs (global and `--local`) package in parallel Linux jobs;
  the Windows zip and Inno Setup installer package in parallel Windows
  jobs. Every flavor bundles both binaries (client + daemon).
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
| `rhesis` | Rust | Qt6/KDE desktop UI and client of the local daemon |
| `rhesis-service` | Rust | Headless daemon that owns LanguageTool, settings and the check pipeline |
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
