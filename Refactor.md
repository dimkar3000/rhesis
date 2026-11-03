# Rhesis refactor — service/client split (living plan & progress)

Source of truth for the backend/client refactor. Checkboxes track progress — tick as items land,
each phase closes only with all gates green (`cargo fmt`, `cargo clippy --all-targets` 0 warnings,
`cargo test` clean, shipped `rhesis` binary links no QuickTest; the service binary links no Qt at
all). Supersedes the session draft plan.

## Goal

Split the single-process app into a headless **backend service** (owns the LanguageTool server
lifecycle and all persisted state) and a **client** (existing Qt/Kirigami UI, mirrors state).
All current features preserved. Wins: UI-free restarts, a real "server outlives UI", small-crate
compile times, and a client binary that ships with **no tokio and no reqwest**.

## Locked decisions (with the why, compressed)

| # | Decision | Why |
|---|---|---|
| 1 | Daemon owns **every** setting; QML `Settings` gone; pages mirror a snapshot; writes are protocol round-trips; one-time INI migration | single source of truth surviving UI restarts; autostart stays a local OS call (it reads OS state), tray stays client-rendered |
| 2 | Auto-shutdown on idle: last client disconnect → **5 s grace** (cancel on reconnect) → clean kill | mirrors quit-on-last-window-closed semantics + survives UI restarts within grace; `background` = "stay alive with no clients" |
| 3 | Exactly **one client session**; second connect refused at handshake | UI singleton guard already short-circuits second launches; single-session routing churn-free in v1 |
| 4 | Workspace of **small crates** | consumer isolation: Qt never links reqwest/tokio; QML iteration never rebuilds the LT tree |
| 5 | Transport = **`interprocess` 2**, direct workspace dep (no wrapper crate) | abstract UDS on Linux (Flatpak↔host) + named pipe on Windows behind one `GenericNamespaced` name; the wrapper audit showed it would be a pass-through for ~20 lines |
| 6 | Wire = **length-prefixed postcard** (`[u32 LE][postcard]`, 16 MiB cap, no COBS, no `MaxSize` derive) | machine↔machine, same-compile ends; non-self-describing, so `Hello` versioning is load-bearing |
| 7 | **JSON on disk** (store) / fixtures / LT HTTP stay JSON; postcard only on the wire | store's consumers are humans + other versions (lenient reads, repairability); fixtures replay LT's real API; LT's HTTP contract is JSON |
| 8 | **No cargo feature flags** introduced; single exception justified: `postcard/use-std` | ledger §8; `cfg(windows)` remains as platform dispatch (not a feature) |
| 9 | Daemon is **threaded-sync** (std threads + crossbeam, the worker's idiom) | one socket, one client: async pays under multiplexed waiting that doesn't exist here; `interprocess/tokio` designed away |
| 10 | Client is **threaded-sync too**: plain `fn main` (drops the vestigial `#[tokio::main]`), one `connection.rs` module boundary | same workload argument + keeps the second concurrency dialect out of the Qt tier; QML → helper → connection is the only path to the wire |

## 1. Workspace layout (final)

```
crates/
  rhesis-protocol      PROTOCOL const, SERVICE_SOCKET_NAME, message enums,
                       shared types (Settings, WorkerStatus, Recommendation),
                       framing + round-trip tests        [serde, postcard/use-std]
  rhesis-settings      versioned JSON store, defaults, atomic writes, INI migration
                       [rhesis-protocol, serde_json, rhesis-platform, log]
  rhesis-languagetool  worker (QString→String), HTTP client, models, provider+Fake,
                       LT dir discovery, java/server.properties  [reqwest, crossbeam,
                       tokio (worker-internal runtime), serde_json, rhesis-protocol,
                       rhesis-platform]
  rhesis-platform      agnostic surface over per-OS internals: single_instance,
                       autostart, process (exe_dir, detached spawn, console suppression,
                       kill-child-on-exit/Job object), paths, translations dir  [windows-sys]
bins/
  rhesis-service       daemon: bind, one session, pump, store wiring, idle shutdown
                       [interprocess, crossbeam, rhesis-{protocol,settings,languagetool,platform}]
  rhesis               Qt app minus worker internals plus connection module
                       [cxx-qt stack, interprocess sync, rhesis-{protocol,platform}]
  rhesis-ui-runner     unchanged QML test harness binary (stays in the rhesis bin crate)
```

Dependency direction: `languagetool → protocol`, `settings → protocol`, bins on top. Worker's
internal `Message`/`LanguageToolWorkerEvent` channels stay unchanged; the daemon maps them to
wire frames. Types moving to `rhesis-protocol` (single definition both ends): `Settings`
(store schema == wire schema), `WorkerStatus` (with `code()`/`reason()`; QML codes 0–5, **6 =
Disconnected is client-local**), `Recommendation` (ex `interop/recommendation.rs`,
`get_suggestions`/`replace_word` consume it rehydrated from the wire).

## 2. Unchanged matching rules

`MAX_START_FAILURES = 3` + budget reset by `Stop`/`SetPort`/`Retry`, healthy-LT adoption on a
busy port, 300 ms debounce, 60 s readiness, fastText/server.properties repair, LT dir
candidates — verbatim in the worker crate. Routing stays last-writer-wins.

## 3. Transport & wire

- Name: `io.github.dimkar3000.rhesis.service` (UI single-instance guard keeps its own names — out
  of scope).
- Framing: `u32 LE + postcard bytes`; garbage mid-stream → session teardown → `Disconnected` →
  reconnect loop (non-self-describing stream: no resync).
- Messages (plain enums; variant discriminant transport, **no** `#[serde(tag)]`):

```rust
enum ClientMsg { Hello { protocol: u32 }, Check { text: String },
                 Start, Stop, SetPort(u16), Retry, SwitchToRemote(String),
                 SetSettings(Settings), Shutdown }
enum DaemonMsg { Hello { protocol: u32, busy: bool }, Settings(Settings),
                 Status(WorkerStatus), Suggestion(Vec<Recommendation>) }
```

- `Hello` first in both directions; version check before any schema-sensitive data. Mismatch
  case (lingering pre-upgrade daemon in idle grace): client sends `Shutdown`, respawns
  same-install daemon, reconnects. **Rule: any type change in rhesis-protocol bumps `PROTOCOL`
  (starts at 1)**; a round-trip test per message variant is the drift tripwire.
- Observability: log *decoded* messages (`Debug` derive) under `trace`, matching the
  LanguageTool log style.
- Fire-and-forget; commands confirm via the next `Status` broadcast (worker's state machine is
  the single source of truth).

## 4. Daemon (rhesis-service)

- Bind-or-fail; accept on a std thread; handshake adopts or answers `Hello { busy: true }` and
  closes.
- **Session pump** = today's dispatcher retargeted: inbound decode → translate to
  `LanguageToolWorkerEvent`s; apply `SetSettings` (persist atomically → mirror → `UpdateColors`);
  worker `Message`s → encode → socket. Idle grace via `crossbeam::select!`/`after(5s)`.
- Worker boots `Stopped`; on handshake the daemon replays stored config as today's QML
  `restart(embedded, port|addr)` equivalent. `background` is a stored flag; the idle timer only
  shuts the daemon down when it is off.
- Shutdown order: `Kill` → existing `Drop` path kills the JVM; Windows Job-object custody via
  `platform::process::kill_child_on_exit` (no-op non-Windows).
- Hermeticity lives here: `RHESIS_FAKE_LT` env-resolved fixture provider — never java, never the
  network, never a cargo feature.

## 5. Client (rhesis, Qt side)

- **First simplification:** `main.rs` drops `#[tokio::main]` → plain `fn main`. Runtime owner is
  gone from the UI binary entirely (the worker never lived here in the new world); the Qt bin's
  dependency set contains no tokio/reqwest/serde_json/rhesis-languagetool.
- **`connection.rs` module boundary** (the only socket-aware module in the client): probe,
  spawn-detached, handshake + retries, decode loop, `CxxQtThread::queue` marshaling, outbound
  channel, reconnect/backoff, `Shutdown` send. Everything else in the Qt tier is wire-blind.
- **Second simplification:** the single-entry rule is the UI contract — QML talks only to
  `AsyncMessagingHelper`, the helper feeds `connection.rs`. No other object touches the socket.
- `AsyncMessagingHelper` keeps its public QML surface (`restart`, `start_server`, `stop_server`,
  `set_server_port`, `retry_server`, `update_colors`, `server_status*` + new setting-mirror
  properties); internals move from in-process channels to the connection module.
- **Startup order:** log init → QApplication → app names + ini format → single-instance
  `acquire()` → probe-or-spawn daemon (detached spawn + handshake retry 50 ms → 2 s cap) → apply
  snapshot (background behavior via `syncBackgroundBehavior`'s successor, language) → icon theme
  → engine load → QML binds mirrors. Daemon unreachable → defaults + system locale; UI still
  boots with `Disconnected` status.
- `platform::single_instance` is Qt-free: the show-runner takes a platform-agnostic callback
  (`impl Fn() + Send`); the Qt app supplies the `queue(... appRaiseMainWindow)` closure.
- Close-handler stranding rule reads the mirror (harmless defaults during the pre-connect
  window; connect precedes engine load).
- Final quit: flush pending `SetSettings`, then `Shutdown` when `background` is off; tray gains
  a dedicated **"Quit service"** item.

## 6. Store & migration (rhesis-settings)

- `~/.config/rhesis/service.json` (XDG_CONFIG_HOME honored) / `%APPDATA%\rhesis\service.json`;
  whole-file atomic writes (tmp + rename) per `SetSettings`.
- Migration: first boot with no store and an existing QSettings INI
  (`$XDG_CONFIG_HOME/rhesis/Rhesis.conf` / `%APPDATA%\rhesis\Rhesis.ini` — exact paths from
  today's `settings_ini_path`) seeds everything, sets `migrated_v1: true`, leaves the INI for
  rollback.
- Store versioned separately (`schema_version`, serde); `PROTOCOL` only rules the wire.

## 7. rhesis-platform surface (agnostic; per-OS modules hidden)

| API | Replaces |
|---|---|
| `single_instance::acquire()` / `dispatch_show_requests(cb)` | `single_instance.rs` + `windows.rs` (verbatim) |
| `autostart::{is_enabled, set_enabled, self_heal}` | `src/autostart.rs` |
| `process::{exe_dir, spawn_detached, suppress_console, kill_child_on_exit}` | exe-dir (language.rs), CREATE_NO_WINDOW + Job object (service.rs) |
| `paths::{qsettings_ini_path, config_dir}` | `settings_ini_path` (language.rs) |
| `resources::translations_dir()` | `find_translations_dir` (language.rs) |

**Not moved:** Qt/GUI concerns (tray, hidden launch, quit-on-last-window-closed, translation
installation) and LT-specific layout logic (`language_tool_dir`, `java_executable` — consume
`exe_dir` but stay in rhesis-languagetool with their `#[cfg(test)]` tests).

## 8. Feature-flag ledger (final)

| Dependency | Features | Why |
|---|---|---|
| `interprocess = "2"` | none | 2.x defaults empty; local sockets unconditional |
| `postcard = { version = "1.1", features = ["use-std"] }` | `use-std` | no_std-first crate; std collections/`to_vec` need serde/std+alloc. Defaults untouched (`heapless` inert); `use-crc` not default and identical `to_vec` bytes; `MaxSize` derive skipped (runtime cap exists) |
| our crates | none | no cargo features anywhere |

Re-open conditions (documented branch points, not defaults): many concurrent clients or
heavy async work in the Qt tier → `interprocess/tokio` with a measured reason; dependency grows
harmful defaults → `default-features = false`, named dep by name.

## 9. Testing

Gates after every change: `cargo fmt`, `cargo clippy --all-targets` (0 warnings), `cargo test`
clean, QuickTest-link check on `rhesis` only. Service binary builds Qt-free.

| Tier | Status |
|---|---|
| languagetool-client/-service/-models | move with crate (QString asserts updated) |
| recommendation | becomes a rhesis-protocol tier test |
| async-messaging-helper | reworked → covered by daemon-session tier |
| single-instance | moves to rhesis-platform tests |
| **protocol framing** (new) | per-variant round-trip, negative decode, version-mismatch handling |
| **settings store** (new) | defaults, atomic writes, INI migration from fixtures |
| **daemon session** (new) | in-process pump + fake provider over real socket names; busy refusal; idle-shutdown timing (test-shortened grace); reattach |
| ui-boot / qml-suites | in-process **fake daemon speaking the real protocol** (tier+pid socket names); QML-Settings-dependent suites reworked onto the mirror object |
| single-instance-e2e | unchanged (client-side) |

Hermeticity preserved: env-var fake provider, per-tier XDG sandboxing (`isolate_environment`),
recorded fixtures. Carried lessons: fail-first `capture` in QML suites, zeroed layer
transitions, reverse-order child pushes, `verify()` aborts. QQmlSettings 500 ms timer lessons
become obsolete (replaced by round-trip acks).

## 10. Packaging / CI / docs

- Every flavor (Flatpak, AppImage, desktop tarball + `runner.sh`, Windows installer/zip) ships
  both binaries; Windows resource/version metadata applies to both exes; JRE + LanguageTool stay
  in the payload the daemon locates (same dir candidates).
- CI Docker image unchanged; workspace builds via existing cargo flow; CMake still orchestrates
  the Qt targets; translations stay client-side.
- One version; CHANGELOG/metainfo flow unchanged; AGENTS.md rewritten for the new layout and
  testing sections.

## 11. Progress

### Phase 1 — Workspace + crate extraction (behavior identical) — DONE
- [x] Workspace members created; bins rebuilt on crates
- [x] rhesis-platform: single-instance (callback-decoupled), autostart, process helpers, paths,
  translations dir; its tests moved (unit tiers redeclared)
- [x] rhesis-languagetool: `src/languagetool/*` + `interop/recommendation.rs` move in;
  `QString→String` inside worker/provider/channels
- [x] Qt bin consumes crates; dead modules removed; gates green

Phase-1 notes: `spawn_detached` (§7) lands with the client rewiring (Phase 4) — no detached
spawn exists yet; `config_dir()` (§6) landed with rhesis-settings (Phase 2). The old
`tests/unit/recommendation.rs` tier folded into `custom_highlighter.rs` in-module tests
(visibility rule: keeps `recommendation_to_variant` crate-private); all LT tiers +
`tests/fixtures/lt/` now live in `crates/rhesis-languagetool/tests/`, the single-instance tier in
`crates/rhesis-platform/tests/unit/`. CI switched to `--workspace` (fmt/test/clippy, build+test
workflows). AGENTS.md paths/commands updated surgically (full rewrite stays in Phase 5).

### Phase 2 — Protocol + settings — DONE
- [x] rhesis-protocol: `PROTOCOL`, socket name, `WorkerStatus`/`Recommendation`/`Settings`,
  `ClientMsg`/`DaemonMsg`, framing + cap; postcard is the crate's only unusual dep
- [x] Round-trip tier: every message variant, negative decode, mismatch unit tests; recommendation
  tests moved here; languagetool imports switch to protocol types
- [x] rhesis-settings: store + defaults + atomic writes + INI migration (fixture-based tier)

Phase-2 notes: `Settings` uses `#[serde(rename_all = "camelCase")]` so the JSON store keeps the
QML-era keys (`externalAddress`, `showTooltips`, ...) while postcard (positional) ignores the
naming; `migrated_v1` lives only in the disk envelope, never in the wire struct. Store semantics:
JSON shadows the INI from the first byte; seeding only when no readable store file; a broken file
is renamed `<path>.json.corrupt` (quarantine, never delete); `schema_version != 1` logs an error
and keeps defaults without touching the file; the INI stays untouched by migration (rollback).
`decode_plain_string`/`@Variant` decoding mirror `qsettings.cpp` exactly (verified against the
source AND a real-QSettings-generated fixture, `tests/fixtures/ini/qml_store.conf`): the writer
emits \x escapes with minimal-length hex (\\x3, not \\x03 — the reader treats `\`+octal as a
greedy octal run, so the writer re-escapes any following hex-digit char; named table is
`\a \b \f \n \r \t \v \" \' \? \\ \0`). Hermeticity: tiers MUST pin the INI via
`with_ini_path(Some(...))` — plain `open()` resolves through the real user machine config, and a
real `Rhesis.conf` exists on dev machines. `config_dir()` landed in rhesis-platform paths
(`config_dir_with` pure seam). LT crate now depends on rhesis-protocol; shims kept:
`recommendation.rs` re-exports `Range`/`Recommendation`, `service.rs` re-exports `WorkerStatus`.
Gates: clippy 0 warnings, 100 tests green across 22 suites, shipped binary QuickTest-clean.

### Phase 3 — Daemon — DONE
- [x] Socket bind via interprocess sync, accept thread, single-session gate
- [x] Session pump: frame ↔ worker-event translation, `SetSettings` persistence + colors,
  handshake config replay
- [x] Idle grace 5 s (`crossbeam::select!`/`after`), `Shutdown` → `Kill` order verified
- [x] Session tier hermetic tests (fake provider, real sockets)

Phase-3 notes: `crates/rhesis-service` (lib + `rhesis-service` bin, socket name
`SERVICE_SOCKET_NAME`). `worker_events(applied, stored)` in `config.rs` is the whole
reconcile: a fresh worker (applied=None) gets the full first-handshake replay —
`RestartLocalServer(port)` (embedded; restart-also-starts, nothing is running yet),
`SwitchToRemoteServer(addr)`, or `Stop` when remote with a cleared address — plus
`UpdateColors` whenever the color table differs; a live worker (applied=Some) only moves
on real drift (changes of port/address/mode), never on an equal reconnect: within the
5 s grace the embedded JVM keeps running. Session = strict-`Hello` handshake (client
speaks first; daemon answers `Hello{protocol,busy}`, then mirrors `Settings`),
worker-message relay thread (`crossbeam::select!` against a stop signal) that feeds
`last_status` and always hands the receiver back before the slot frees. Reads run on a
cloned stream (`try_clone`, `&Stream: Read+Write` — the deprecated 2.4.7 `split()` is
avoided), writes serialize behind one `Arc<Mutex<Stream>>` shared pump↔relay. The
nonblocking listener polls at a 50 ms slice; `background` re-reads on every grace window.
Store: `Store::transient()` added to rhesis-settings for the no-configuration-root case
(defaults-only, never writes anything). Tier lessons: the busy+drop refusal delivers
best-effort — a client may see `ConnectionReset` instead of the busy frame (documented;
both mean "no session"); interprocess accept errors surface as plain `io::Error`
(`WouldBlock`). Gates: clippy 0 warnings, 123 tests green across 26 suites, shipped
binary QuickTest-clean, and the new `rhesis-service` binary links neither QuickTest nor
any Qt library.

### Phase 4 — Client rewiring — DONE
- [x] `connection.rs` (probe, spawn, handshake, pump, reconnect, shutdown send)
- [x] `main.rs`: plain `fn main`; startup re-order (§5); snapshot apply
- [x] Helper over wire; `Disconnected` status + auto-reconnect; setting mirrors
- [x] Root/SettingsPage rework: `Settings{}` removed, async apply/dirty logic
- [x] Tray "Quit service"; shutdown-on-quit per `background`
- [x] Fake in-process daemon for ui-boot + qml-suites (real protocol, tier+pid socket names)
- [x] Qt bin deps cleaned (no tokio/reqwest/serde_json/languagetool); gates green

Phase-4 notes: `Connection::{start, send}` over an unbounded crossbeam channel; the loop
thread (`rhesis-connection`) is the only socket-touching code; `Lost` events flow to the
fan-out (which now only *delivers* — highlighter/setter mirroring — and never stops on a
`Lost`, only on send failure or channel close). Backoff 50 ms→2 s cap; `Spawn` gated by
`RHESIS_SERVICE_SOCKET` presence (never under the test seam); `SetPort`/`SwitchToRemote`
kept as commands for the tray/page (the settings page no longer restarts anything —
`SetSettings` carries the whole snapshot). Handshake: strict `Hello`, `Ok(None)` → reset
(binds to Busy semantics), replay read = `Settings` → `Status` (deterministic order),
status-before-settings ⇒ bail to defaults. Boot probe: `boot_snapshot()` (pub, 2 s
budget; spawns once; Busy→None) BEFORE Qt builds — background behavior + language are
set from the snapshot, never from an INI scan (`persisted_language`, `useIniSettingsFormat`,
`syncBackgroundBehavior` all deleted; language.rs = LanguageManagerRust only). Helper is
the mirror: qproperties `embedded/port/default_port/external_address/background/
show_tooltips/show_debug_tooltips/language/color_settings` + status pair; invokables
apply eagerly and forward `SetSettings` (daemon echo re-applies — no-op). The old
`set_background` qinvokable was REMOVED: the generated `set_background` setter for the
qproperty owns that name; `apply_settings` (bridge.rs) runs the sequential
`self.as_mut().set_*()` chain and re-asserts `setBackgroundBehavior`. `Drop`: sends
`Shutdown` iff `!background && connection.is_some()`. QML: all 10 files rebound to the
mirror (`helper.X`), `appSettings`/`QtCore` gone, port compare via `parseInt`,
Apply = language-first → autostart commit → full snapshot object → eager window-visible
logic → `helper.set_settings(snapshot)` (no `restart`, no `update_colors`, no
`set_background` call). Status 6 (`lt_disconnected`, neutral dot) added to Root/
MainPage/ServerStatusIndicator (+windows variants); tray gains "Quit service" above
"Quit". Fake daemon (`src/fake_daemon.rs`, registered on the lib; Qt-free, QuickTest-free,
never referenced by the bin): pid-suffixed socket via `tier_socket_name`, boots as
already-`Started` (snap-replay = Settings default + Started), fixture text answers the
3 recorded recommendations (2× HE_LIKE "@ [4,8)" + MORFOLOGIK "quickly" @ [38,44),
`#FF0000` — same assertions as the daemon-session tier), `SetSettings` mirrors and
re-emits the derived status; Shutdown ends the session only (the fixture never dies —
suites reuse it; a helper `Drop` during teardown cannot kill a later suite). The runner
(`ui_tests.rs :: run_suites`) and `ui_boot.rs` spawn it FIRST, before any XDG/Qt work;
QML suites: checker keeps harness-hold semantics against the replay (`statusStarted`,
exact strings "is like|likes|quickly", "#ff0000"), settings now seeds/resets through the
mirror (`set_settings` with the page-shaped snapshot; QSettings store deleted from the
suite). Translations: `tray_quit_service` + `lt_disconnected` (+2 contexts) landed in
both .ts (lupdate green, no drops; el_GR translated). Gates: fmt, clippy 0 warnings
workspace-wide, `cargo test --workspace` green, `ldd rhesis | grep -c QuickTest` = 0.
`tests/unit/` (root) deleted with its `[[test]]` entry.

### Phase 5 — Packaging & docs — DONE
- [x] Both binaries in all flavors; runner.sh; Windows resources for both exes
- [x] CI: service-binary no-Qt check; gates unchanged
- [x] AGENTS.md rewrite; CHANGELOG entry

Phase-5 notes: CMake `rhesis_build` = `cargo build -p rhesis --bin rhesis -p rhesis-service
--bin rhesis-service` (scoped flags REQUIRED: bare `--bin` names resolve against the
default-run package and would reject the daemon's bin) + a `RHESIS_SERVICE_BINARY_PATH`
computed like the client's — verified by a real release-fast build (both exes in
`build/cmake-build/release-fast/`, daemon `ldd`-grep 0 for `qt[0-9]|quicktest`). `install_app`
(refactor of build-common.sh) installs bin/rhesis + bin/rhesis-service from the same profile
dir and HARD-FAILS when the daemon is missing (proven negative path); build-desktop
`install_payload`, build-appimage (linuxdeploy `--executable` for the daemon too; the app/
tree copy already carried both), the flatpak manifest build-commands, and build-windows
(`--profile` build + `stage_payload` copies rhesis-service.exe beside rhesis.exe;
windeployqt stays on the client only — the daemon links no Qt) all carry both. All four
 flavors verified by artifact-tree construction with the debug bins; the Inno `.iss`
needed no change (`Source: "{#SourceDir}\*"`). Windows resources for the daemon: new
`crates/rhesis-service/build.rs` (winresource 0.1 build-dep), FileDescription "Rhesis
Service" / OriginalFilename `rhesis-service.exe` / shared product name+publisher+icon
path `../../assets/rhesis.ico` (compile-gated to windows targets, no-op elsewhere — same
shape as the root build.rs, which keeps applying only to its own crate). CI: test.yml
gains the "Daemon binary links no Qt (headless check)" step after fmt (fails when
`build/target/debug/rhesis-service` is not built or greps `qt[0-9]|quicktest`); the
client's QuickTest gate stays a documented local gate (nothing in CI ran it before
either). runner.sh's contract holds unchanged (client finds the sibling via exe_dir; the
daemon inherits runner's PATH prepend through spawn_detached so bundled java/fasttext
resolve) — one comment line added. CHANGELOG: new `## [0.7.0]` section (undated: dates
are pinned at tagging per the release flow; generate-metainfo ran clean with today's
date + tag warning; metainfo NOT committed per AGENTS.md). README: intro/architecture/
local-dev/CI/Option-3/Key-Components updated to the two-binary reality (dev = `cargo
build --workspace` then `cargo run`; client spawns the sibling daemon). AGENTS.md: Build
(dev run + scoped packaging command + daemon log redirection), Testing (rhesis-service
lib tier + daemon-session [[test]] tier in the command list/layout table), gates (both
binaries' linkage: client QuickTest=0 local check, daemon no-Qt enforced in CI), new
"Headless rule" section, build.yml paragraph (Four flavors, both binaries, windows
script). Gates after the packaging change: fmt clean, clippy 0 warnings, `cargo test
--workspace` green (124 passed/0 failed, verified twice incl. one run under concurrent
cargo+make load), `ldd`-gates pass on debug AND release-fast binaries (client
QuickTest-count 0, daemon qt/quicktest-count 0).

## 12. Constants

New: `PROTOCOL = 1` · idle grace 5 s · frame cap 16 MiB · handshake retry 50 ms → 2 s cap.
Unchanged: debounce 300 ms, readiness timeout 60 s, `MAX_START_FAILURES = 3`, default port
2689, fastText layouts, LT dir candidates, store/INI paths (§6).

## 13. Risks & mitigations

- **Async settings pages:** "unsaved changes" diffs become round-trip-sensitive — plan the page
  around an explicit dirty/apply state on the mirror object.
- **Pre-connect window:** stranding rule uses harmless defaults (`background=false`) until the
  snapshot arrives; connect precedes engine load.
- **Upgrade race:** old daemon lingering → `Hello` mismatch → `Shutdown` → respawn → reconnect.
- **Garbage frames:** unrecoverable by design → clean teardown + reconnect; no half-resync.
- **Windows pipe lifecycle** quirks delegated to interprocess (client uses it too).
- **Second-launch UX:** UI singleton fires before the daemon probe; busy-refusal is the backstop.
- **Peer-credential hardening** dropped (session-scoped scope already stronger than the exposed
  loopback port); revisit only for real multi-user sharing.
- **`verify()`/QML suite quirks** carried over; settings suites get the fake daemon first.

## 14. Resolved in review

- Store format stays JSON on disk (wire postcard-only) — confirmed: JSON's consumers are humans
  and cross-version builds; postcard's are same-compile peers.
- No `rhesis-ipc` wrapper crate — `interprocess` is consumed directly by both bins.
- No tokio anywhere in the client binary (plain `fn main` + sync connection module).
- Daemon and client both threaded-sync; single `use-std` feature justified in §8.
- Store filename `service.json` in the config-dir family; tray item wording "Quit service";
  `PROTOCOL` bump enforced by the documented rule + per-variant round-trip tripwire; test socket
  names derive tier+pid via a rhesis-protocol helper.
