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

use rhesis_constants::{PROTOCOL, SERVICE_SOCKET_NAME};
use rhesis_languagetool::service::{
    CompletionProvider, LanguageToolWorker, LanguageToolWorkerEvent, ServiceMessage, WorkerStatus,
};
use rhesis_protocol::daemon_msg::DaemonMsg;
use rhesis_protocol::write_frame;
use rhesis_settings::settings::Settings;
use rhesis_settings::store::Store;

use crate::session;

/// How often the accept loop wakes between connection attempts to service
/// end signals and the grace countdown. Slice only — a connect is answered
/// within this window; the session itself is fully synchronous.
const ACCEPT_SLICE: Duration = Duration::from_millis(50);

#[derive(Debug, Clone)]
pub struct DaemonOptions {
    pub socket_name: String,
    pub store_path: PathBuf,
    pub idle_grace: Duration,
}

impl DaemonOptions {
    pub fn production() -> Self {
        DaemonOptions {
            socket_name: SERVICE_SOCKET_NAME.to_string(),
            store_path: Store::default_path().unwrap_or_default(),
            idle_grace: Duration::from_secs(5),
        }
    }
}

#[derive(Debug)]
pub enum SessionEnd {
    Done,
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
    pub(crate) receiver_slot: Mutex<Option<Receiver<ServiceMessage>>>,
}

pub fn deamon_run<P: CompletionProvider + 'static>(
    options: &DaemonOptions,
    provider: P,
) -> Result<(), String> {
    let (store, persist) = if options.store_path.as_os_str().is_empty() {
        log::warn!("no configuration root; running with default settings, nothing persists");
        (Store::transient(), false)
    } else {
        (Store::open(options.store_path.clone()), true)
    };
    let stored = store.load();

    let Some(worker) = LanguageToolWorker::default().start(provider) else {
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
    let slot = Arc::new(AtomicBool::new(false));

    let mut grace_since: Option<Instant> = None;
    let mut result = Ok(());
    loop {
        match listener.accept() {
            Ok(stream) => {
                if slot
                    .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                    .is_err()
                {
                    refuse_second_client(stream);
                    continue;
                }
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
                        slot.store(false, Ordering::SeqCst);
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

    log::info!("stopping the LanguageTool worker");
    if let Err(e) = state.event_sender.send(LanguageToolWorkerEvent::Kill) {
        log::warn!("worker event channel already gone: {e:?}");
    }
    if let Err(e) = worker.thread.join() {
        result = result.and_then(|()| Err(format!("worker thread panicked: {e:?}")));
    }

    result
}

fn refuse_second_client(stream: Stream) {
    log::info!("second client while a session is open; refusing");
    let mut stream = stream;
    let _ = write_frame(
        &mut stream,
        &DaemonMsg::Connect {
            protocol: PROTOCOL,
            busy: true,
        },
    );
}
