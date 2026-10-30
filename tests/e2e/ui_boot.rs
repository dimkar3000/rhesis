//! Full-boot E2E — the Rust-driven tier of the UI test stack.
//!
//! Boots the real `Root.qml` inside a running QApplication by mirroring the
//! production boot sequence (`src/main.rs :: run_ui`) step by step: identity,
//! background-behavior sync, the single-instance guard, icon theme,
//! translations, desktop file name, KDE style, engine load.
//!
//! Every observed state is fetched back through the C++ probes
//! (`src/ui_tests/harness.cpp`): expressions are evaluated in Root.qml's own
//! scope, so `messagingHelper` / `highlighter` / `appSettings` — the ids the
//! rest of the file wires together — are directly reachable without any QML
//! or bridge changes.
//!
//! Hermeticity matches the QuickTest runner (`src/ui_tests.rs`): XDG homes
//! are redirected into `build/ui-tests/boot-env` and `RHESIS_FAKE_LT=checker`
//! is seeded, so the LanguageTool worker answers from the recorded fixture
//! and never spawns java or touches the network.
//!
//! Serialization: Qt allows one QApplication per process, and the test needs
//! the event loop, so this target carries exactly one test. Run it with:
//!
//! ```sh
//! cargo test --test ui-boot -- --test-threads=1
//! ```
//!
//! `QT_QPA_PLATFORM=offscreen` is forced unless the caller picked a platform
//! (a watched local run may keep a real window). A local run saves one
//! screenshot to `build/ui-tests/boot-env/boot-window.png` for manual
//! inspection; CI sets `CI` and saves nothing.

mod common;

use std::env;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use cxx::UniquePtr;
use cxx_qt_lib::{QGuiApplication, QQuickStyle, QString};
use cxx_qt_lib_extras::{QApplication, QEventLoop};
use rhesis::interop::bridge;
use rhesis::interop::language::{find_translations_dir, persisted_language};

/// App id, doubling as the desktop file name (see main.rs).
const NAMESPACE: &str = "io.github.dimkar3000.rhesis";

/// Worker status codes (see `WorkerStatus::code` in languagetool/service.rs);
/// the fake provider settles on Started without any server.
const STATUS_STARTED: i32 = 2;

/// The recorded field text ("checker" fixture) and its clean counterpart.
const FIXTURE_TEXT: &str = "She like football and play the guitar quicly.";
const CLEAN_TEXT: &str = "This sentence has no problems.";

/// How long a condition may take (worker boot, pipeline, secondary launch).
const WORKER_BOOT_TIMEOUT: Duration = Duration::from_secs(5);
const PIPELINE_TIMEOUT: Duration = Duration::from_secs(5);
const STAND_DOWN_TIMEOUT: Duration = Duration::from_secs(15);
/// QQmlSettings stores a property change from a 500ms timer
/// (`settingsWriteDelay` in Qt sources); polling the file keeps the wait
/// adaptive instead of assuming the delay verbatim.
const SETTINGS_STORE_TIMEOUT: Duration = Duration::from_secs(5);

/// Event-loop pump pacing: enough per pump to hand the worker threads wall
/// time, small enough to keep assertions snappy.
const PUMP_TICK: Duration = Duration::from_millis(20);
const PUMP_PAUSE: Duration = Duration::from_millis(5);
const STAND_DOWN_POLL: Duration = Duration::from_millis(50);

/// Caller buffer for probe results (joined suggestion values stay far below).
const PROBE_BUFFER_SIZE: usize = 4096;

/// Where the boot tier's evidence lands (beside the QuickTest harness's
/// own `harness-env` sandbox, so `cargo clean`-adjacent churn never
/// pollutes a real user configuration and failing runs leave state to
/// inspect — it is truncated at every launch).
fn sandbox_root() -> PathBuf {
    common::sandbox_root("boot-env")
}

/// Wraps a `&str` into a `CString`; the harness strings never carry interior
/// NULs, so the conversion cannot fail silently.
fn cstring(value: &str) -> CString {
    CString::new(value).expect("harness strings contain no interior NUL")
}

/// Forces offscreen rendering for deterministic runs unless the caller
/// explicitly picked a platform (a watched local run may want a real
/// window: `QT_QPA_PLATFORM=xcb cargo test ...`). Boot-only helper: the
/// process tiers inherit the environment through their spawned children.
fn force_offscreen_when_unset() {
    if env::var_os("QT_QPA_PLATFORM").is_none() {
        env::set_var("QT_QPA_PLATFORM", "offscreen");
    }
}

// C++ probes from src/ui_tests/harness.cpp; every call below this block is
// wrapped so the unsafe surface stays contained.
extern "C" {
    fn rhesis_boot_engine_new() -> *mut c_void;
    fn rhesis_boot_engine_load(engine: *mut c_void, url: *const c_char) -> *mut c_void;
    fn rhesis_boot_engine_delete(engine: *mut c_void);
    fn rhesis_boot_find_by_object_name(root: *mut c_void, name: *const c_char) -> *mut c_void;
    fn rhesis_boot_eval_bool(object: *mut c_void, expr: *const c_char, out: *mut bool) -> bool;
    fn rhesis_boot_eval_int(object: *mut c_void, expr: *const c_char, out: *mut c_int) -> bool;
    fn rhesis_boot_eval_string(
        object: *mut c_void,
        expr: *const c_char,
        out: *mut c_char,
        cap: usize,
    ) -> bool;
    fn rhesis_boot_write_string(
        object: *mut c_void,
        name: *const c_char,
        value: *const c_char,
    ) -> bool;
    fn rhesis_boot_grab_window(root: *mut c_void, path: *const c_char) -> bool;
    fn rhesis_boot_last_error() -> *const c_char;
}

#[test]
fn full_boot_drives_the_real_ui_end_to_end() {
    // Hermeticity first: sandbox, fake provider, offscreen rendering. All of
    // these must exist before Qt or a worker can read any environment.
    force_offscreen_when_unset();
    if let Err(message) = common::isolate_environment("boot-env") {
        panic!("refusing to run: {message}");
    }
    common::seed_fake_provider();
    env_logger::init();

    // ---------------------------------------------------------------------
    // Production boot sequence (mirrors run_ui in src/main.rs).
    // ---------------------------------------------------------------------
    let mut application = QApplication::new();
    if let Some(mut app) = application.as_mut() {
        use std::pin::Pin;
        // The QML Settings store inside Root.qml resolves its file location
        // from these names, so they are set before the engine loads — as in
        // main.rs, the same re-pins per call are its pattern for the
        // by-value Pin receivers.
        Pin::as_mut(&mut app).set_organization_name(&QString::from("rhesis"));
        Pin::as_mut(&mut app).set_organization_domain(&QString::from("dimkar.org"));
        Pin::as_mut(&mut app).set_application_name(&QString::from("Rhesis"));
    }

    // Persisted "background" preference -> quitOnLastWindowClosed.
    bridge::ffi::syncBackgroundBehavior();

    // Single-instance guard: the boot tier must win the abstract socket. A
    // live Rhesis (host install, flatpak, or a stale test process) would own
    // it and this UI flow would then deliberately stand down.
    if !rhesis::single_instance::acquire() {
        panic!(
            "single-instance socket already owned — is another Rhesis running? \
             stop it before running the boot tier"
        );
    }

    bridge::ffi::setupIconTheme();

    // Translations resolve the same way production does. A dev checkout keeps
    // .ts sources only, so applyLanguage failing is expected here; the side
    // effect worth exercising is the resolution order itself.
    if let Some(dir) = find_translations_dir() {
        let dir = dir.to_string_lossy().to_string();
        let preferred = persisted_language();
        bridge::ffi::applyLanguage(&QString::from(&dir), &QString::from(preferred.as_str()));
    }

    // Engine, desktop identity and KDE style, in the same order production
    // uses: the style is chosen before anything loads.
    let engine = unsafe { rhesis_boot_engine_new() };
    QGuiApplication::set_desktop_file_name(&QString::from(NAMESPACE));
    if env::var("QT_QUICK_CONTROLS_STYLE").is_err() {
        QQuickStyle::set_style(&QString::from("org.kde.desktop"));
    }

    let root = expect_probe_root(engine, &root_qml_url());

    // ---------------------------------------------------------------------
    // Assertions while the real UI runs (one QApplication, one event loop).
    // ---------------------------------------------------------------------
    let mut event_loop = QEventLoop::new();

    hold(
        &mut event_loop,
        || {
            probe_int(root, "messagingHelper.server_status").map(|status| {
                if status == STATUS_STARTED {
                    None
                } else {
                    Some(format!("worker status is {status}"))
                }
            })
        },
        WORKER_BOOT_TIMEOUT,
        "the fake provider boots the worker into Started without any server",
    );

    // Real text pipeline: the page's TextArea feeds text_area_changed, the
    // worker debounces and calls the fake provider, and the answer reaches
    // the highlighter through the production message sink.
    let text_area =
        expect_probe_find_object(root, "checkerTextArea", "the checker page's input field");
    expect_probe_write_string(text_area, "text", FIXTURE_TEXT);
    // Echo check: the property write is what fires onTextChanged, so confirm
    // it actually landed before waiting on anything downstream.
    assert_eq!(
        expect_probe_string(text_area, "text"),
        FIXTURE_TEXT,
        "the property write must reach the field before the pipeline can fire"
    );

    // Wait on the same expression the later probes assert, so a slow pipeline
    // fails as "the recommendation never landed" and not as a mixed bag of
    // unrelated value mismatches. findRecommendation returns the [start,
    // length] array (or empty); `.length` converts to a plain int, which is
    // the unambiguous settle form.
    hold(
        &mut event_loop,
        || {
            probe_int(root, "highlighter.findRecommendation(6).length").map(|bounds| {
                if bounds == 2 {
                    None
                } else {
                    Some(format!("findRecommendation(6).length = {bounds}"))
                }
            })
        },
        PIPELINE_TIMEOUT,
        "typing the recorded text produces the recommendation at [4,8)",
    );
    let like = expect_probe_string(root, "highlighter.findRecommendation(6).join(',')");
    assert_eq!(
        like, "4,8",
        "the grammar group starts at 'like' and its end is exclusive"
    );

    let gap = expect_probe_int(root, "highlighter.findRecommendation(8).length");
    assert_eq!(gap, 0, "between words nothing covers the gap index");

    // The context-menu path: MainPage calls getSuggestions(start, end); both
    // fixture groups pass the length-based filter, so the recorded
    // replacements come back in fixture order.
    let inside = expect_probe_string(
        root,
        "highlighter.getSuggestions(4,8).map(r => r.value).join('|')",
    );
    assert_eq!(
        inside, "is like|likes|quickly",
        "the menu lists every recorded replacement admitted by the filter"
    );

    let quicly = expect_probe_string(root, "highlighter.findRecommendation(38).join(',')");
    assert_eq!(
        quicly, "38,44",
        "the typo group starts at 'quicly' and ends at the period"
    );

    let spelling_only = expect_probe_string(
        root,
        "highlighter.getSuggestions(38,44).map(r => r.value).join('|')",
    );
    assert_eq!(
        spelling_only, "quickly",
        "clicking a later group offers only its own replacements"
    );

    // Clean text: empty answers replace the previous recommendation set.
    expect_probe_write_string(text_area, "text", CLEAN_TEXT);
    hold(
        &mut event_loop,
        || {
            probe_bool(
                root,
                "Array.from({length: 30}, (_, i) => \
                 highlighter.findRecommendation(i).length).every(len => len === 0) \
                 && highlighter.getSuggestions(0,999).length === 0",
            )
            .map(|clean| {
                if clean {
                    None
                } else {
                    Some("the clean-text scan still finds recommendations".to_string())
                }
            })
        },
        PIPELINE_TIMEOUT,
        "the recorded clean answer removes all matches",
    );

    // Autostart lifecycle through the production invokables, with the
    // file-based state read back by the production functions on the Rust side.
    let created = expect_probe_bool(root, "messagingHelper.set_autostart(true)");
    assert!(
        created,
        "the sandboxed config root accepts an autostart entry"
    );
    assert!(
        rhesis::autostart::is_enabled(),
        "set_autostart(true) wrote the desktop entry"
    );
    assert!(
        expect_probe_bool(root, "messagingHelper.is_autostart()"),
        "the file-based state flows back through the helper"
    );
    let removed = expect_probe_bool(root, "messagingHelper.set_autostart(false)");
    assert!(removed, "the sandbox accepts removing the entry again");
    assert!(!rhesis::autostart::is_enabled(), "the entry is gone");

    // QSettings persistence roundtrip: the store write lands in the sandboxed
    // INI file whose path the production reader resolves (org/app identity
    // is set like in main.rs, so no explicit location pinning is needed).
    // QQmlSettings does not write synchronously: a property change is cached
    // and stored by a 500ms timer (`settingsWriteDelay` in Qt sources), and
    // sync() only flushes the QSettings buffer without triggering it. The
    // holds below therefore pump the loop until the timer's write has
    // actually reached the file — flushing per poll — so the deadline adapts
    // to whatever delay the Qt build really uses.
    let conf = common::sandbox_root("boot-env").join("config/rhesis/Rhesis.conf");
    let assigned = expect_probe_string(root, "appSettings.language = 'el_GR'");
    assert_eq!(
        assigned, "el_GR",
        "the assignment expression's value confirms the new store state"
    );
    hold(
        &mut event_loop,
        || {
            probe_int(root, "(appSettings.sync(), 1)")?;
            let stored = std::fs::read_to_string(&conf).unwrap_or_default();
            if stored.contains("language=el_GR") {
                Ok(None)
            } else {
                Ok(Some(format!(
                    "the store still holds {:?}",
                    stored
                        .lines()
                        .filter(|line| line.starts_with("language"))
                        .collect::<Vec<_>>()
                )))
            }
        },
        SETTINGS_STORE_TIMEOUT,
        "the delayed settings write stores the assigned language",
    );
    assert_eq!(
        persisted_language(),
        "el_GR",
        "the production reader sees the persisted language"
    );

    expect_probe_string(root, "appSettings.language = ''");
    hold(
        &mut event_loop,
        || {
            probe_int(root, "(appSettings.sync(), 1)")?;
            let stored = std::fs::read_to_string(&conf).unwrap_or_default();
            if !stored.contains("language=el_GR") {
                Ok(None)
            } else {
                Ok(Some("the store still holds el_GR".to_string()))
            }
        },
        SETTINGS_STORE_TIMEOUT,
        "the delayed settings write stores the reset",
    );
    assert_eq!(
        persisted_language(),
        "",
        "resetting the store falls back to the system locale"
    );

    // Single-instance, end to end: the runner thread is up in this process
    // (Root's Component.onCompleted started it next to the message thread),
    // so a real launch of the shipped binary connects, nudges this instance
    // and exits without any UI of its own.
    assert_secondary_instance_stands_down();

    // Evidence capture (best effort; a null grab is a diagnostic, not a
    // behavior contract). Pictures are a local-development tool, so a CI run
    // (where `CI` is set by the runner) generates none.
    if std::env::var_os("CI").is_none() {
        let screenshot = sandbox_root().join("boot-window.png");
        if save_boot_screenshot(root, &screenshot) {
            println!("boot screenshot: {}", screenshot.display());
        } else {
            println!(
                "no boot screenshot ({}): {}",
                probe_explanation(),
                screenshot.display()
            );
        }
    }

    // Teardown order matters: the engine must die while Qt is alive, so the
    // QML teardown (and with it the worker Kill) runs inside a valid app.
    unsafe { rhesis_boot_engine_delete(engine) };
    // Qt's application object is intentionally leaked (the library harness in
    // src/ui_tests.rs lets quick_test_main do the same): dropping a live
    // QApplication under the test harness is different production territory.
    std::mem::forget(application);
}

// ----------------------------------------------------------------------
// Boot plumbing
// ----------------------------------------------------------------------

/// The root document URL, decomposed the same way main.rs builds it from the
/// app id (dots become the path separators).
fn root_qml_url() -> String {
    format!(
        "qrc:/qt/qml/{}/src/interop/qml/Root.qml",
        NAMESPACE.replace('.', "/")
    )
}

/// Loads the root document, failing with a probe-style message so a load
/// error names itself instead of surfacing as a bare null.
fn expect_probe_root(engine: *mut c_void, url: &str) -> *mut c_void {
    probe_engine_load(engine, url)
        .unwrap_or_else(|detail| panic!("loading Root.qml failed: {detail}"))
}

/// Polls a condition across the Qt event loop, failing with `reason` when the
/// deadline expires — the Rust-side twin of `tests/ui/harness.js :: hold`.
/// Probe internals surface immediately, so a broken expression never shows up
/// as an unrelated timeout.
///
/// The predicate returns `None` once settled; otherwise it produces the
/// current state as a diagnostic, and the timeout panic carries the last one
/// so a slow pipeline fails with evidence instead of just a deadline.
fn hold(
    event_loop: &mut UniquePtr<QEventLoop>,
    predicate: impl Fn() -> Result<Option<String>, String>,
    timeout: Duration,
    reason: &str,
) {
    let deadline = Instant::now() + timeout;
    loop {
        // Each iteration's observation doubles as the timeout diagnostic;
        // the freshest state is the interesting one, so nothing is carried
        // across iterations.
        let state = match predicate() {
            Ok(Some(state)) => Some(state),
            Ok(None) => return,
            Err(probe_detail) => {
                panic!("while holding for ({reason}) the QML probe failed: {probe_detail}")
            }
        };
        if Instant::now() >= deadline {
            panic!(
                "{reason} (deadline exceeded); last state: {}",
                state.unwrap_or_else(|| "no observation yet".to_string())
            );
        }
        // The unused generic parameter is a phantom in the cxx-qt-lib
        // binding; the turbofish keeps the same call shape production uses.
        event_loop
            .pin_mut()
            .process_all_events_until::<()>(PUMP_TICK);
        std::thread::sleep(PUMP_PAUSE);
    }
}

// ----------------------------------------------------------------------
// Probe wrappers: unsafe stays here, everything else reads like Rust
// ----------------------------------------------------------------------

/// Reads back the last failure detail recorded by the C++ probes.
fn probe_explanation() -> String {
    unsafe {
        CStr::from_ptr(rhesis_boot_last_error())
            .to_string_lossy()
            .into_owned()
    }
}

fn probe_find_object(root: *mut c_void, name: &str) -> Option<*mut c_void> {
    let name = cstring(name);
    let found = unsafe { rhesis_boot_find_by_object_name(root, name.as_ptr()) };
    (!found.is_null()).then_some(found)
}

/// Finds `name` in the object tree, failing with a probe-style message that
/// carries the caller's context.
fn expect_probe_find_object(root: *mut c_void, name: &str, context: &str) -> *mut c_void {
    probe_find_object(root, name)
        .unwrap_or_else(|| panic!("QML object lookup failed: no objectName {name:?} — {context}"))
}

fn probe_bool(root: *mut c_void, expr: &str) -> Result<bool, String> {
    let expr = cstring(expr);
    let mut out = false;
    let decoded = unsafe { rhesis_boot_eval_bool(root, expr.as_ptr(), &mut out) };
    if decoded {
        Ok(out)
    } else {
        Err(probe_explanation())
    }
}

fn expect_probe_bool(root: *mut c_void, expr: &str) -> bool {
    probe_bool(root, expr).unwrap_or_else(|detail| panic!("QML probe {expr:?} failed: {detail}"))
}

fn probe_int(root: *mut c_void, expr: &str) -> Result<i32, String> {
    let expr = cstring(expr);
    let mut out: c_int = 0;
    let decoded = unsafe { rhesis_boot_eval_int(root, expr.as_ptr(), &mut out) };
    if decoded {
        Ok(out)
    } else {
        Err(probe_explanation())
    }
}

fn expect_probe_int(root: *mut c_void, expr: &str) -> i32 {
    probe_int(root, expr).unwrap_or_else(|detail| panic!("QML probe {expr:?} failed: {detail}"))
}

fn probe_string(root: *mut c_void, expr: &str) -> Result<String, String> {
    let expr = cstring(expr);
    let mut buffer = vec![0 as c_char; PROBE_BUFFER_SIZE];
    let decoded = unsafe {
        rhesis_boot_eval_string(root, expr.as_ptr(), buffer.as_mut_ptr(), PROBE_BUFFER_SIZE)
    };
    if decoded {
        Ok(unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned())
    } else {
        Err(probe_explanation())
    }
}

fn expect_probe_string(root: *mut c_void, expr: &str) -> String {
    probe_string(root, expr).unwrap_or_else(|detail| panic!("QML probe {expr:?} failed: {detail}"))
}

fn probe_write_string(root: *mut c_void, name: &str, value: &str) -> Result<(), String> {
    let name = cstring(name);
    let value = cstring(value);
    let written = unsafe { rhesis_boot_write_string(root, name.as_ptr(), value.as_ptr()) };
    if written {
        Ok(())
    } else {
        Err(probe_explanation())
    }
}

fn expect_probe_write_string(root: *mut c_void, name: &str, value: &str) {
    probe_write_string(root, name, value).unwrap_or_else(|detail| {
        panic!("QML property write failed ({name:?} := {value:?}): {detail}")
    });
}

/// Loads the root document, failing with the C++ probe's stored detail so a
/// load error names itself instead of surfacing as a bare null.
fn probe_engine_load(engine: *mut c_void, url: &str) -> Result<*mut c_void, String> {
    let url = cstring(url);
    let root = unsafe { rhesis_boot_engine_load(engine, url.as_ptr()) };
    if root.is_null() {
        Err(probe_explanation())
    } else {
        Ok(root)
    }
}

/// Saves the boot evidence screenshot; the error detail is reported through
/// [`probe_explanation`] (kept best-effort on purpose).
fn save_boot_screenshot(root: *mut c_void, path: &Path) -> bool {
    unsafe { rhesis_boot_grab_window(root, cstring(&path.to_string_lossy()).as_ptr()) }
}

// ----------------------------------------------------------------------
// Secondary-instance launch (the real binary, the real contract)
// ----------------------------------------------------------------------

/// Launches the shipped binary the way an autostart opener would (`--hidden`)
/// and asserts the single-instance contract: it nudges the running instance
/// (standing right here in this test process) and exits without a UI of its
/// own.
fn assert_secondary_instance_stands_down() {
    let mut guard = ChildGuard {
        child: Command::new(env!("CARGO_BIN_EXE_rhesis"))
            .arg("--hidden")
            // The secondary never maps a window; offscreen keeps its boot
            // deterministic even for a watched primary run.
            .env("QT_QPA_PLATFORM", "offscreen")
            // The stand-down decision must be visible in the captured log.
            .env("RUST_LOG", "info")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap_or_else(|e| panic!("failed to launch the secondary instance: {e:?}")),
    };

    let status = wait_for_exit(
        &mut guard.child,
        STAND_DOWN_TIMEOUT,
        "the secondary instance must stand down on its own",
    );
    assert_eq!(
        status.code(),
        Some(0),
        "the secondary's stand-down path exits cleanly"
    );

    let stderr = guard
        .child
        .stderr
        .take()
        .expect("stderr was just piped for the secondary instance");
    let mut collected = String::new();
    BufReader::new(stderr)
        .read_to_string(&mut collected)
        .unwrap_or_else(|e| panic!("failed to read the secondary's log: {e:?}"));
    assert!(
        collected.contains("another Rhesis instance is running"),
        "the secondary must log the stand-down decision; captured: {collected:?}"
    );
}

/// Kills and reaps its child on drop, so a failing assertion cannot leak the
/// launched process.
struct ChildGuard {
    child: Child,
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Polls the child exit with a deadline; the standard library's `Child` has
/// no bounded wait, so this is the wait(2)-style helper the test needs.
fn wait_for_exit(child: &mut Child, timeout: Duration, reason: &str) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) => {}
            Err(e) => panic!("{reason}: failed to poll the process: {e:?}"),
        }
        if Instant::now() >= deadline {
            panic!("{reason} (deadline exceeded)");
        }
        std::thread::sleep(STAND_DOWN_POLL);
    }
}
