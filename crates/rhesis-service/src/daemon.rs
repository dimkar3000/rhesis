//! The daemon process: listener, single-client slot, idle grace, exits.
//!
//! Lifecycle (Refactor.md §2/§3): accept connections on a nonblocking
//! listener so between attempts the daemon can watch the session-end signal
//! and the idle-grace countdown; exactly one session may run at a time
//! (second connects get a busy `Hello` and a drop); when the last client
//! disconnects the daemon waits out the grace window (reconnects cancel it),
//! unless `background` says to stay alive; on exit — grace, `Shutdown`, or
//! fatal error — the LanguageTool worker is killed and joined
//! unconditionally so the embedded JVM never outlives the daemon.
//!
//! The worker is spawned in-process; its message receiver is loaned to the
//! active session and returned on teardown (see the module docs of
//! [`crate::session`]).

use std::{
    io::ErrorKind,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use crossbeam::channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use interprocess::local_socket::{
    traits::Listener as _, GenericNamespaced, ListenerNonblockingMode, ListenerOptions, Stream,
    ToNsName as _,
};

use rhesis_languagetool::service::{LanguageToolWorker, LanguageToolWorkerEvent, Message};
use rhesis_protocol::{
    write_frame, DaemonMsg, Settings, WorkerStatus, PROTOCOL, SERVICE_SOCKET_NAME,
};
use rhesis_settings::store::Store;

use crate::session;

/// How often the accept loop wakes between connection attempts to service
/// end signals and the grace countdown. Slice only — a connect is answered
/// within this window; the session itself is fully synchronous.
const ACCEPT_SLICE: Duration = Duration::from_millis(50);

/// Daemon configuration, resolved once per process. `production()` is what
/// the binary runs; tiers build their own with pinned paths and a short
/// grace (hermeticity: every path must stay inside the sandbox — the
/// defaults resolve through the real user configuration otherwise).
#[derive(Debug, Clone)]
pub struct DaemonOptions {
    /// Resolved local-socket name (Linux abstract namespace address).
    pub socket_name: String,
    /// Explicit store path; `None` resolves through
    /// [`Store::default_path`], and no configuration root at all means a
    /// defaults-only transient store.
    pub store_path: Option<PathBuf>,
    /// Legacy QSettings INI the one-time migration may consult. Production
    /// resolves via `rhesis_platform::paths::qsettings_ini_path`; a test
    /// pins an explicit value (or `None`) and never touches the file.
    pub ini_path: Option<PathBuf>,
    /// Idle window after the last client leaves.
    pub idle_grace: Duration,
}

impl DaemonOptions {
    pub fn production() -> Self {
        DaemonOptions {
            socket_name: SERVICE_SOCKET_NAME.to_string(),
            store_path: None,
            ini_path: rhesis_platform::paths::qsettings_ini_path(),
            idle_grace: Duration::from_secs(5),
        }
    }
}

/// How a session ended, reported to the accept loop.
#[derive(Debug)]
pub enum SessionEnd {
    /// The client left on its own: the idle-grace countdown starts.
    Done,
    /// The client asked the daemon to quit: exit as soon as it reports.
    Shutdown,
}

/// State shared between the accept loop, the session pump and the relay:
/// the owned settings, what the worker last received, the newest status,
/// and the message receiver that ping-pongs between sessions. Fields are
/// crate-visible (owned by the two cooperating modules; the wire-protocol
/// line stops here).
pub struct DaemonState {
    /// The store is the daemon's single settings file; accessed under the
    /// mutex together with the in-memory snapshot so a save and the mirror
    /// never interleave.
    pub(crate) store: Mutex<Store>,
    pub(crate) persist: bool,
    /// In-memory settings snapshot (equals the last mirrored ownership).
    pub(crate) settings: Mutex<Settings>,
    /// What the worker last received: `None` until the first handshake
    /// replays, then the intent after every worker-moving message.
    pub(crate) applied: Mutex<Option<Settings>>,
    /// Newest worker status, fed by the relay, replayed at handshake.
    pub(crate) last_status: Mutex<WorkerStatus>,
    /// Live `background` flag ("stay alive with no clients"), updated as
    /// snapshots land.
    pub(crate) background: AtomicBool,
    /// The worker's event channel (shared with the pump).
    pub(crate) event_sender: Sender<LanguageToolWorkerEvent>,
    /// The worker's message receiver, parked here between sessions.
    pub(crate) receiver_slot: Mutex<Option<Receiver<Message>>>,
}

/// Run the daemon to completion: bind, serve, and shut the worker down.
/// Returns `Err` only for fatal setup errors (bind conflict, worker spawn
/// failure) or a dead listener; a graceful exit (idle grace, `Shutdown`)
/// returns `Ok(())`.
pub fn run(options: &DaemonOptions) -> Result<(), String> {
    // 1. Store: the daemon owns the settings file; hermetic callers pin it.
    //    Without a configuration root the daemon degrades to defaults-only
    //    (warns once, never persists, never invents a file location).
    let (store, persist) = match &options.store_path {
        Some(path) => (
            Store::open(path.clone()).with_ini_path(options.ini_path.clone()),
            true,
        ),
        None => match Store::default_path() {
            Some(path) => (
                Store::open(path).with_ini_path(options.ini_path.clone()),
                true,
            ),
            None => {
                log::warn!(
                    "no configuration root; running with default settings, nothing persists"
                );
                (Store::transient(), false)
            }
        },
    };
    let stored = store.load();

    // 2. Worker: LanguageTool manager thread, spawned with its channels.
    //    A fake provider (test harness via RHESIS_FAKE_LT) resolves inside
    //    the worker thread, so seeding the variable must happen before
    //    this call.
    let Some(worker) = LanguageToolWorker::default().start() else {
        return Err("LanguageTool worker failed to spawn".to_string());
    };

    let state = Arc::new(DaemonState {
        persist,
        store: Mutex::new(store),
        settings: Mutex::new(stored.clone()),
        applied: Mutex::new(None),
        last_status: Mutex::new(WorkerStatus::default()),
        background: AtomicBool::new(stored.background),
        event_sender: worker.event_sender.clone(),
        receiver_slot: Mutex::new(Some(worker.message_receiver)),
    });

    // 3. Listener: nonblocking accept so the loop between attempts can
    //    serve session-end signals and the grace countdown.
    let name = options
        .socket_name
        .clone()
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| format!("invalid service socket name {:?}: {e}", options.socket_name))?;
    let listener = ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .create_sync()
        .map_err(|e| {
            format!(
                "cannot bind the service socket {:?}: {e}",
                options.socket_name
            )
        })?;
    log::info!("daemon listening on {:?}", options.socket_name);

    let (end_sender, end_receiver) = unbounded::<SessionEnd>();
    let slot: Arc<Mutex<bool>> = Arc::new(Mutex::new(false));

    let mut grace_since: Option<Instant> = None;
    let mut result = Ok(());
    loop {
        match listener.accept() {
            Ok(stream) => {
                let occupied = *slot.lock().unwrap();
                if occupied {
                    second_client_refused(stream);
                    continue;
                }
                // Take the slot for the session; the pump releases it.
                *slot.lock().unwrap() = true;
                grace_since = None;

                let session_slot = slot.clone();
                let session_end = end_sender.clone();
                let session_state = state.clone();
                match std::thread::Builder::new()
                    .name("rhesis-session".to_string())
                    .spawn(move || {
                        session::run_session(stream, session_state, session_slot, session_end)
                    }) {
                    Ok(_handle) => {}
                    Err(e) => {
                        log::error!("cannot spawn the session thread: {e:?}");
                        // Slot back to free: the never-started session
                        // cannot return it. Dropping the stream closes the
                        // connection, which is the client's answer.
                        *slot.lock().unwrap() = false;
                    }
                }
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                // Nothing waiting: pump the exit signals, then revisit.
                match end_receiver.recv_timeout(ACCEPT_SLICE) {
                    Ok(SessionEnd::Shutdown) => {
                        log::info!("client requested the daemon shutdown");
                        break;
                    }
                    Ok(SessionEnd::Done) => {
                        grace_since = Some(Instant::now());
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        result = Err("session bookkeeping channel collapsed".to_string());
                        break;
                    }
                }
                if grace_since.is_some_and(|since| since.elapsed() >= options.idle_grace) {
                    if state.background.load(Ordering::SeqCst) {
                        log::info!("no clients in the grace window; background mode keeps the daemon alive");
                        grace_since = None;
                    } else {
                        log::info!("no clients for {:?}; shutting down", options.idle_grace);
                        break;
                    }
                }
            }
            Err(e) => {
                result = Err(format!("listener failed: {e}"));
                break;
            }
        }
    }

    // 4. Worker reaping: unconditional on every exit path. The kill is
    //    idempotent for an already-stopped worker; joining guarantees the
    //    embedded JVM child is reaped before the process exits.
    log::info!("stopping the LanguageTool worker");
    if let Err(e) = state.event_sender.send(LanguageToolWorkerEvent::Kill) {
        log::warn!("worker event channel already gone: {e:?}");
    }
    if let Err(e) = worker.thread.join() {
        result = result.and_then(|()| Err(format!("worker thread panicked: {e:?}")));
    }
    result
}

/// Busy handshake for a second client: answer with the version and the
/// busy flag, then drop the stream (§3: one session; the client surfaces
/// "service busy" instead of hanging).
///
/// Delivery of the busy frame is best-effort: dropping the stream can
/// reset a connection whose own bytes are still arriving, so a client may
/// observe the refusal as either the frame or a plain close — both mean
/// "no session", which is what the reconnect logic needs.
fn second_client_refused(stream: Stream) {
    log::info!("second client while a session is open; refusing");
    let mut stream = stream;
    let _ = write_frame(
        &mut stream,
        &DaemonMsg::Hello {
            protocol: PROTOCOL,
            busy: true,
        },
    );
    // `stream` dropped here closes the connection.
}
