//! Single-instance guard, verified at the process level: two real launches
//! of the shipped binary.
//!
//! The first launch becomes primary and stays alive with its UI (its own
//! full boot, driven further by the real worker under the fake provider);
//! the second launch detects the machine-global abstract socket name, nudges
//! the running instance to show its window, and exits without a UI of its
//! own.
//!
//! The socket name lives in the network namespace (see `src/single_instance.rs`),
//! so the XDG sandbox does not isolate it: the test refuses to run while
//! another Rhesis instance owns the name. Stop any running instance (or
//! stale earlier run) first.
//!
//! ```sh
//! cargo test --test single-instance-e2e -- --test-threads=1
//! ```

mod common;

use std::io::{BufRead, BufReader};
use std::os::linux::net::SocketAddrExt;
use std::os::unix::net::{SocketAddr, UnixListener, UnixStream};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Socket name copied from single_instance.rs (its constant is private to
/// that module — the test reads the same address the guard binds).
const SOCKET_NAME: &[u8] = b"io.github.dimkar3000.rhesis";

/// How long each observable step may take: the primary must offer its
/// guard socket quickly, the secondary must stand down without a UI, and
/// the primary's own boot (engine load included) must reach the event loop.
const LISTENER_TIMEOUT: Duration = Duration::from_secs(15);
const STAND_DOWN_TIMEOUT: Duration = Duration::from_secs(30);
const BOOT_TIMEOUT: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(100);

/// One stream drained into shared memory so assertions can read it after the
/// child is gone.
type SharedLog = Arc<Mutex<String>>;

#[test]
fn second_real_launch_stands_down_against_the_first() {
    // Preconditions: nothing owns the machine-global socket yet, and the
    // children share the hermetic sandbox (their QSettings, caches, and the
    // fake provider live under build/ui-tests/instance-env).
    assert_socket_is_free();
    if let Err(message) = common::isolate_environment("instance-env") {
        panic!("refusing to run: {message}");
    }
    common::seed_fake_provider();

    // Primary: a real UI boot. `--hidden` is the autostart opener's shape;
    // the fake worker answers its checks, so no LanguageTool is spawned.
    let mut primary = ChildGuard::new(launch("primary", &["--hidden"]), "the primary instance");

    // The primary binds the guard socket before any UI exists (see the
    // `acquire` doc): a connect success proves the guard is live. This
    // throwaway connect is ignored by production ("no-payload" rule, see
    // handle_stream), so it does not disturb the later real request.
    wait_for_listener(LISTENER_TIMEOUT);

    // Secondary: same binary, no flags — a plain manual launch. Its only
    // correct behavior is: nudge the primary, log the stand-down decision,
    // and exit cleanly WITHOUT ever booting a UI.
    let mut secondary = ChildGuard::new(launch("secondary", &[]), "the secondary instance");
    let status = wait_for_exit(
        &mut secondary.child,
        STAND_DOWN_TIMEOUT,
        "the secondary instance must stand down on its own",
    );
    assert_eq!(
        status.code(),
        Some(0),
        "the secondary's stand-down path exits cleanly"
    );
    // The drainer reaches EOF only after the child is gone; joining it makes
    // the captured log complete before the assertions below read it.
    secondary.join_drainer();
    let secondary_log = secondary.log();
    assert!(
        secondary_log.contains("another Rhesis instance is running"),
        "the secondary must log the stand-down decision; captured: {secondary_log:?}"
    );
    assert!(
        !secondary_log.contains("Initialized"),
        "the secondary must not boot its own UI; captured: {secondary_log:?}"
    );

    // The primary survived the nudge (its guard thread queued the show
    // request into the Qt loop) and its full boot reached the event loop:
    // both together prove the real binary boots, stays primary and services
    // a later launch instead of crashing on it.
    assert!(
        primary
            .child
            .try_wait()
            .expect("the primary instance must be pollable")
            .is_none(),
        "the primary stays alive after servicing a later launch"
    );
    // The primary boots in parallel with everything above — the guard socket
    // lives before the UI — so the event-loop milestone is waited for rather
    // than asserted against a snapshot taken while the engine still loads.
    wait_for_log(
        &primary,
        "Initialized",
        BOOT_TIMEOUT,
        "the primary's full boot must reach the event loop",
    );

    // Both children are reaped by their guards on drop.
}

// ----------------------------------------------------------------------
// Process plumbing
// ----------------------------------------------------------------------

fn socket_addr() -> SocketAddr {
    // Mirrors single_instance.rs's own address construction; the constant is
    // short enough that this cannot fail.
    SocketAddr::from_abstract_name(SOCKET_NAME).expect("socket name is short and valid")
}

/// Ensures the guard socket is free this very moment, by trying to own it
/// and releasing it again. Any failure message names the likely owner.
fn assert_socket_is_free() {
    match UnixListener::bind_addr(&socket_addr()) {
        // Dropping the listener releases the abstract name again, so the
        // primary child can bind it first.
        Ok(listener) => drop(listener),
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => panic!(
            "the single-instance socket name is already owned — is another \
             Rhesis running? stop it before running this tier"
        ),
        Err(e) => panic!("failed to probe the single-instance socket: {e:?}"),
    }
}

/// Launches the shipped binary with the child-side environment choices the
/// tier wants the same way production gets them (offscreen, info logs).
fn launch(role: &str, args: &[&str]) -> Child {
    Command::new(env!("CARGO_BIN_EXE_rhesis"))
        .args(args)
        .env("QT_QPA_PLATFORM", "offscreen")
        // The stand-down and boot decisions must be visible in the captured
        // stream the guard drains.
        .env("RUST_LOG", "info")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| panic!("failed to launch the {role} instance: {e:?}"))
}

/// Waits until the primary's guard accepts connections; the guard socket
/// lives before any UI (production binds it during `
/// acquire`), so this never races the UI setup.
fn wait_for_listener(timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        match UnixStream::connect_addr(&socket_addr()) {
            Ok(_) => return,
            Err(_) if Instant::now() < deadline => {}
            Err(e) => panic!("the primary never offered the single-instance socket ({e:?})"),
        }
        std::thread::sleep(POLL);
    }
}

/// Kills and reaps its child on drop (also stopping the log drainer), so a
/// failing assertion cannot leak the launched process.
struct ChildGuard {
    child: Child,
    log: SharedLog,
    drainer: Option<std::thread::JoinHandle<()>>,
}

impl ChildGuard {
    fn new(mut child: Child, role: &str) -> Self {
        let log = Arc::new(Mutex::new(String::new()));
        let drainer = match child.stderr.take() {
            Some(stderr) => {
                let sink = Arc::clone(&log);
                Some(std::thread::spawn(move || {
                    // Best-effort capture: a poisoned sink or a torn line
                    // must not escalate into a test failure of its own.
                    for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                        if let Ok(mut sink) = sink.lock() {
                            sink.push_str(&line);
                            sink.push('\n');
                        }
                    }
                }))
            }
            None => panic!("stderr must be piped for the {role}"),
        };
        Self {
            child,
            log,
            drainer,
        }
    }

    fn log(&self) -> String {
        self.log
            .lock()
            .expect("the log sink must be reachable")
            .clone()
    }

    /// Waits for the log drainer to reach EOF, so the captured log is
    /// complete; only valid once the child is known to be gone.
    fn join_drainer(&mut self) {
        if let Some(drainer) = self.drainer.take() {
            let _ = drainer.join();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(drainer) = self.drainer.take() {
            // The drainer reaches EOF once the child goes away; a bounded
            // join is unnecessary, the thread cannot outlive the pipe.
            let _ = drainer.join();
        }
    }
}

/// Polls the child's exit with a deadline; the standard library's `Child`
/// has no bounded wait, so this is the helper the contract needs.
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
        std::thread::sleep(POLL);
    }
}

/// Polls the shared log until it contains `needle`, failing with the
/// captured log when the deadline expires — a child may still be mid-boot
/// when the previous observable step returns.
fn wait_for_log(guard: &ChildGuard, needle: &str, timeout: Duration, reason: &str) {
    let deadline = Instant::now() + timeout;
    loop {
        let log = guard.log();
        if log.contains(needle) {
            return;
        }
        if Instant::now() >= deadline {
            panic!("{reason} (deadline exceeded); captured: {log:?}");
        }
        std::thread::sleep(POLL);
    }
}
