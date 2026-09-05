use std::{
    io::{BufRead, BufReader, Read},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

use crate::{interop::recommendation::Recommendation, languagetool::client::LanguageToolClient};
use crossbeam::channel::{after, unbounded, Receiver, Sender};
use cxx_qt_lib::QString;

/// How many consecutive start attempts may fail before the worker gives up
/// and latches into `Failed`. Reset by `Stop`, `SetPort` and `Retry`.
pub const MAX_START_FAILURES: u32 = 3;

/// Lifecycle of the embedded LanguageTool server, owned by the worker thread.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum WorkerStatus {
    #[default]
    Stopped,
    Starting,
    Started,
    Failed(String),
}

impl WorkerStatus {
    /// Numeric code exposed to QML (`server_status` property).
    /// Mapping: 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed
    /// (see `server_status_reason` for the failure details).
    pub fn code(&self) -> i32 {
        match self {
            WorkerStatus::Stopped => 0,
            WorkerStatus::Starting => 1,
            WorkerStatus::Started => 2,
            WorkerStatus::Failed(_) => 3,
        }
    }

    pub fn reason(&self) -> String {
        match self {
            WorkerStatus::Failed(r) => r.clone(),
            _ => String::new(),
        }
    }

    pub fn is_started(&self) -> bool {
        matches!(self, WorkerStatus::Started)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Suggestion(Suggestion),
    StatusChanged(WorkerStatus),
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Suggestion(pub Vec<Recommendation>);

pub enum LanguageToolWorkerEvent {
    /// The user typed new text, get new suggestions for it.
    /// Only honored while `Started`; dropped otherwise.
    UpdateSuggestions(QString),

    /// Get the new colors to use from the settings (any state).
    UpdateColors(Vec<(QString, QString)>),

    /// Send this event to stop the worker thread entirely.
    Kill,

    /// Change Server Address and verify a remote server (no local spawn).
    ///
    /// Not produced by the UI yet; reserved for switching to a remote
    /// LanguageTool server.
    #[allow(dead_code)]
    ChangeAddress(String, u16),

    /// Stop if required and restart the local server (legacy settings path).
    RestartLocalServer(u16),

    /// Stop the embedded server when it is disabled in the settings.
    StopLocalServer,

    /// Start the local server on the current port.
    /// `Stopped -> Starting -> Started`. Ignored in `Starting`/`Started`;
    /// ignored in `Failed` until `Stop`/`SetPort`/`Retry` resets the budget.
    Start,

    /// Stop the local server. Any state -> `Stopped`, failure budget cleared.
    Stop,

    /// Update the server port. Clears the failure budget. If the server was
    /// `Started`/`Starting` it is restarted on the new port, otherwise the
    /// port is just stored for the next `Start`.
    SetPort(u16),

    /// Clear the failure budget and try to start again (used by the top-bar
    /// retry button when `Failed`).
    Retry,
}

/// Handles to talk with a running [`LanguageToolWorker`] from other threads
pub struct WorkerHandles {
    pub event_sender: Sender<LanguageToolWorkerEvent>,
    pub message_receiver: Receiver<Message>,
    pub running: Arc<AtomicBool>,
    pub thread: JoinHandle<()>,
}

pub struct LanguageToolWorker {
    port: u16,
    host: String,

    use_local_server: bool,

    event_sender: Sender<LanguageToolWorkerEvent>,
    event_receiver: Receiver<LanguageToolWorkerEvent>,

    message_sender: Sender<Message>,
    message_receiver: Receiver<Message>,

    local_server_handle: Option<Child>,
    status: WorkerStatus,
    consecutive_failures: u32,
    pub startup_timeout: Duration,
}

/// Default Implementation of the LanguageToolWorker will just leave the service uninitialized. It's up to the user the set it up afterward and start it
impl Default for LanguageToolWorker {
    fn default() -> Self {
        let (event_sender, event_receiver) = unbounded::<LanguageToolWorkerEvent>();
        let (message_sender, message_receiver) = unbounded::<Message>();
        Self {
            event_sender,
            event_receiver,

            message_sender,
            message_receiver,

            // Default State: stopped, nothing running.
            use_local_server: true,
            host: "localhost".to_string(),
            status: WorkerStatus::Stopped,
            consecutive_failures: 0,
            startup_timeout: Duration::from_secs(60),
            port: 2689,
            local_server_handle: None,
        }
    }
}

impl LanguageToolWorker {
    /// Override the readiness timeout (tests use a few seconds).
    pub fn with_startup_timeout(mut self, timeout: Duration) -> Self {
        self.startup_timeout = timeout;
        self
    }

    pub fn status(&self) -> WorkerStatus {
        self.status.clone()
    }
}

enum Outcome {
    Continue,
    Stop,
}

/// Result of waiting for the freshly spawned server to become ready.
enum VerifyOutcome {
    Ready,
    Timeout(String),
    /// A lifecycle event arrived while verifying; the current attempt was
    /// abandoned (child killed) and the event must be dispatched fresh.
    Aborted(LanguageToolWorkerEvent),
    /// `Kill` arrived while verifying; the whole worker must exit.
    Killed,
}

impl LanguageToolWorker {
    /// Spawn the worker in a standalone thread and return handles to talk with it.
    pub fn start(self) -> WorkerHandles {
        let event_sender = self.event_sender.clone();
        let message_receiver = self.message_receiver.clone();

        let running = Arc::new(AtomicBool::new(true));

        let thread_running = running.clone();
        let thread = std::thread::Builder::new()
            .name("languagetool-worker".to_string())
            .spawn(move || self.run_loop(thread_running))
            .unwrap();

        WorkerHandles {
            event_sender,
            message_receiver,
            running,
            thread,
        }
    }

    /// Blocking event loop, runs on the worker's standalone thread and fully
    /// manages the lifetime of the embedded LanguageTool server.
    fn run_loop(mut self, running: Arc<AtomicBool>) {
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                log::error!("Failed to build LanguageTool worker runtime: {e:?}");
                running.store(false, Ordering::SeqCst);
                return;
            }
        };

        let mut client = LanguageToolClient::new(&self.host, self.port);
        let mut last_text = QString::default();

        // Boot state: stopped, worker thread alive, no server.
        self.emit_status(WorkerStatus::Stopped);

        loop {
            let event = match self.event_receiver.recv() {
                Ok(event) => event,
                Err(e) => {
                    log::warn!(
                        "LanguageTool worker channel closed, the worker can be restarted: {e:?}"
                    );
                    break;
                }
            };

            let outcome = match event {
                LanguageToolWorkerEvent::UpdateSuggestions(text) => {
                    self.handle_suggestions(text, &mut last_text, &mut client, &runtime)
                }
                other => self.process_event(other, &mut client, &runtime, &mut last_text),
            };

            match outcome {
                Outcome::Continue => (),
                Outcome::Stop => break,
            }
        }

        self.terminate_child();
        running.store(false, Ordering::SeqCst);
        log::info!("LanguageTool worker stopped");
    }

    /// Debounce new text for 300ms, then fetch suggestions for the latest text.
    /// Drops everything unless the server is `Started`.
    fn handle_suggestions(
        &mut self,
        mut text: QString,
        last_text: &mut QString,
        client: &mut LanguageToolClient,
        runtime: &tokio::runtime::Runtime,
    ) -> Outcome {
        if !self.status.is_started() {
            log::debug!(
                "dropping suggestions request, server status is {:?}",
                self.status
            );
            return Outcome::Continue;
        }

        if text.trimmed().is_empty() {
            return Outcome::Continue;
        }

        // Wait 300ms after the last text change before checking. New text
        // events reset the timer; other events are processed immediately.
        'debounce: loop {
            crossbeam::select! {
                recv(self.event_receiver) -> next => match next {
                    Ok(LanguageToolWorkerEvent::UpdateSuggestions(new_text)) => {
                        if !new_text.trimmed().is_empty() {
                            text = new_text;
                        }
                    }
                    Ok(other) => match self.process_event(other, client, runtime, last_text) {
                        Outcome::Continue => {
                            // A lifecycle event (e.g. Stop) may have taken us
                            // out of Started; drop the pending text then.
                            if !self.status.is_started() {
                                return Outcome::Continue;
                            }
                        }
                        Outcome::Stop => return Outcome::Stop,
                    },
                    Err(_) => break 'debounce,
                },
                recv(after(Duration::from_millis(300))) -> _ => break 'debounce,
            }
        }

        if !self.status.is_started() {
            return Outcome::Continue;
        }

        if text == *last_text || text.trimmed().is_empty() {
            return Outcome::Continue;
        }

        *last_text = text.clone();

        let suggestions = runtime.block_on(client.get_recommendation(text.to_string()));
        log::debug!(
            "got {} recommendations for {} characters of text",
            suggestions.len(),
            text.len()
        );
        match self
            .message_sender
            .send(Message::Suggestion(Suggestion(suggestions)))
        {
            Ok(_) => (),
            Err(e) => log::warn!("failed to send suggestions to the UI: {e:?}"),
        };

        Outcome::Continue
    }

    fn emit_status(&mut self, status: WorkerStatus) {
        self.status = status.clone();
        if let Err(e) = self.message_sender.send(Message::StatusChanged(status)) {
            log::warn!("failed to send status to the UI: {e:?}");
        }
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        log::info!("LanguageTool server is ready on port {}", self.port);
        self.emit_status(WorkerStatus::Started);
    }

    fn record_failure(&mut self, reason: String) {
        self.terminate_child();
        self.consecutive_failures += 1;
        if self.consecutive_failures >= MAX_START_FAILURES {
            log::error!(
                "LanguageTool failed to start {} times, giving up: {reason}",
                self.consecutive_failures
            );
            self.emit_status(WorkerStatus::Failed(reason));
        } else {
            log::warn!(
                "LanguageTool start attempt {}/{} failed: {reason}",
                self.consecutive_failures,
                MAX_START_FAILURES
            );
            self.emit_status(WorkerStatus::Stopped);
        }
    }

    fn terminate_child(&mut self) {
        if let Some(mut child) = self.local_server_handle.take() {
            log::info!("Killing LanguageTool");
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn process_event(
        &mut self,
        event: LanguageToolWorkerEvent,
        client: &mut LanguageToolClient,
        runtime: &tokio::runtime::Runtime,
        last_text: &mut QString,
    ) -> Outcome {
        match event {
            LanguageToolWorkerEvent::UpdateSuggestions(_) => Outcome::Continue,
            LanguageToolWorkerEvent::UpdateColors(colors) => {
                log::info!("updating {} color rules", colors.len());
                client.set_colors(colors);
                Outcome::Continue
            }
            LanguageToolWorkerEvent::Kill => {
                log::info!("Killing LanguageTool worker");
                self.terminate_child();
                Outcome::Stop
            }
            LanguageToolWorkerEvent::ChangeAddress(host, port) => {
                log::info!("Changing address to {host}:{port}");
                self.host = host;
                self.port = port;
                client.update_address(&self.host, self.port);
                self.use_local_server = false;
                self.terminate_child();
                self.verify_remote(client, runtime);
                // Remote check invalidates cached text so the next identical
                // input is re-checked against the new server.
                *last_text = QString::default();
                Outcome::Continue
            }
            LanguageToolWorkerEvent::RestartLocalServer(port) => {
                log::info!("Restarting local server on port {port}");
                self.port = port;
                self.host = "localhost".to_string();
                client.update_address(&self.host, self.port);
                self.use_local_server = true;
                // Explicit restart gets a fresh failure budget.
                self.consecutive_failures = 0;
                self.start_local_server(client, runtime);
                *last_text = QString::default();
                Outcome::Continue
            }
            LanguageToolWorkerEvent::StopLocalServer => {
                log::info!("stopping the local LanguageTool server");
                self.use_local_server = false;
                self.terminate_child();
                self.consecutive_failures = 0;
                self.emit_status(WorkerStatus::Stopped);
                Outcome::Continue
            }
            LanguageToolWorkerEvent::Start => {
                match self.status.clone() {
                    WorkerStatus::Started | WorkerStatus::Starting => {
                        log::debug!("Start ignored, already {:?}", self.status);
                        Outcome::Continue
                    }
                    WorkerStatus::Failed(reason) => {
                        log::warn!(
                            "Start ignored while Failed({reason}); use Retry/Stop/SetPort"
                        );
                        Outcome::Continue
                    }
                    WorkerStatus::Stopped => {
                        self.use_local_server = true;
                        self.start_local_server(client, runtime);
                        *last_text = QString::default();
                        Outcome::Continue
                    }
                }
            }
            LanguageToolWorkerEvent::Stop => {
                log::info!("stopping the local LanguageTool server");
                self.use_local_server = false;
                self.terminate_child();
                self.consecutive_failures = 0;
                self.emit_status(WorkerStatus::Stopped);
                Outcome::Continue
            }
            LanguageToolWorkerEvent::SetPort(port) => {
                log::info!("Setting LanguageTool port to {port}");
                let was_running =
                    matches!(self.status, WorkerStatus::Started | WorkerStatus::Starting);
                let was_failed = matches!(self.status, WorkerStatus::Failed(_));
                self.port = port;
                if self.use_local_server {
                    self.host = "localhost".to_string();
                }
                client.update_address(&self.host, self.port);
                // New port means a fresh budget.
                self.consecutive_failures = 0;
                if was_failed {
                    // Leave Failed behind; wait for an explicit Start/Retry
                    // unless we were previously running (then restart now).
                    self.emit_status(WorkerStatus::Stopped);
                }
                if was_running {
                    self.start_local_server(client, runtime);
                    *last_text = QString::default();
                } else if was_failed {
                    // Stay stopped; UI shows stopped and can Start/Retry.
                } else {
                    // Stay stopped but re-emit so the UI picks up the port
                    // change even when nothing is running.
                    self.emit_status(WorkerStatus::Stopped);
                }
                Outcome::Continue
            }
            LanguageToolWorkerEvent::Retry => {
                match self.status.clone() {
                    WorkerStatus::Started | WorkerStatus::Starting => {
                        log::debug!("Retry ignored, already {:?}", self.status);
                        Outcome::Continue
                    }
                    WorkerStatus::Stopped | WorkerStatus::Failed(_) => {
                        log::info!("Retrying LanguageTool server on port {}", self.port);
                        self.consecutive_failures = 0;
                        self.use_local_server = true;
                        self.host = "localhost".to_string();
                        client.update_address(&self.host, self.port);
                        self.start_local_server(client, runtime);
                        *last_text = QString::default();
                        Outcome::Continue
                    }
                }
            }
        }
    }

    /// Verify a remote server without spawning anything.
    fn verify_remote(
        &mut self,
        client: &mut LanguageToolClient,
        runtime: &tokio::runtime::Runtime,
    ) {
        self.emit_status(WorkerStatus::Starting);
        let deadline = Instant::now() + self.startup_timeout;
        while Instant::now() < deadline {
            // Abort promptly on lifecycle events, but don't lose colors/text.
            match self.event_receiver.recv_timeout(Duration::from_millis(300)) {
                Ok(LanguageToolWorkerEvent::UpdateSuggestions(_)) => continue,
                Ok(LanguageToolWorkerEvent::UpdateColors(colors)) => {
                    client.set_colors(colors);
                    continue;
                }
                Ok(LanguageToolWorkerEvent::Kill) => {
                    self.terminate_child();
                    let _ = self.event_sender.send(LanguageToolWorkerEvent::Kill);
                    self.emit_status(WorkerStatus::Stopped);
                    return;
                }
                Ok(LanguageToolWorkerEvent::Stop)
                | Ok(LanguageToolWorkerEvent::StopLocalServer) => {
                    self.use_local_server = false;
                    self.terminate_child();
                    self.consecutive_failures = 0;
                    self.emit_status(WorkerStatus::Stopped);
                    return;
                }
                Ok(other) => {
                    // Defer any other lifecycle event back to the main loop.
                    let _ = self.event_sender.send(other);
                    self.emit_status(WorkerStatus::Stopped);
                    return;
                }
                Err(_) => {}
            }
            if runtime.block_on(client.health_check()) {
                self.record_success();
                return;
            }
        }
        self.record_failure(format!(
            "remote LanguageTool at {} did not answer within {:?}",
            client.address(),
            self.startup_timeout
        ));
    }

    fn start_local_server(
        &mut self,
        client: &mut LanguageToolClient,
        runtime: &tokio::runtime::Runtime,
    ) {
        // Outer entry: Starting, spawn, then verify (abortable).
        self.emit_status(WorkerStatus::Starting);
        // `start_local_server` may be re-entered via Aborted events; loop
        // until the verification settles instead of recursing.
        loop {
            self.terminate_child();
            let child = match Self::spawn_server(self.port) {
                Ok(c) => c,
                Err(reason) => {
                    self.record_failure(reason);
                    return;
                }
            };
            self.local_server_handle = Some(child);

            match self.wait_until_ready(client, runtime) {
                VerifyOutcome::Ready => {
                    self.record_success();
                    return;
                }
                VerifyOutcome::Timeout(reason) => {
                    self.record_failure(reason);
                    return;
                }
                VerifyOutcome::Killed => {
                    self.terminate_child();
                    // Tell the outer run_loop to exit.
                    let _ = self.event_sender.send(LanguageToolWorkerEvent::Kill);
                    // Consume it immediately by returning a marker: set a flag
                    // via status? Instead just return; the re-sent Kill will
                    // break the outer loop on its next recv.
                    return;
                }
                VerifyOutcome::Aborted(event) => {
                    self.terminate_child();
                    // Dispatch the interrupting event fresh. If it is another
                    // start-like event we loop around and try again with the
                    // new settings; otherwise handle once and return.
                    let needs_restart = matches!(
                        event,
                        LanguageToolWorkerEvent::RestartLocalServer(_)
                            | LanguageToolWorkerEvent::SetPort(_)
                            | LanguageToolWorkerEvent::Start
                            | LanguageToolWorkerEvent::Retry
                            | LanguageToolWorkerEvent::ChangeAddress(_, _)
                    );
                    // Use a dummy last_text; callers reset it on success.
                    let mut dummy = QString::default();
                    let outcome =
                        self.process_event(event, client, runtime, &mut dummy);
                    if matches!(outcome, Outcome::Stop) {
                        let _ = self.event_sender.send(LanguageToolWorkerEvent::Kill);
                        return;
                    }
                    if needs_restart
                        && matches!(self.status, WorkerStatus::Starting)
                    {
                        // process_event already started a nested attempt which
                        // settled; do not loop again.
                        return;
                    }
                    if needs_restart {
                        // e.g. SetPort while stopped already handled; but if
                        // the handler left us Starting without settling (should
                        // not happen), loop to verify the new child.
                        if matches!(self.status, WorkerStatus::Starting)
                            && self.local_server_handle.is_some()
                        {
                            continue;
                        }
                        return;
                    }
                    return;
                }
            }
        }
    }

    /// Poll `health_check` until ready/timeout, aborting on lifecycle events.
    fn wait_until_ready(
        &mut self,
        client: &mut LanguageToolClient,
        runtime: &tokio::runtime::Runtime,
    ) -> VerifyOutcome {
        let deadline = Instant::now() + self.startup_timeout;
        while Instant::now() < deadline {
            match self.event_receiver.recv_timeout(Duration::from_millis(300)) {
                Ok(LanguageToolWorkerEvent::UpdateSuggestions(_)) => {
                    // Dropped while starting; keep verifying.
                    continue;
                }
                Ok(LanguageToolWorkerEvent::UpdateColors(colors)) => {
                    client.set_colors(colors);
                    continue;
                }
                Ok(LanguageToolWorkerEvent::Kill) => return VerifyOutcome::Killed,
                Ok(other @ LanguageToolWorkerEvent::Stop)
                | Ok(other @ LanguageToolWorkerEvent::StopLocalServer)
                | Ok(other @ LanguageToolWorkerEvent::SetPort(_))
                | Ok(other @ LanguageToolWorkerEvent::RestartLocalServer(_))
                | Ok(other @ LanguageToolWorkerEvent::Start)
                | Ok(other @ LanguageToolWorkerEvent::Retry)
                | Ok(other @ LanguageToolWorkerEvent::ChangeAddress(_, _)) => {
                    return VerifyOutcome::Aborted(other);
                }
                Err(_) => {}
            }

            if runtime.block_on(client.health_check()) {
                return VerifyOutcome::Ready;
            }
        }
        VerifyOutcome::Timeout(format!(
            "LanguageTool server on port {} did not become ready within {:?}",
            self.port, self.startup_timeout
        ))
    }

    /// Locate a usable LanguageTool installation: the first candidate directory
    /// that actually contains `languagetool-server.jar`. Packaged locations
    /// come first so system installs always win over development fallbacks.
    fn language_tool_dir() -> Option<PathBuf> {
        let mut candidates = vec![PathBuf::from("/usr/share/rhesis/LanguageTool")];

        // Per-user install: $HOME/.local/share/rhesis/LanguageTool
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(".local/share/rhesis/LanguageTool"));
        }

        // Flatpak: standard app layout
        candidates.push(PathBuf::from("/app/share/rhesis/LanguageTool"));

        // AppImage: path relative to $APPDIR
        if let Ok(appdir) = std::env::var("APPDIR") {
            candidates.push(PathBuf::from(format!("{appdir}/app/share/rhesis/LanguageTool")));
        }

        // Local development: setup.sh output, then build-common.sh artifacts
        candidates.push(PathBuf::from("./build/LanguageTool"));
        candidates.push(PathBuf::from("./build/artifacts/LanguageTool"));
        candidates.push(PathBuf::from("./LanguageTool"));

        for candidate in &candidates {
            if candidate.join("languagetool-server.jar").is_file() {
                return Some(candidate.clone());
            }
        }
        log::warn!("no LanguageTool installation found (searched: {candidates:?})");
        None
    }

    fn spawn_server(port: u16) -> Result<Child, String> {
        let lt_dir = match Self::language_tool_dir() {
            Some(dir) => dir,
            None => {
                return Err("LanguageTool is not installed (languagetool-server.jar not found); run scripts/setup.sh to download it".to_string());
            }
        };
        log::info!("LanguageTool dir: {:?}", lt_dir);

        // Fail fast with an actionable reason instead of burning the full
        // readiness timeout when something else already owns the port
        // (stale server, second app instance, ...).
        if let Err(e) = std::net::TcpListener::bind(("127.0.0.1", port)) {
            return Err(format!(
                "port {port} is already in use ({e}); stop the process holding it first"
            ));
        }

        Self::ensure_server_properties(&lt_dir);

        let result = Command::new("java")
            .args([
                "-cp",
                "languagetool-server.jar",
                "org.languagetool.server.HTTPServer",
                "--config",
                "server.properties",
                "--port",
                &port.to_string(),
                "--allow-origin",
            ])
            .current_dir(&lt_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match result {
            Ok(c) => c,
            Err(e) => {
                return Err(format!("failed to spawn LanguageTool: {e:?}"));
            }
        };

        // Fail fast when java exits during startup (bad config, missing jar,
        // lost port race, ...), capturing its stderr so the failure reason
        // is actionable instead of a bare exit status. Piped output stays
        // unread only briefly; JVM startup chatter never fills the pipe.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let detail = Self::read_child_stderr(&mut child);
                    return Err(format!("LanguageTool exited early ({status}): {detail}"));
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(200));
                }
                Err(e) => return Err(format!("failed to poll LanguageTool process: {e:?}")),
            }
        }

        if let Some(out) = child.stdout.take() {
            std::thread::spawn(move || {
                let reader = BufReader::new(out);
                for line in reader.lines() {
                    match line {
                        Ok(line) => log::trace!("[LanguageTool]: {line}"),
                        Err(e) => log::error!("Error from stdout reader: {e:?}"),
                    }
                }
            });
        }
        if let Some(error) = child.stderr.take() {
            std::thread::spawn(move || {
                let reader = BufReader::new(error);
                for line in reader.lines() {
                    match line {
                        Ok(line) => log::trace!("[LanguageTool]: {line}"),
                        Err(e) => log::error!("Error from stderr reader: {e:?}"),
                    }
                }
            });
        }

        log::info!("LanguageTool process spawned on port {port}");
        Ok(child)
    }

    /// Drain a dead child's stderr (bounded) for failure diagnostics.
    fn read_child_stderr(child: &mut Child) -> String {
        let mut output = String::new();
        if let Some(stderr) = child.stderr.take() {
            let mut reader = BufReader::new(stderr);
            let _ = reader.read_to_string(&mut output);
        }
        let lines: Vec<&str> = output
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .take(5)
            .collect();
        if lines.is_empty() {
            "no output on stderr".to_string()
        } else {
            lines.join(" | ")
        }
    }

    /// (fasttext model, fasttext binary) locations relative to the
    /// LanguageTool dir, covering every layout that does not already ship a
    /// server.properties: dev artifacts (setup.sh), desktop payloads, Flatpak.
    const FASTTEXT_LAYOUTS: &'static [(&'static str, &'static str)] = &[
        ("../lid.176.ftz", "../fastText/fasttext"),
        ("../lid.176.ftz", "../bin/fasttext"),
        ("../lid.176.ftz", "../../bin/fasttext"),
    ];

    /// Recreate a missing server.properties from the surrounding layout so a
    /// lost config fails open instead of killing startup with a bare exit
    /// status. Never overwrites an existing file.
    fn ensure_server_properties(lt_dir: &Path) {
        let props = lt_dir.join("server.properties");
        if props.is_file() {
            return;
        }
        for (model, binary) in Self::FASTTEXT_LAYOUTS {
            if lt_dir.join(model).is_file() && lt_dir.join(binary).is_file() {
                let contents = format!("fasttextModel={model}\nfasttextBinary={binary}\n");
                match std::fs::write(&props, &contents) {
                    Ok(()) => log::warn!(
                        "server.properties was missing; created it for this layout:\n{contents}"
                    ),
                    Err(e) => log::warn!(
                        "server.properties is missing and could not be created at {props:?}: {e:?}"
                    ),
                }
                return;
            }
        }
        log::warn!(
            "server.properties is missing at {props:?} and no fasttext layout was recognized; \
             starting without language-detection config"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::LanguageToolWorker;
    use std::path::PathBuf;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rhesis-lt-test-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &PathBuf) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, b"").unwrap();
    }

    #[test]
    fn creates_properties_for_dev_layout() {
        let base = scratch_dir("dev");
        let lt_dir = base.join("LanguageTool");
        std::fs::create_dir_all(&lt_dir).unwrap();
        touch(&base.join("lid.176.ftz"));
        touch(&base.join("fastText/fasttext"));

        LanguageToolWorker::ensure_server_properties(&lt_dir);

        let props = std::fs::read_to_string(lt_dir.join("server.properties")).unwrap();
        assert_eq!(
            props,
            "fasttextModel=../lid.176.ftz\nfasttextBinary=../fastText/fasttext\n"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn creates_properties_for_desktop_layout() {
        let base = scratch_dir("desktop");
        let lt_dir = base.join("LanguageTool");
        std::fs::create_dir_all(&lt_dir).unwrap();
        touch(&base.join("lid.176.ftz"));
        touch(&base.join("bin/fasttext"));

        LanguageToolWorker::ensure_server_properties(&lt_dir);

        let props = std::fs::read_to_string(lt_dir.join("server.properties")).unwrap();
        assert_eq!(
            props,
            "fasttextModel=../lid.176.ftz\nfasttextBinary=../bin/fasttext\n"
        );
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn never_overwrites_existing_properties() {
        let base = scratch_dir("existing");
        let lt_dir = base.join("LanguageTool");
        std::fs::create_dir_all(&lt_dir).unwrap();
        touch(&base.join("lid.176.ftz"));
        touch(&base.join("fastText/fasttext"));
        std::fs::write(lt_dir.join("server.properties"), "custom=true\n").unwrap();

        LanguageToolWorker::ensure_server_properties(&lt_dir);

        let props = std::fs::read_to_string(lt_dir.join("server.properties")).unwrap();
        assert_eq!(props, "custom=true\n");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn unrecognized_layout_creates_nothing() {
        let base = scratch_dir("bare");
        let lt_dir = base.join("LanguageTool");
        std::fs::create_dir_all(&lt_dir).unwrap();

        LanguageToolWorker::ensure_server_properties(&lt_dir);

        assert!(!lt_dir.join("server.properties").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
