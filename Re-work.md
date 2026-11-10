# Rhesis launch / autostart re-work — implementation plan

Status: agreed design, implementation pending. Reference while working;
tick items off as they land.

## Goal

The app makes no flavor decisions at runtime, probes no files, and needs no
wrapper process on any install flavor. One user-writable configuration file
(`install.conf`) carries the install identity; both the client and the daemon
read it at startup. Movable installs (AppImage, Windows zip) write back to it
when they move. Autostart entries "just start the application"; the app reads
the configuration to know what to do.

No env-var contract: the `RHESIS_*` override layer is **dropped entirely**
(user decision). The client keeps hand-rolled `--hidden` parsing (no clap
adoption).

## The configuration file

- Path: `config_dir()/install.conf` — the platform crates' existing
  `config_dir()` (Linux `~/.config/rhesis/`, grown with the app dir as today;
  Windows `%APPDATA%`-based equivalent). Flatpak resolves inside the sandbox
  app home (`~/.var/app/io.github.dimkar3000.rhesis/…`), so its conf is
  app-local and never shared with native installs.
- Format: tiny INI (`key = value`, `;`/`#` comments) — std-only parser living
  in a new OS-generic module `rhesis-platform::install` (crate root, not the
  OS folders — the logic is identical on both OSes; constants stay per-OS).
- Keys:

| Key | Meaning |
|-----|---------|
| `flavor` | `linux-local` / `linux-flatpak` / `linux-appimage` / `windows` |
| `translations` | absolute dir of the compiled `.qm` catalogs |
| `languagetool` | absolute LanguageTool dir (`languagetool-server.jar` lives here) |
| `java` | absolute java executable |
| `autostart-exec` | the exact Exec line (incl. ` --hidden`) this flavor last wrote to the OS — the heal record |

- Lifecycle / ownership:
  - **Client** ensures it at boot: read it; if absent — or if present but the
    *derived* install truth differs while `flavor` matches — derive and write
    it back, all **before** spawning the daemon (so the daemon's first read
    sees it). One writer: the client. Nothing else writes.
  - **Daemon**: read-only consumer (LT dir, java, flavor for the boot log).
    Falls back to deriving in memory when the file is absent (standalone/dev
    runs) but never writes.
  - A foreign-`flavor` instance reads its own derived paths and never rewrites
    a record describing another flavor's install.

## Derivation (the seed/write-back formula)

A pure function of `exe_dir` (+ `$APPIMAGE` where the AppImage runtime
exports it — identity, not configuration). No filesystem stats, no env
branching beyond the AppImage identity. Per flavor:

| Flavor | exe_dir | root | translations | languagetool | java |
|--------|---------|------|--------------|--------------|------|
| `linux-local` | `$PREFIX/bin` | `exe_dir/..` | `root/share/rhesis/translations` *(verify against build-common's `install_payload` at edit time)* | `root/share/rhesis/LanguageTool` | `root/lib/jre/bin/java` |
| `linux-flatpak` | `/app/bin` | `/app` | `root/share/…` | `root/share/…` | `root/lib/jre/bin/java` |
| `linux-appimage` | `$APPDIR/app/bin` | `exe_dir/..` | same shape as flatpak (*verify payload path in build-appimage*) | | |
| `windows` | install root itself | `exe_dir` | `root/translations` | `root/LanguageTool` | `root/jre/bin/java.exe` |

Dev residuals (`target/debug` exe, when no conf is seeded): current CWD
`./translations`, the existing dev LT paths, bare `java` from PATH — so
`cargo run` keeps working off a setup.sh install. Dev runs **derive but never
write** the conf (no pollution of a real install's file).

## Autostart

- Exec composition (pure, at toggle time and heal time): flatpak →
  `flatpak run <app id> --hidden` (app id from flavor+injection, not
  `$FLATPAK_ID`); appimage → quoted `$APPIMAGE --hidden`; everything else →
  quoted `current_exe() --hidden`. Same `exec_escape` quote-if-space rule.
- Toggle on (`set_enabled(true)`): write the entry / Run value as today, then
  append/update `autostart-exec` in the conf (best effort; failure logs).
- Toggle off: unchanged (delete what's there).
- `is_enabled()`: unchanged semantics (entry Exec == currently composed, the
  cross-flavor honesty guard).
- Heal v2, every client launch, replaces `is_appimage_exec` shape matching:
  1. derive current truth; compare against conf (`flavor` equal, paths
     differ) → the install moved → write conf back;
  2. if the OS entry's Exec (Linux) / Run value (Windows) equals the conf's
     recorded `autostart-exec` and that line differs from the newly composed
     one → rewrite the OS entry and update the record.
  Windows keeps the small `run_target` exists-check as a final guard.
  Deterministic record comparison; the "arbitrarily renamed AppImage cannot
  be recognized" limitation disappears (we compare against our own record).
- Migration note: entries enabled under the old code have no record; they
  degrade to the honest-toggle path (toggle reads off; re-enabling rewrites).
  No behavioral regression.
- The entry still carries ` --hidden` (unchanged UX, Root.qml gating).

## Uninstall

- runner.sh keeps only **download + install** (`--install [global|local]` and
  `--help`); its `--uninstall` and `launch()`/PATH/RHESIS_LAUNCHER exports are
  deleted (user decision).
- Uninstall moves into the binaries: `rhesis --uninstall` (client arg,
  hand-rolled parsing) — "only available where it already is": guarded by
  `flavor` and by the conf existing.
  - `linux-local`: remove owned autostart entry, the packaged menu desktop
    file (app id), then the payload root and `config_dir()`; global installs
    need privileges → permission failure logs a clear error, exit non-zero.
  - `windows` portable: same set, plus the running-exe deletion problem
    (exe holds a lock): schedule the payload deletion (detached helper/
    `MoveFileEx` semantics) — implementation detail during work.
  - `windows` installed: covered by Inno's own uninstaller; `--uninstall`
    reports that and exits.
  - `linux-flatpak`: OS-managed (`flatpak uninstall`); report the command,
    do nothing.
  - `linux-appimage`: delete owned entry + records; image removal is the
    user's (or Gear Lever's) act.

## Packaging / flavor changes

| Flavor | Change |
|--------|--------|
| Tarball (build-common/build-desktop) | desktop file's `Exec` points straight at the payload (baked abs path); no wrapper in the launch chain; runner.sh kept as download+install CLI |
| Flatpak (io.github.dimkar3000.rhesis.json) | no new wrapper; `command` stays `rhesis`; payload paths untouched (sibling-daemon spawn keeps working) |
| AppImage (build-appimage.sh :: write_apprun) | AppRun keeps only loader duties (LD_LIBRARY_PATH/Qt); **all RHESIS_* exports removed** (incl. the dangling `RHESIS_LANGUAGETOOL_DIR`) |
| Windows (build-windows.sh, .iss) | **no launcher binary**; `stage_payload` unchanged in shape; loading of conf derivation covers zip and installed alike |

`server.properties` stays packaging-baked (LT-directory-relative), no
fasttext/model key needed. `RHESIS_SERVICE_SOCKET`, `RHESIS_ARCHIVE_SRC`,
`RHESIS_REPO`, `RHESIS_LT_DIR` (dev script) keep their existing roles.

## App-side deletions (why this is a net subtraction)

- `linux/resources.rs` + `windows/resources.rs`: whole probe chains → conf
  read (+ dev residual in the derive fn). The `translations_dir()` entry
  point stays (thin forward to the install module).
- `worker.rs`: `language_tool_dir` candidate list and `java_executable`'s
  Windows-tree/PATH decision → params injected at worker construction from
  the conf-derived values.
- `linux/autostart.rs`: `is_appimage_exec` + its tests, the heal shape
  machinery; `exec_for` loses the `$FLATPAK_ID` and `RHESIS_LAUNCHER`
  branches (3 pure inputs remain).
- `windows/autostart.rs`: registry layer untouched; heal becomes record
  compare + the existing exists-check.
- AppRun/runner/wrapppers as above; no third Windows binary.

## Stays as-is

- `paths.rs` (both OSes): `config_dir()` per-user state, flavor-free —
  unchanged. The heal record lives in `install.conf`, not a separate file.
- Run key / desktop file writers, QSettings migration, `qsettings_ini_path`.
- Single-instance swap (separate work item, unchanged): adopt
  `app-single-instance` over the literal `single-instance` crate (that one is
  detection-only and loses the show-window semantics); interface stands in
  for `acquire()` / `spawn_runner(show)`; `job_object` custody stays; flatpak
  abstract-socket + session-local Windows naming constraints preserved.

## Boot sequence deltas

1. `single_instance::acquire()` (unchanged).
2. NEW: ensure `install.conf` (read / derive-with-writeback) — client-side.
3. `connection::boot_snapshot()` spawns the daemon, which now reads the conf
   read-only; conf-derived LT/java params injected into the worker.
4. `autostart::self_heal()` → heal v2 (conf-record based).
5. `translations_dir()` from conf/derivation → `applyLanguage` (unchanged).

No protocol changes; `daemon-session` tier untouched.

## Tests

- `rhesis-platform`: derive-formula tests (per flavor, injected exe paths /
  `$APPIMAGE` — pure, no FS), conf parse+round-trip tests, heal-record tests
  for both OSes (replacing the AppImage shape tests), uninstall-guard tests.
- `rhesis-languagetool`: worker tests take injected params (probing tests
  deleted).
- Qt/`src` tiers: unchanged signatures; `self_heal` call site unchanged.
- E2E: `single_instance_e2e` / `ui_boot` sandbox via `RHESIS_SERVICE_SOCKET`
  + isolated XDG homes — unaffected; dev-no-write guard keeps them inert.

## Gates (per AGENTS.md)

`cargo fmt`; `cargo clippy --workspace --all-targets` 0 warnings;
`cargo test --workspace` clean; `ldd build/target/debug/rhesis | grep -c
QuickTest` = 0; `ldd rhesis-service` shows no Qt/QuickTest. The conf parser
is std-only — no linkage impact.

## Suggested order

1. `rhesis-platform::install` (parser + derive + conf read/write) + tests.
2. Wire client boot (ensure-conf, translations) and daemon read-only
   consumer + worker param injection.
3. Autostart v2 both OSes (composition + record + heal) + test rewrites.
4. Delete resources/worker probing, AppRun/runner slimming, packaging script
   updates (desktop Exec, manifest, stage_payload), uninstall-in-binary.
5. Full gate pass + flavor smoke matrix.
