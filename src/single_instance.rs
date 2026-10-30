//! Single-instance guard.
//!
//! Linux: an abstract unix domain socket (`abstract` names live in the
//! network namespace, so the flatpak sandbox shares state with host-side
//! installs without any filesystem permissions). The first instance binds
//! it and keeps listening; later launches send it a `show` request — their
//! window would have been hidden behind the tray — and exit immediately.
//!
//! Windows: a session-local named mutex decides who is primary and a named
//! event carries the `show` request (see `windows.rs`), behind the same
//! `acquire`/`spawn_runner` interface.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{acquire, spawn_runner};

#[cfg(not(windows))]
use std::{
    io::Write,
    os::linux::net::SocketAddrExt,
    os::unix::net::{SocketAddr, UnixListener, UnixStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock,
    },
    thread::JoinHandle,
};

/// Reserved-only app id: doubles as the abstract socket name, which stays
/// well below the 107-byte socket address limit.
pub const SOCKET_NAME: &[u8] = b"io.github.dimkar3000.rhesis";

/// Payload a secondary instance sends to the primary one.
pub const SHOW_REQUEST: &[u8] = b"show\n";

/// Bound while acquiring, consumed by `spawn_runner` afterwards: the listener
/// has to come up before any UI does (a second launch happening during the
/// QML setup phase must fail its connect), but the dispatch thread only has
/// a Qt thread handle once the QML objects exist.
#[cfg(not(windows))]
static LISTENER: OnceLock<UnixListener> = OnceLock::new();

#[cfg(not(windows))]
static RUNNER_SPAWNED: AtomicBool = AtomicBool::new(false);

#[cfg(not(windows))]
fn socket_addr() -> SocketAddr {
    SocketAddr::from_abstract_name(SOCKET_NAME).expect("socket name is short and valid")
}

/// Bind the singleton socket on loopback terms: `true` when this process is
/// the primary instance (and owns the listener), `false` when another
/// instance is running — in which case it has been asked to show its window
/// and this process should exit right away.
#[cfg(not(windows))]
pub fn acquire() -> bool {
    let addr = socket_addr();
    match UnixListener::bind_addr(&addr) {
        Ok(listener) => {
            // Kernel-level backlog: connections made before the runner thread
            // exists are queued until `spawn_runner` takes over, so there is
            // no gap where a second launch could think it's primary.
            let _ = LISTENER.set(listener);
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Another instance is alive; its connect answers (even before the
            // runner accepts) because the socket file is live. Nudge it and
            // stand down.
            let known_instance = UnixStream::connect_addr(&addr)
                .map(|mut stream| stream.write_all(SHOW_REQUEST).is_ok())
                .unwrap_or(false);
            if !known_instance {
                log::warn!("single-instance socket is in use but did not accept a nudge");
            }
            false
        }
        Err(e) => {
            // Degraded mode (e.g. unsupported sandbox): run as primary rather
            // than failing the launch — two instances would then fight over
            // the LanguageTool port, which the adoption logic resolves.
            log::warn!("single-instance guard unavailable, continuing without it: {e:?}");
            true
        }
    }
}

/// Take over listener duty once a Qt thread is available (called from
/// `start_message_thread`, right after the worker comes up). Each `show`
/// request is marshaled to the GUI thread, where the hidden window becomes
/// visible and focused. The thread intentionally runs until process exit;
/// joining it would block teardown on a pending accept.
#[cfg(not(windows))]
pub fn spawn_runner(
    qt_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
) -> Option<JoinHandle<()>> {
    if RUNNER_SPAWNED.swap(true, Ordering::SeqCst) {
        log::debug!("single-instance runner already running");
        return None;
    }

    let listener = match LISTENER.get() {
        Some(listener) => match listener.try_clone() {
            Ok(listener) => listener,
            Err(e) => {
                log::error!("failed to clone the single-instance listener: {e:?}");
                RUNNER_SPAWNED.store(false, Ordering::SeqCst);
                return None;
            }
        },
        None => {
            // acquire() always binds before this point; treat a violation
            // as a recoverable no-op rather than a broken feature.
            log::debug!("no single-instance listener is bound");
            RUNNER_SPAWNED.store(false, Ordering::SeqCst);
            return None;
        }
    };

    match std::thread::Builder::new()
        .name("single-instance".to_string())
        .spawn(move || run_loop(listener, qt_thread))
    {
        Ok(handle) => Some(handle),
        Err(e) => {
            log::error!("failed to spawn single-instance runner thread: {e:?}");
            RUNNER_SPAWNED.store(false, Ordering::SeqCst);
            None
        }
    }
}

#[cfg(not(windows))]
fn run_loop(
    listener: UnixListener,
    qt_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
) {
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => handle_stream(stream, &qt_thread),
            Err(e) => {
                log::warn!("single-instance accept failed: {e:?}");
            }
        }
    }
}

#[cfg(not(windows))]
fn handle_stream(
    mut stream: UnixStream,
    qt_thread: &cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
) {
    _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
    let mut buffer = [0u8; 16];
    match std::io::Read::read(&mut stream, &mut buffer) {
        Ok(_) if buffer.starts_with(SHOW_REQUEST.trim_ascii()) => {
            match qt_thread.queue(move |_helper| {
                crate::interop::bridge::ffi::appRaiseMainWindow();
            }) {
                Ok(_) => (),
                Err(e) => log::warn!("failed to queue show request to the UI thread: {e:?}"),
            }
        }
        Ok(_) => log::debug!("single-instance request ignored (unknown payload)"),
        Err(e) => log::debug!("single-instance request read failed: {e:?}"),
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
}
