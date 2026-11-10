//! Cross-platform single-instance guard backed by the `app-single-instance` crate.
//!
//! The crate provides cross-platform single-instance enforcement with
//! process-to-process notification: a second launch wakes the primary,
//! which can then raise its window. Linux/macOS use a Unix domain socket;
//! Windows uses a named pipe. `interprocess` (already a rhesis dependency)
//! handles both transports.

use std::sync::{Mutex, OnceLock};

use app_single_instance::{notify_if_running, start_primary, PrimaryHandle};

/// Unique identifier for Rhesis's single-instance lock and socket.
const APP_ID: &str = "io.github.dimkar3000.rhesis";

/// The primary's listener handle, kept alive for the process lifetime.
/// `PrimaryHandle` is `Send` but not `Sync`, so it is stored behind a `Mutex`
/// inside a `OnceLock`.
static PRIMARY_HANDLE: OnceLock<Mutex<PrimaryHandle>> = OnceLock::new();

/// Bind the singleton lock via `app-single-instance`. Returns `true` when
/// this process is the primary instance, `false` when another instance is
/// running — in which case it has been requested to show its window and
/// this process should exit.
pub fn acquire() -> bool {
    !notify_if_running(APP_ID)
}

/// Register the primary instance's listener. Each "show" request from a
/// later launch invokes `on_show` on the listener thread. Returns `true`
/// if the listener was started, `false` if it was already running.
pub fn spawn_runner(on_show: impl Fn() + Send + 'static) -> bool {
    if PRIMARY_HANDLE.get().is_some() {
        log::debug!("single-instance runner already running");
        return false;
    }
    let handle = start_primary(APP_ID, on_show);
    PRIMARY_HANDLE.set(Mutex::new(handle)).is_ok()
}

/// Check whether a primary instance is listening and, if so, request it to
/// show its window. Returns `true` if a primary was found and notified.
/// Used by the single-instance E2E test to wait for the primary to be ready.
pub fn ping_primary() -> bool {
    notify_if_running(APP_ID)
}
