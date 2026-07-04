# Rhesis

A Qt/KDE desktop writing assistant powered by LanguageTool, written in Rust.

Rhesis runs a local [LanguageTool](https://languagetool.org) HTTP server and communicates with it over loopback to provide grammar and style checking.

## Architecture

A Rust Qt6/KDE desktop client that communicates over HTTP with a local
LanguageTool server (Java, runs on a trimmed JRE inside the flatpak).
LanguageTool uses a bundled fastText binary + model for language
identification.

## Completed/Pending Features
- [x] Basic suggestions features 
- [x] Flatpak distribution
- [ ] I18N - Localization beyond English
- [ ] External LanguageTool server - Support for remote LanguageTool instances
- [ ] Better UI/UX - Improved syntax highlighting in the editor
- [ ] Ability to run in the background as allow for the server to be used by other apps.
- [ ] Alternative distribution methods - Native packages beyond Flatpak
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

The script generates `cargo-sources.json` from `Cargo.lock`, then runs
`flatpak-builder`.  `cargo-sources.json` is not tracked in git — it is
generated on every build.  LanguageTool, fastText, and the lid model
are downloaded from their original sources at build time via the
manifest.

### CI/CD

Two GitHub Actions workflows are provided:

- **`build-flatpak.yml`** — triggered on version tags (`v*`) or manually.
  Builds the flatpak and attaches it to the release.
- **`docker-build.yml`** — triggered on changes to `Dockerfile` or
  `requirements.txt`. Builds and pushes the CI container image to
  `ghcr.io/<owner>/rhesis-ci:kde-6.10`.

The CI uses a custom Docker image (`Dockerfile`) that pre-installs the
flatpak SDK extensions and Python dependencies. To build and push it:

```sh
docker build -t ghcr.io/<owner>/rhesis-ci:kde-6.10 .
docker push ghcr.io/<owner>/rhesis-ci:kde-6.10
```

### Testing

```sh
flatpak run --user --env=RUST_LOG=trace io.github.dimkar3000.rhesis
```

The LanguageTool server output (both stdout and stderr) is forwarded to the
Rust logging system under the `[LanguageTool]:` prefix.

## Scripts

### `scripts/build-flatpak.sh`

Wrapper for building the flatpak. Installs the required flatpak runtimes
and extensions, generates `cargo-sources.json` from `Cargo.lock`, then
runs `flatpak-builder`.

### `scripts/setup.sh`

Downloads and prepares dependencies for local development:

1. Downloads the LanguageTool distribution ZIP and extracts it to `build/LanguageTool/`
2. Clones and builds `fastText` from source
3. Downloads the `lid.176.ftz` language-identification model

Run this from the project root before doing local development.

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

## Building/Updating the container used in CI/CD\

```sh
podman tag -t ghcr.io/dimkar3000/rhesis-ci:kde-6.10 .
podman push ghcr.io/dimkar3000/rhesis-ci:kde-6.10
```
## Licensing

- Rhesis — MIT
- LanguageTool — LGPL 2.1+ 
