# Testing Refactor — Rhesis

> **Historical record.** This was the working plan for building the test stack
> and is kept for the reasoning behind the seams. The layout has since moved on
> (tests under `tests/unit/` + `tests/e2e/`, no `ui-harness` feature, no
> real-jar tier) — `AGENTS.md` → *Testing* is the current source of truth.

Working plan for adding layered testing (Rust unit → Qt Quick Test page-level → full-boot E2E).
This file is the live progress tracker: statuses below are updated as work lands.

**Locked decisions**
- Provider seam refactor (not TCP stubs): the completion-provider boundary is abstracted so UI
  tests run hermetically and the Rust plumbing (debounce/teardown/lifetime) is unit-testable.
- Fake provider injectable via env (`RHESIS_FAKE_LT`); real provider stays the hard default.
- Qt Quick Test (`TestCase`/`SignalSpy`/`mouseClick`/`grabImage`) is the page-level framework;
  custom runner binary `rhesis-ui-runner` links the lib (cxx-qt static registrations).
- LanguageTool jar/spawn abstracted away from fast paths; real-jar suite is opt-in `#[ignore]`.
- QML pages and bridge signatures stay untouched (only minimal `objectName` additions).
- Existing 17 unit tests must stay green after every step.

**Status legend**: ⬜ pending · 🔄 in progress · ✅ done

## Phase 1 — Provider + sink seams (production code) ✅
- ✅ `src/languagetool/provider.rs` — `enum CompletionProvider { Live(LanguageToolClient), Fake(FakeProvider) }`
  chosen in `run_loop`; `RHESIS_FAKE_LT=<id>` env decides (falls back to Live with a loud log).
  Fake = `tests/fixtures/lt/<id>.json` (array of `{text, response|error}` envelopes), mapped through
  the **real** client conversion (`map_dto`), so fixtures carry live DTO shapes. Builder API
  (`respond`/`respond_dto`/`fail`) for lib tests; `request_log()` probe counts checks
  (debounce assertions). `LanguageToolWorker::with_fake_provider(..)` overrides env for tests.
- ✅ Fake lifecycle: `verify_remote`/`start_local_server` short-circuit to `record_success()` in fake
  mode → `Stopped -> Started` instantly, no java, no HTTP; all else (Stop/Kill/SetPort/status
  emission, debounce, failure bookkeeping) unchanged. `cargo build` clean, zero warnings.
- ✅ Message-sink abstraction: `MessageSink` (`send_suggestions`/`send_status`) +
  `QtMessageSink` production impl + generic `fan_out_messages(receiver, sink) -> sink` in
  `custom_highlighter.rs`; `start_message_thread` signature unchanged (bridge/QML zero diff).
- ✅ `client.rs`: DTO→`Recommendation` conversion extracted (`map_dto` + free `select_color`),
  shared by Live HTTP arm and fake fixture loader. `serde_json` moved to `[dependencies]`
  (fixture parsing is runtime-facing, kept in release by design).
- ✅ Gate: `cargo test --lib` green — **32 tests** (17 pre-existing + 15 new), app builds clean.

## Phase 2 — Rust tests enabled by the seams ✅
- ✅ Worker event loop with `FakeProvider`: `Start` → `[Stopped, Started]` without a server spawn;
  N keystrokes inside the 300ms debounce window collapse into **one** check of the last text
  (asserted via `request_log`); empty text dropped before the provider; outages flip
  `FailedToRespond`, the next text recovers to `Started`; `Kill` → thread join + `running=false`.
- ✅ `client.rs`/`models.rs` vs fixtures: real server responses recorded from the actual
  LanguageTool jar (`tests/fixtures/lt/checker.json`: `HE_LIKE`×2 replacements + `MORFOLOGIK`
  + a clean sentence) — parsed with `LanguageToolDto` serde, mapped to `Recommendation`s,
  fields asserted. Address-normalization edge cases asserted.
- ✅ `status_to_qml` mapping table (0..5 codes + reasons).

## Phase 3 — Qt Quick Test harness (page-level) ✅
- ✅ Spike `tests/ui/tst_spike.qml` — all three cornerstones pass under
  `QT_QPA_PLATFORM=offscreen` (**5/5 PASS**): QuickTest runs suites from the
  directory, the statically registered `io.github.dimkar3000.rhesis` module
  resolves at compile time **and** instantiates in the test engine, and
  `grabImage` renders offscreen. Host Qt 6.12 confirmed; pinned SDK 6.11
  still to verify (CI-only concern). **Retired**: the file was deleted once
  the real suites below superseded it.
- ✅ Runner: the entry point lives IN the lib — `src/ui_tests.rs`
  (`#[cfg(feature = "ui-harness")] pub mod ui_tests`) exposes `run_suites()`,
  and `src/bin/rhesis-ui-runner.rs` is a one-line shim calling it. That real
  lib use is exactly what delivers the crate's recorded
  `-lrhesis-cxxqt-generated` + Qt module directives and the static QML module
  registration into the runner's link — no dummy symbol anchors (an earlier
  `#[used] static` anchor hack worked via the same rlib-metadata mechanism
  and was removed the same day). `Cargo.toml`: explicit `[[bin]]` blocks with
  `required-features = ["ui-harness"]` — the runner binary is simply not
  built for default/release profiles. `build.rs` under the feature adds
  `qt_module("QuickTest")` (cxx-qt-build resolves and links it — no manual
  `rustc-link-lib` override) and compiles `src/ui_tests/wrapper.cpp`, a
  6-line `extern "C"` wrapper for the C++-mangled `quick_test_main(int,
  char**, name, sourceDir)`; `sourceDir` is passed explicitly (dodging
  `QTEST_SET_MAIN_SOURCE_PATH`). `RHESIS_FAKE_LT` is seeded inside
  `run_suites()` before anything can spawn a worker or touch the network.
  Qt Quick Test **requires the `tst_` prefix** for suite directory discovery.
- ✅ Suites `tests/ui/*.qml` — **15/15 PASS** offscreen (3× stable reruns,
  ~0.7s) with `QTEST_...`-style semantics verified on host Qt 6.12:
  - `tst_checker.qml`: mirrors Root's object graph (highlighter wired before
    helper initializes the fake worker); asserts worker → `Started` via the
    fake provider, `findRecommendation` bounds ([4,8)/[38,44), exclusive end,
    gaps empty), `getSuggestions(start, end)` menu contents — both fixture
    groups pass the length-based filter for the first word, so recorded
    values come back in fixture order — clean text clearing all matches,
    and `grabImage` finding the pipeline-default `#ff0000` underlined text
    (fake recommendations are pre-colored with an empty rule table at
    construction; `update_colors` only affects future mappings).
  - `tst_settings.qml`: fresh-page Apply disabled; tooltip switch joins the
    draft until Apply (and debug tooltips pair with it on commit);
    autostart⇄background pairing forces+locks the switch and Apply writes/
    removes the desktop entry that `is_autostart` reads back; the appended
    rule row blocks Apply while empty and commits into `colorSettings`
    (`CATEGORY:RULETEST`); removing the row cancels the addition.
  - Minimally-named QML hooks (`checkerTextArea`, switches/buttons,
    `ruleTextField`/`removeRuleButton`) — pages and bridge API untouched.
  - QuickTest lessons baked into the suites: the tst root must be a plain
    Item (QuickTest loads files in a QQuickView — app shell = nested window
    below the TestCase); test functions run **alphabetically** (an `init()`
    resets store/entry state so functions are order-independent); direct
    `checked` writes and `toggle()` do not emit `toggled` on bound switches
    — `click()` is the interaction path (also for buttons); `Pages` created
    from a TestCase-rooted file resolve `applicationWindow()` through the
    file root, so the test root re-exposes it; host Qt 6.12 `tryVerify`/
    `tryCompare` take no trailing status — `harness.js :: hold()` wraps the
    polling to keep human-readable failure reasons; `grabImage().pixel()`
    returns QColor objects (coerced via `toString()`); QtCore `Settings`
    has no identity in a bare quick_test_main process — the store is pinned
    via the documented `location` property into the runner's sandbox.

## Phase 4 — Full-boot E2E (Rust-driven) ✅
- ✅ `tests/ui_boot.rs`: fake XDG config/runtime dirs, serialized; boot real `Root.qml`;
  autostart lifecycle; persistence roundtrip; screenshots → `build/ui-tests/`.
  **3/3 stable runs** (~2s each).
- ✅ `tests/single_instance_e2e.rs`: process-level single-instance guard test — two real
  launches, primary stays alive, secondary nudges and exits cleanly. **3/3 stable** (~0.3s).
- ✅ `tests/real_jar.rs`: opt-in `#[ignore]` suite (requires LanguageTool installation).
- Lessons: inverted settle contract (fixed via `Ok(None)`/`Ok(Some)` convention),
  QQmlSettings 500ms write timer (poll-until-file-contains pattern instead of blind pump),
  phantom generic turbofish `::<()>` for `process_all_events_until`, rerun-if-changed gap
  in build.rs for harness.cpp, auto-discovery rule (declared `[[test]]` paths claim their
  file but don't disable auto-discovery for other `tests/*.rs`), socket preconditions for
  single-instance tier, secondary-launch E2E log polling (child exit ≠ log drain complete).

## Phase 5 — Docs/CI ⬜
- ✅ `AGENTS.md` test section rewrite (commands per layer, `--test-threads=1` for boot tier).
- ✅ CI step added in `.github/workflows/build.yml` (env `QT_QPA_PLATFORM=offscreen`;
  `continue-on-error: true` until kde-6.11 QuickTest availability proven).
- ✅ Real-jar suite `#[ignore]` (opt-in, no lint suppressions).

**Seam map (implemented 2026-10-28)**
```
QML text_area_changed → AsyncMessagingHelperRust
  → event_sender.send(WorkerEvent::UpdateSuggestions(text))    [crossbeam]
  → LanguageToolWorker event loop (std thread)
      → CompletionProvider::chosen / with_fake_provider        ← SEAM 1 (landed)
          Live(LanguageToolClient) : Fake(FakeProvider from tests/fixtures/lt/<id>.json)
      (fake mode: verify_remote/start_local_server short-circuit → instant Started, no jar)
  → Message::Suggestion / StatusChanged → message_receiver      [crossbeam]
  → fan_out_messages(receiver, sink)                            ← SEAM 2 (landed)
      QtMessageSink (CxxQtThread::queue → highlighter/helper)   : any test MessageSink
```

**Session log**
- 2026-10-28: Phases 1+2 landed (both seams + 15 new Rust tests; 32 total green, no warnings,
  app builds). Fixtures recorded live from the real LanguageTool jar. Bridge/QML/artifact
  scripts untouched. Uncommitted set from this session: `Cargo.toml`, `src/languagetool/*`
  (client/mod/models/service/provider), `src/interop/custom_highlighter.rs`,
  `tests/fixtures/lt/checker.json`, this file. The `src/autostart.rs` + `src/main.rs`
  dead-AppImage heal arm mentioned as separate work has since been **reverted** on request
  (`git checkout`); the earlier bullet about it is stale.
- 2026-10-28 (later): Phase 3 spike landed. Runner link root cause found: a bin that
  references no item of the package lib gets none of the build script's `-l` directives
  (only `-L` paths reach it) — the app bin worked purely because it uses the lib and
  inherits the recorded native libs via rlib metadata. Anchored one lib fn pointer in the
  runner (`#[used] static`), link went green in 5.7s. Spike suite `tst_spike.qml` renamed
  to the `tst_` prefix QuickTest requires, rewritten to instantiate a module type directly
  (createQmlObject is unavailable inside TestCase functions) — **5/5 PASS offscreen**.
  Default build + `cargo test --lib` still clean (32/32, zero warnings).
  Uncommitted set now additionally: `build.rs` (conditional wrapper compile +
  `dylib=Qt6QuickTest`), `src/bin/rhesis-ui-runner.rs`, `src/ui_tests/wrapper.cpp`,
  `tests/ui/tst_spike.qml`, this file. Closed build.rs leftover: unused `builder` binding.
- 2026-10-28 (evening): removed the `#[used]` link-anchor hack properly: the
  QuickTest entry point moved into the lib (`src/ui_tests.rs :: run_suites()`,
  behind `ui-harness`) and `src/bin/rhesis-ui-runner.rs` became a one-line shim
  calling it — a genuine lib reference, i.e. the same rustc rlib-metadata
  channel the dummy anchor exploited, expressed honestly. `Cargo.toml` gained
  explicit `[[bin]]` sections (runner now `required-features =
  ["ui-harness"]`, truly absent from default and release builds);
  `build.rs` gained `qt_module("QuickTest")` under the feature, replacing the
  manual `rustc-link-lib=dylib=Qt6QuickTest` line (cxx-qt-build resolves the
  module fine). Spike still **5/5 PASS** offscreen; app binary verified
  QuickTest-free (`ldd` grep count 0). Gates: 32/32 lib tests; zero warnings
  across `cargo build` (default), `cargo build --features ui-harness`,
  `cargo test --lib`.
- 2026-10-28 (night): Phase 3 suites landed — `tests/ui/harness.js` + the
  two real suites replacing the spike; **15/15 PASS** offscreen ×3 stable
  runs (~0.7s). `src/ui_tests.rs` rewritten for hermeticity/readability:
  XDG sandbox (`build/ui-tests/harness-env/{config,data,cache,runtime,home}`,
  truncated per run, 0o700), `RHESIS_FAKE_LT` seeded before Qt/worker boot,
  small named functions (`suite_root`, `collect_runner_arguments`,
  `isolate_environment`, `seed_fake_provider`) with documented failures.
  QML changes: only `objectName` hooks (checkerTextArea, switches/buttons,
  ruleTextField/removeRuleButton). Hard findings baked into the suites
  (see Phase 3 bullet): tst root must stay a plain Item + nested app-shell
  window; alphabetical test function order → per-function `init()` reset;
  `click()` over `toggle()`/`checked=` for bound switches; TestCase-root
  providing `applicationWindow()`; `harness.js::hold()` for message-carrying
  polling; QSettings via explicit `location` (bare quick_test_main has no
  app identity). Debug-instrumentation removed; suites fail with reasons.
  Gates: `cargo fmt`; clippy **0 warnings/errors** both profiles;
  `cargo test --lib` **32/32**; app build QuickTest-free (`ldd` grep 0);
  ui-harness build + 3× suite rerun stable. Uncommitted additions:
  `tests/ui/harness.js`, `tests/ui/tst_checker.qml`, `tests/ui/tst_settings.qml`.
- 2026-10-28 (Phase 4): Full-boot E2E passes 3/3 stable (~2s); single-instance
  E2E passes 3/3 (~0.3s); real-jar correctly opt-in via `#[ignore]`.
  Key fixes: QQmlSettings 500ms timer → poll-until-file-contains pattern in
  `hold()` predicate; single-instance log race → `wait_for_log` + `join_drainer`;
  `hold()` restructured to avoid `last_state` carry (eliminates `unused_assignments`).
  Phase 5: `AGENTS.md` rewritten with per-tier Testing section; CI `build.yml`
  gains "Run tests" step (continue-on-error).
