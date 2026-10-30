//! Windows single-instance guard.
//!
//! A named mutex says who is the primary instance; a named auto-reset event
//! carries the "show your window" request from a later launch. Both live in
//! the `Local\` namespace: one Rhesis per login session, so several users on
//! the same machine (or Remote Desktop sessions) do not block each other.
//!
//! Same interface as the Linux guard (`acquire` / `spawn_runner`), so main.rs
//! and the tests are platform-neutral.

use std::{
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    thread::JoinHandle,
};

use windows_sys::Win32::{
    Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0},
    System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE},
};

const MUTEX_NAME: &str = r"Local\io.github.dimkar3000.rhesis.instance";
const EVENT_NAME: &str = r"Local\io.github.dimkar3000.rhesis.show";

struct OwnedHandle(HANDLE);

// SAFETY: a kernel handle is an opaque value that any thread may use.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

/// The primary instance's handles. Held for the whole process lifetime: the
/// mutex must stay open for later launches to see it, and the system closes
/// both when the process exits, however it exits.
struct Guard {
    _mutex: OwnedHandle,
    show_event: OwnedHandle,
}

static GUARD: OnceLock<Guard> = OnceLock::new();

static RUNNER_SPAWNED: AtomicBool = AtomicBool::new(false);

fn wide(name: &str) -> Vec<u16> {
    name.encode_utf16().chain(std::iter::once(0)).collect()
}

/// `true` when this process is the primary instance. `false` when another
/// instance is running; it has then been asked to show its window and this
/// process should exit right away.
pub fn acquire() -> bool {
    let event_name = wide(EVENT_NAME);
    let mutex_name = wide(MUTEX_NAME);

    // SAFETY: null attributes are valid (default security), and the names are
    // NUL-terminated UTF-16 that outlive the calls.
    let event = unsafe { CreateEventW(ptr::null(), 0, 0, event_name.as_ptr()) };
    if event.is_null() {
        // Degraded mode: run as primary rather than failing the launch.
        log::warn!(
            "single-instance guard unavailable, continuing without it: {}",
            std::io::Error::last_os_error()
        );
        return true;
    }

    // The event is created before the mutex on purpose: whoever sees an
    // existing mutex is guaranteed an existing event to signal.
    // SAFETY: as above.
    let mutex = unsafe { CreateMutexW(ptr::null(), 0, mutex_name.as_ptr()) };
    // SAFETY: read straight after the call it describes.
    let already_running = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if mutex.is_null() {
        log::warn!(
            "single-instance guard unavailable, continuing without it: {}",
            std::io::Error::last_os_error()
        );
        return true;
    }

    if already_running {
        // SAFETY: `event` is a valid event handle created above.
        if unsafe { SetEvent(event) } == 0 {
            log::warn!(
                "single-instance event could not be signaled: {}",
                std::io::Error::last_os_error()
            );
        }
        return false;
    }

    let _ = GUARD.set(Guard {
        _mutex: OwnedHandle(mutex),
        show_event: OwnedHandle(event),
    });
    true
}

/// Take over listener duty once a Qt thread is available: each `show`
/// request is marshaled to the GUI thread, where the hidden window becomes
/// visible and focused. The thread runs until process exit.
pub fn spawn_runner(
    qt_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
) -> Option<JoinHandle<()>> {
    if RUNNER_SPAWNED.swap(true, Ordering::SeqCst) {
        log::debug!("single-instance runner already running");
        return None;
    }

    let Some(guard) = GUARD.get() else {
        // acquire() stores the guard before this point; a degraded launch
        // has none, which is a recoverable no-op.
        log::debug!("no single-instance event is bound");
        RUNNER_SPAWNED.store(false, Ordering::SeqCst);
        return None;
    };
    // Raw handles are not Send; the guard (and so the handle) is 'static.
    let event = guard.show_event.0 as usize;

    match std::thread::Builder::new()
        .name("single-instance".to_string())
        .spawn(move || run_loop(event as HANDLE, qt_thread))
    {
        Ok(handle) => Some(handle),
        Err(e) => {
            log::error!("failed to spawn single-instance runner thread: {e:?}");
            RUNNER_SPAWNED.store(false, Ordering::SeqCst);
            None
        }
    }
}

fn run_loop(
    event: HANDLE,
    qt_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
) {
    loop {
        // SAFETY: `event` stays valid for the process lifetime (see Guard).
        let result = unsafe { WaitForSingleObject(event, INFINITE) };
        if result != WAIT_OBJECT_0 {
            log::warn!("single-instance wait failed ({result}); stopping the runner");
            return;
        }
        if let Err(e) = qt_thread.queue(move |_helper| {
            crate::interop::bridge::ffi::appRaiseMainWindow();
        }) {
            log::warn!("failed to queue show request to the UI thread: {e:?}");
        }
    }
}
