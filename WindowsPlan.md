# Windows support plan

Goal: run Rhesis on Windows with a modern (Windows 11 / Fluent) look. Keep Qt/QML.
Linux keeps Kirigami; Windows gets its own QML using Qt Quick Controls `FluentWinUI3` (Qt 6.8+).

Legend: `[x]` done and verified here, `[~]` done but not verifiable here (reason given), `[ ]` todo.

## Decisions
- Phase 3 UI approach: **A**, separate Windows QML in `src/interop/qml/windows/`, Linux QML untouched.
- Build selects one QML set per target OS in `build.rs` (otherwise qmlcachegen fails resolving `org.kde.kirigami` on Windows).
- Windows QML reuses existing `qsTr()` keys only (per-file translation context!), so the `.ts` files need no changes.
- fastText on Windows: **not bundled**. LanguageTool's built-in language detection is used; the app starts the server without `--config` when there is no `server.properties`.
- Installer: **Inno Setup** (per-user by default, optional all-users). Plus a portable zip.
- Qt: 6.8.3 MSVC 2022 64-bit on Windows (CI installs it via `jurplel/install-qt-action`). Linux Qt is untouched.

## Phase 1: Build on Windows
- [x] Build with cargo + MSVC, `QMAKE` -> Qt 6.8.3 (CMake/ECM is Linux-only and not used on Windows)
- [x] Dynamic Qt + `windeployqt` works (`link_qt_object_files` did not get in the way)
- [x] Quick Controls 2 / Svg / Dialogs present in the deployed bundle
- [ ] `.cargo/config.toml` (mold linker, `build/target` per `AGENTS.md`) is gitignored and absent; Windows uses the default `target/`. Nothing to do unless that is wanted.

## Phase 2: Platform-neutral Rust
- [x] `language_tool_dir()` / `find_translations_dir()`: exe-relative Windows candidates
- [x] Persisted language: Windows forces `QSettings` INI format (`useIniSettingsFormat`), file is `%APPDATA%\rhesis\Rhesis.ini` (verified: QML Settings write it there, registry untouched)
- [x] Java launch: bundled `jre\bin\java.exe` (verified from the staged build with no java on PATH), `CREATE_NO_WINDOW`
- [x] Job Object (`KILL_ON_JOB_CLOSE`), `src/languagetool/job_object.rs` (verified: hard-killing `rhesis.exe` also ends java and frees port 2689)
- [x] Server starts without `--config` when `server.properties` is missing (also fixes a Linux failure mode)
- [x] `cargo test`: all existing tests pass on Windows
- Left as is on purpose: `setupIconTheme()` and `set_desktop_file_name` still run on Windows (harmless no-ops).

## Phase 3: Windows UI (approach A)
- [x] `build.rs` OS filter; `main.rs` root QML + style (`FluentWinUI3`) per OS
- [x] `icons/windows/*.svg` + `resources.qrc`
- [x] `windows/Root.qml`, `MainPage.qml`, `SettingsPage.qml`, `components/ServerStatusIndicator.qml`, `components/RuleListItem.qml`
- [x] Fixed after first run: ballooning header (ToolBar needs a real `contentItem`, not an anchored layout; logged geometry: header 960x48), raw `app_title` key (only translated in Root's context), oversized/off-screen window (960x640, centered)
- [x] Fixed after second report: text area content centered vertically (Fluent `TextArea` sets `verticalAlignment: AlignVCenter`; now `AlignTop`). Confirmed by the user.
- [x] Visual check by the user: main page and settings page look right
- [ ] Not yet checked: light vs dark mode, 125%/150% DPI

## Phase 4: Bundle LanguageTool
- [x] `jlink` JRE (same module set as Linux) and trimmed LanguageTool 6.6 in `scripts/build-windows.sh`
- [x] fastText decision (not bundled, see above)

## Phase 5: Packaging and CI
- [x] `scripts/build-windows.sh`: translations, cargo, LanguageTool, JRE, `windeployqt`, `.ico`, staged layout, zip (verified end to end, produced `rhesis-0.4.0-windows-x86_64.zip`, 311 MB; the staged copy runs standalone)
- [~] `io.github.dimkar3000.rhesis.iss` + installer step: compiles (a 278 MB setup.exe was produced on the user's machine), but the installer has **not been installed or run** yet
- [~] `build-windows` job in `.github/workflows/build.yml` (windows-latest, Qt via install-qt-action, temurin 17, `choco install innosetup`, uploads `windows` artifact; release job now needs it and attaches the zip and setup exe). YAML parses; **never run on GitHub**.
- [x] `.gitattributes` forces LF for `*.sh` (bash cannot source CRLF scripts that `core.autocrlf` produces on Windows)
- [x] README: Windows install + development sections
- [x] Code signing: decided not to sign. The installer and exe stay unsigned, so SmartScreen will warn on first run.

## Phase 6: Verification
- [x] `cargo test` on Windows
- [x] Standalone staged run: translations, bundled JRE, LanguageTool reaches `Started`, `/v2/check` with `language=auto` answers
- [ ] First CI run on GitHub (tag or manual dispatch), then install the produced setup.exe
- [ ] Manual checklist: port in use, external server, language switch, non-ASCII install path, uninstall

## develop features (v0.5.0 background/tray, v0.5.1 autostart): ported to Windows
This work was started on `main` (v0.4.0 era) and moved onto `develop`, which was ahead.
- [x] Branch: stashed from `main`, applied on `develop`; 6 conflicts resolved (Cargo.toml, build.rs, main.rs, helper.h, README, build.yml)
- [x] Windows single-instance guard (`src/single_instance/windows.rs`): session-local named mutex + named event; same interface as the Linux abstract-socket guard. Verified with the real app: second launch exits in ~300 ms and the first one shows its window.
- [x] Windows autostart (`src/autostart.rs`): `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` value `Rhesis` = quoted exe + `--hidden`; `self_heal` repoints a value whose exe no longer exists. Registry round-trip is unit-tested on a scratch key (never the real Run key).
- [x] Windows QML: tray icon + menu, hide-on-close, `--hidden` launch, background and autostart switches (same draft/Apply contract as the Linux page). Verified: `--hidden` runs with no window, a second launch shows it; the settings page loads without QML errors.
- [x] `develop`'s Linux-only test suites (`ui_boot`, `qml_suites`, `single_instance_e2e`) are gated with `#![cfg(not(windows))]`: they target the Kirigami UI / Linux socket, and `tests/e2e/common` is a symlink Windows checkouts do not resolve.
- [x] `cargo fmt --check` clean, `cargo clippy --all-targets -- -D warnings` clean (Windows), `cargo test` all green (Windows). Linux lint could not be run here; Linux code paths were kept textually identical to upstream where possible.
- [ ] Not exercised: clicking the autostart switch in the UI and logging out/in (the pieces are verified separately); the tray icon itself being visible in the notification area.
- [ ] Windows may show the entry as disabled if the user turns it off in Task Manager > Startup; that state is not read back.

## Progress log
- Plan written; Phase 3 QML, build.rs/main.rs wiring written.
- First `cargo build` on Windows succeeded with Qt 6.8.3; Phase 2 Rust changes done and verified.
- Header/title/window-size/text-alignment UI bugs found by running the app and fixed.
- Packaging script, Inno Setup script, CI job and docs added.
- Release builds on Windows now use the GUI subsystem (no console window); debug builds keep the console. Verified in the PE header.
- Moved onto `develop`; single-instance, autostart, tray/background and settings switches ported to Windows and verified against the real app.
