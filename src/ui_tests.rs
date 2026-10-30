//! Qt Quick Test entry point for the page-level suites (`tests/ui/*.qml`).
//!
//! The crate's statically registered QML module and its recorded Qt/native
//! link directives only reach a binary that actually references the lib.
//! The runner binary simply calls [`run_suites`] — a genuine use of the lib —
//! so no dummy link anchors are needed to pull the linkage in.
//!
//! The companion C++ glue lives next to this file in `src/ui_tests/wrapper.cpp`
//! (compiled unconditionally by build.rs).
//!
//! The suite files live under `tests/ui/` beside the test fixtures; a
//! trailing CLI argument selects a different root, as qmltestrunner does:
//!
//! ```sh
//! QT_QPA_PLATFORM=offscreen build/target/debug/rhesis-ui-runner          # all suites
//! QT_QPA_PLATFORM=offscreen build/target/debug/rhesis-ui-runner tests/ui # same
//! ```

use std::ffi::CString;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};

extern "C" {
    fn rhesis_quick_test_main(
        argc: c_int,
        argv: *mut *mut c_char,
        name: *const c_char,
        source_dir: *const c_char,
    ) -> c_int;
}

/// Test suite directory handed to Quick Test; every `tst_*.qml` inside is a
/// separate suite.
fn suite_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ui")
}

/// Runner process name and Quick Test suite id, reported in the output
/// banner (`********* Start testing of rhesis-ui-tests *********`).
const TEST_ID: &str = "rhesis-ui-tests";

/// Fixture id served to worker requests when the caller did not pick one
/// (`RHESIS_FAKE_LT=<id>` overridable, see `CompletionProvider::chosen`).
const DEFAULT_FAKE_FIXTURE: &str = "checker";

/// The sandbox lives inside the build frame so `cargo clean`-adjacent churn
/// never pollutes a real user configuration, and failing runs leave their
/// state around for inspection (it is truncated at every launch).
fn sandbox_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("build/ui-tests/harness-env")
}

/// Runs Qt Quick Test over the suites under [`suite_root`], guaranteeing
/// hermeticity first: the fake completion provider is seeded (no network,
/// no embedded java server) and every XDG home the test process could touch
/// is redirected into a fresh sandbox directory, before Qt or the worker
/// read any environment. Screenshots land in `build/tests/` beneath the
/// project root, named `<suite>_<test>[_<label>]_<PASS|FAIL>.png`.
///
/// Returns Qt's exit code; the runner binary is expected to exit with it.
pub fn run_suites() -> i32 {
    seed_fake_provider();
    if let Err(message) = isolate_environment() {
        eprintln!("ui-tests: refusing to run: {message}");
        return 1;
    }

    // Render the suites with the same controls style and embedded icon theme
    // as the app (`main.rs`). Without these the offscreen screenshots show
    // the plain Qt style: switches look like two adjacent boxes and the
    // icon-only reset/remove buttons come out as empty rectangles, which
    // reads as duplicated widgets.
    crate::interop::bridge::ffi::setupIconTheme();
    if std::env::var_os("QT_QUICK_CONTROLS_STYLE").is_none() {
        std::env::set_var("QT_QUICK_CONTROLS_STYLE", "org.kde.desktop");
    }

    // Every suite saves its pictures here (`tests/ui/harness.js :: capture`
    // derives the same path from its own location). Truncated per launch so
    // the directory always shows the latest run — a stale PASS from an
    // earlier build is worse than no image at all.
    let screenshots_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("build/tests");
    if let Err(e) = std::fs::remove_dir_all(&screenshots_dir) {
        if e.kind() != std::io::ErrorKind::NotFound {
            eprintln!(
                "ui-tests: cannot truncate screenshot dir {}: {e}",
                screenshots_dir.display()
            );
            return 1;
        }
    }
    if let Err(e) = std::fs::create_dir_all(&screenshots_dir) {
        eprintln!(
            "ui-tests: cannot create screenshot dir {}: {e}",
            screenshots_dir.display()
        );
        return 1;
    }

    let argv = collect_runner_arguments();
    let name = CString::new(TEST_ID).expect("suite id contains no interior NUL");
    let suite_dir = CString::new(
        suite_root()
            .to_str()
            .expect("suite root path is valid UTF-8"),
    )
    .expect("suite root path contains no interior NUL");

    let mut argv_ptrs: Vec<*mut c_char> =
        argv.iter().map(|arg| arg.as_ptr() as *mut c_char).collect();
    argv_ptrs.push(std::ptr::null_mut());

    // SAFETY: `rhesis_quick_test_main` is Qt's plain C++-mangled entry point
    // behind the wrapper. The CStrings outlive the call (bound above the
    // block), contain no interior NULs, and the pointer array plus the
    // terminating null is an owned allocation.
    unsafe {
        rhesis_quick_test_main(
            argv_ptrs.len() as c_int - 1,
            argv_ptrs.as_mut_ptr(),
            name.as_ptr(),
            suite_dir.as_ptr(),
        )
    }
}

/// Selects the recorded-checker fixture unless the caller already named one.
/// Worker selection reads the variable when the worker starts — which is
/// after this — so release behavior only changes for harness runs.
fn seed_fake_provider() {
    if std::env::var_os("RHESIS_FAKE_LT").is_none() {
        std::env::set_var("RHESIS_FAKE_LT", DEFAULT_FAKE_FIXTURE);
    }
}

/// Builds the CLI Quick Test expects: argv[0] is the process name, an
/// optional trailing argument selects the suite root.
fn collect_runner_arguments() -> Vec<CString> {
    let mut args = vec![CString::new(TEST_ID).expect("suite id contains no interior NUL")];
    if let Some(directory) = std::env::args().nth(1) {
        args.push(CString::new(directory).expect("argument contains no interior NUL"));
    }
    args
}

/// Redirects every XDG-style home the process could write (config, data,
/// caches, sockets, `$HOME`) into a fresh sandbox under
/// [`sandbox_root`], so suites can freely exercise, e.g., the autostart
/// desktop files and `QSettings` persistence without ever touching the
/// real user configuration. Must run before Qt is instantiated.
#[cfg(unix)]
fn isolate_environment() -> Result<(), String> {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let roots = [
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_DATA_HOME", "data"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_RUNTIME_DIR", "runtime"),
        ("HOME", "home"),
    ];

    for (variable, subdir) in roots {
        let directory = sandbox_root().join(subdir);
        if let Err(e) = fs::remove_dir_all(&directory) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("cannot truncate {}: {e}", directory.display()));
            }
        }
        fs::create_dir_all(&directory)
            .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
        // Same restriction mode as XDG_RUNTIME_DIR demands.
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("cannot restrict {}: {e}", directory.display()))?;
        std::env::set_var(variable, &directory);
    }

    Ok(())
}

/// No-op fallback for platform variants whose directory layout the sandbox
/// does not model yet; the runner is only built for local/CI Linux anyway.
#[cfg(not(unix))]
fn isolate_environment() -> Result<(), String> {
    Ok(())
}
