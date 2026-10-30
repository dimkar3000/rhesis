//! Qt Quick Test suites (`tests/ui/*.qml`), driven as a subprocess.
//!
//! Quick Test owns `QGuiApplication` and parses its own command line, so it
//! cannot share a process with the Rust test harness: this target launches
//! `rhesis-ui-runner` (a one-line shim over `rhesis::ui_tests::run_suites`)
//! and asserts its exit code. The runner keeps itself hermetic — it seeds
//! `RHESIS_FAKE_LT` and redirects every XDG home into
//! `build/ui-tests/harness-env` before Qt starts — so the child needs no
//! setup from here beyond a platform plugin that can render offscreen.
//!
//! Serialization: exactly one test, and the child is the only Quick Test
//! instance in flight, so `--test-threads=1` is not required.

use std::process::Command;

/// Platform plugin for the child: offscreen is deterministic and works
/// headless (CI), while an explicit caller choice (`QT_QPA_PLATFORM`) wins
/// so the suites can be replayed on a real display when debugging.
fn platform() -> String {
    std::env::var("QT_QPA_PLATFORM").unwrap_or_else(|_| "offscreen".to_string())
}

#[test]
fn qml_suites_pass() {
    let output = Command::new(env!("CARGO_BIN_EXE_rhesis-ui-runner"))
        .env("QT_QPA_PLATFORM", platform())
        .output()
        .expect("failed to launch rhesis-ui-runner");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "QML suites failed ({})\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}",
        output.status
    );

    // A runner that found no suite at all still exits 0; require the
    // evidence that suites actually executed.
    assert!(
        stdout.contains("Start testing of rhesis-ui-tests"),
        "the runner reported no suite execution:\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    );
}
