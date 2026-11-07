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

use crossbeam::channel::{after, unbounded, Receiver, Sender};
use rhesis_protocol::{constants::DEFAULT_PORT, WorkerStatus};

use crate::{
    completion_provider::CompletionProvider,
    constants::{FASTTEXT_LAYOUTS, MAX_START_FAILURES},
    service::{ServiceMessage, ServiceSuggestion},
};

#[derive(Debug)]
pub enum LanguageToolWorkerEvent {
    Start,
    Stop,
    Retry,
    Kill,
    StopLocalServer,

    UpdateSuggestions(String),
    UpdateColors(Vec<(String, String)>),

    SwitchToRemoteServer(String),
    RestartLocalServer(u16),
    SetPort(u16),
}

/// Handles to talk with a running [`LanguageToolWorker`] from other threads
pub struct WorkerHandles {
    pub event_sender: Sender<LanguageToolWorkerEvent>,
    pub message_receiver: Receiver<ServiceMessage>,
    pub running: Arc<AtomicBool>,
    pub thread: JoinHandle<()>,
}

pub struct LanguageToolWorker {
    local_port: u16,
    local_server_handle: Option<Child>,
    use_local_server: bool,

    remote_address: String,

    event_sender: Sender<LanguageToolWorkerEvent>,
    event_receiver: Receiver<LanguageToolWorkerEvent>,

    message_sender: Sender<ServiceMessage>,
    message_receiver: Receiver<ServiceMessage>,

    status: WorkerStatus,
    consecutive_failures: u32,
    pub startup_timeout: Duration,

    fake_fixture: Option<String>,
}

impl Default for LanguageToolWorker {
    fn default() -> Self {
        let (event_sender, event_receiver) = unbounded::<LanguageToolWorkerEvent>();
        let (message_sender, message_receiver) = unbounded::<ServiceMessage>();
        Self {
            event_sender,
            event_receiver,

            message_sender,
            message_receiver,

            // Default State: stopped, nothing running, local mode.
            use_local_server: true,
            remote_address: String::new(),
            // Default: real provider (the fake comes from the seam below).
            fake_fixture: None,
            status: WorkerStatus::Stopped,
            consecutive_failures: 0,
            startup_timeout: Duration::from_secs(60),
            local_port: DEFAULT_PORT,
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

    /// Point the worker at the recorded fixture provider instead of the
    /// real LanguageTool server: The Data are driven by the file `{{fixture_id}}.json`
    pub fn with_fake_fixture(mut self, fixture_id: Option<String>) -> Self {
        self.fake_fixture = fixture_id;
        self
    }

    pub fn status(&self) -> WorkerStatus {
        self.status.clone()
    }

    /// Base URL of whatever server the client should currently talk to.
    fn active_address(&self) -> String {
        if self.use_local_server {
            format!("http://localhost:{}", self.local_port)
        } else {
            self.remote_address.clone()
        }
    }
}

enum Outcome {
    Continue,
    Stop,
}

enum VerifyOutcome {
    Ready,
    Timeout(String),
    Aborted(LanguageToolWorkerEvent),
    Killed,
}

impl LanguageToolWorker {
    pub fn start(self) -> Option<WorkerHandles> {
        let event_sender = self.event_sender.clone();
        let message_receiver = self.message_receiver.clone();

        let running = Arc::new(AtomicBool::new(true));

        let thread_running = running.clone();
        let thread = match std::thread::Builder::new()
            .name("languagetool-worker".to_string())
            .spawn(move || self.run_loop(thread_running))
        {
            Ok(thread) => thread,
            Err(e) => {
                log::error!("failed to spawn LanguageTool worker thread: {e:?}");
                return None;
            }
        };

        Some(WorkerHandles {
            event_sender,
            message_receiver,
            running,
            thread,
        })
    }

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

        // The fake-vs-real decision the worker was built with (the
        // daemon's `--fake-lt` argument): a fixture id swaps the live
        // server for the recorded fixture provider; a broken fixture
        // degrades to the live client so the daemon stays usable.
        let mut provider = match self.fake_fixture.as_deref() {
            Some(id) => match CompletionProvider::new_fake(id) {
                Ok(provider) => {
                    log::info!("using the recorded fake provider fixture {id:?}");
                    provider
                }
                Err(e) => {
                    log::error!(
                        "fake provider fixture {id:?} failed to load: {e}; \
                         falling back to the live client"
                    );
                    CompletionProvider::new_live(&self.active_address())
                }
            },
            None => CompletionProvider::new_live(&self.active_address()),
        };

        let mut last_text = String::default();

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
                    self.handle_suggestions(text, &mut last_text, &mut provider, &runtime)
                }
                other => self.process_event(other, &mut provider, &runtime, &mut last_text),
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

    fn handle_suggestions(
        &mut self,
        text: String,
        last_text: &mut String,
        provider: &mut CompletionProvider,
        runtime: &tokio::runtime::Runtime,
    ) -> Outcome {
        if !self.status.is_started() {
            log::debug!(
                "dropping suggestions request, server status is {:?}",
                self.status
            );
            return Outcome::Continue;
        }

        if text.trim().is_empty() {
            return Outcome::Continue;
        }

        // Wait 300ms after the last text change before checking. New text
        // events reset the timer; other events are processed immediately.
        let mut text = text;
        'debounce: loop {
            crossbeam::select! {
                recv(self.event_receiver) -> next => match next {
                    Ok(LanguageToolWorkerEvent::UpdateSuggestions(new_text)) => {
                        if !new_text.trim().is_empty() {
                            text = new_text;
                        }
                    }
                    Ok(other) => match self.process_event(other, provider, runtime, last_text) {
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

        if text == *last_text || text.trim().is_empty() {
            return Outcome::Continue;
        }

        *last_text = text.clone();

        match runtime.block_on(provider.check(&text)) {
            Ok(suggestions) => {
                self.note_request_success();
                log::debug!(
                    "got {} recommendations for {} characters of text",
                    suggestions.len(),
                    text.len()
                );
                match self
                    .message_sender
                    .send(ServiceMessage::Suggestion(ServiceSuggestion(suggestions)))
                {
                    Ok(_) => (),
                    Err(e) => log::warn!("failed to send suggestions to the UI: {e:?}"),
                };
            }
            Err(reason) => self.note_request_failure(reason),
        };

        Outcome::Continue
    }

    fn note_request_success(&mut self) {
        if matches!(self.status, WorkerStatus::FailedToRespond(_)) {
            log::info!(
                "LanguageTool is responding again at {}",
                self.active_address()
            );
            self.emit_status(WorkerStatus::Started);
        }
    }

    fn note_request_failure(&mut self, reason: String) {
        match &self.status {
            WorkerStatus::Started => {
                log::warn!("LanguageTool is not responding: {reason}");
                self.emit_status(WorkerStatus::FailedToRespond(reason));
            }
            WorkerStatus::FailedToRespond(current) if *current != reason => {
                // Same outage, different detail; refresh the tooltip only.
                log::debug!("LanguageTool is still not responding: {reason}");
                self.emit_status(WorkerStatus::FailedToRespond(reason));
            }
            _ => (), // already reported, or a dropped request that never mattered
        }
    }

    fn emit_status(&mut self, status: WorkerStatus) {
        log::info!("LanguageTool status: {status:?}");
        self.status = status.clone();
        if let Err(e) = self
            .message_sender
            .send(ServiceMessage::StatusChanged(status))
        {
            log::warn!("failed to send status to the UI: {e:?}");
        }
    }

    fn record_success(&mut self) {
        self.consecutive_failures = 0;
        log::info!("LanguageTool server is ready at {}", self.active_address());
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
        provider: &mut CompletionProvider,
        runtime: &tokio::runtime::Runtime,
        last_text: &mut String,
    ) -> Outcome {
        match event {
            LanguageToolWorkerEvent::UpdateSuggestions(_) => Outcome::Continue,
            LanguageToolWorkerEvent::UpdateColors(colors) => {
                log::info!("updating {} color rules", colors.len());
                provider.set_colors(colors);
                Outcome::Continue
            }
            LanguageToolWorkerEvent::Kill => {
                log::info!("Killing LanguageTool worker");
                self.terminate_child();
                Outcome::Stop
            }
            LanguageToolWorkerEvent::SwitchToRemoteServer(address) => {
                let address = crate::helpers::normalize_address(&address);
                log::info!("Switching to external LanguageTool server at {address}");
                self.remote_address = address.clone();
                provider.update_address(&address);
                self.use_local_server = false;
                self.terminate_child();
                self.consecutive_failures = 0;
                self.verify_remote(provider, runtime);
                // Remote check invalidates cached text so the next identical
                // input is re-checked against the new server.
                *last_text = String::default();
                Outcome::Continue
            }
            LanguageToolWorkerEvent::RestartLocalServer(port) => {
                log::info!("Restarting local server on port {port}");
                self.local_port = port;
                self.use_local_server = true;
                provider.update_address(&self.active_address());
                // Explicit restart gets a fresh failure budget.
                self.consecutive_failures = 0;
                self.start_local_server(provider, runtime);
                *last_text = String::default();
                Outcome::Continue
            }
            LanguageToolWorkerEvent::StopLocalServer => {
                log::info!("stopping the local LanguageTool server");
                self.terminate_child();
                self.consecutive_failures = 0;
                self.emit_status(WorkerStatus::Stopped);
                Outcome::Continue
            }
            LanguageToolWorkerEvent::Start => match self.status.clone() {
                WorkerStatus::Started | WorkerStatus::Starting | WorkerStatus::Connecting => {
                    log::debug!("Start ignored, already {:?}", self.status);
                    Outcome::Continue
                }
                WorkerStatus::FailedToRespond(_) => {
                    // Requests are currently failing; a new attempt may
                    // recover the (possibly crashed) embedded server.
                    log::info!("Start while FailedToRespond, respawning the local server");
                    provider.update_address(&self.active_address());
                    self.consecutive_failures = 0;
                    self.start_local_server(provider, runtime);
                    *last_text = String::default();
                    Outcome::Continue
                }
                WorkerStatus::Failed(reason) => {
                    log::warn!("Start ignored while Failed({reason}); use Retry/Stop/SetPort");
                    Outcome::Continue
                }
                WorkerStatus::Stopped => {
                    self.use_local_server = true;
                    // A previous remote session may have pointed the client
                    // elsewhere; Start means the embedded server.
                    provider.update_address(&self.active_address());
                    self.start_local_server(provider, runtime);
                    *last_text = String::default();
                    Outcome::Continue
                }
            },
            LanguageToolWorkerEvent::Stop => {
                log::info!("stopping the LanguageTool server");
                self.terminate_child();
                self.consecutive_failures = 0;
                self.emit_status(WorkerStatus::Stopped);
                Outcome::Continue
            }
            LanguageToolWorkerEvent::SetPort(port) => {
                log::info!("Setting LanguageTool port to {port}");
                self.local_port = port;
                if self.use_local_server {
                    provider.update_address(&self.active_address());
                    // New port means a fresh budget.
                    self.consecutive_failures = 0;
                    let was_running =
                        matches!(self.status, WorkerStatus::Started | WorkerStatus::Starting);
                    if was_running {
                        self.start_local_server(provider, runtime);
                        *last_text = String::default();
                    } else {
                        // Stay Stopped but re-emit so the UI picks up the
                        // port change (also clears Failed).
                        self.emit_status(WorkerStatus::Stopped);
                    }
                }
                Outcome::Continue
            }
            LanguageToolWorkerEvent::Retry => match self.status.clone() {
                WorkerStatus::Started | WorkerStatus::Starting | WorkerStatus::Connecting => {
                    log::debug!("Retry ignored, already {:?}", self.status);
                    Outcome::Continue
                }
                WorkerStatus::Stopped
                | WorkerStatus::Failed(_)
                | WorkerStatus::FailedToRespond(_) => {
                    log::info!("Retrying LanguageTool server at {}", self.active_address());
                    self.consecutive_failures = 0;
                    if self.use_local_server {
                        provider.update_address(&self.active_address());
                        self.start_local_server(provider, runtime);
                    } else {
                        self.verify_remote(provider, runtime);
                    }
                    *last_text = String::default();
                    Outcome::Continue
                }
            },
        }
    }

    fn verify_remote(
        &mut self,
        provider: &mut CompletionProvider,
        runtime: &tokio::runtime::Runtime,
    ) {
        if matches!(provider, CompletionProvider::Fake(_)) {
            self.record_success();
            return;
        }

        let was_active = matches!(
            self.status,
            WorkerStatus::Started | WorkerStatus::FailedToRespond(_)
        );
        // Connecting, not Starting: nothing is spawned locally.
        self.emit_status(WorkerStatus::Connecting);
        let deadline = Instant::now() + self.startup_timeout;
        while Instant::now() < deadline {
            // Abort promptly on lifecycle events, but don't lose colors/text.
            match self.event_receiver.recv_timeout(Duration::from_millis(300)) {
                Ok(LanguageToolWorkerEvent::UpdateSuggestions(_)) => continue,
                Ok(LanguageToolWorkerEvent::UpdateColors(colors)) => {
                    provider.set_colors(colors);
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
            if runtime.block_on(provider.health_check()) {
                self.record_success();
                return;
            }
        }
        let reason = format!(
            "remote LanguageTool at {} did not answer within {:?}",
            provider.address(),
            self.startup_timeout
        );
        if was_active {
            // A reconnect attempt of a previously working server failed: stay
            // in FailedToRespond so typing keeps retrying and the status still
            // shows the outage, instead of dead-ending in Stopped.
            log::warn!("reconnect attempt failed, waiting for the next request: {reason}");
            self.emit_status(WorkerStatus::FailedToRespond(reason));
        } else {
            self.record_failure(reason);
        }
    }

    fn start_local_server(
        &mut self,
        provider: &mut CompletionProvider,
        runtime: &tokio::runtime::Runtime,
    ) {
        if matches!(provider, CompletionProvider::Fake(_)) {
            self.record_success();
            return;
        }

        self.emit_status(WorkerStatus::Starting);
        loop {
            self.terminate_child();

            if !Self::port_is_free(self.local_port) {
                provider.update_address(&self.active_address());
                if runtime.block_on(provider.health_check()) {
                    log::info!(
                        "adopting LanguageTool already running on port {}",
                        self.local_port
                    );
                    self.record_success();
                    return;
                }
                self.record_failure(format!(
                    "port {} is already in use and no LanguageTool answers there",
                    self.local_port
                ));
                return;
            }

            let child = match Self::spawn_server(self.local_port) {
                Ok(c) => c,
                Err(reason) => {
                    self.record_failure(reason);
                    return;
                }
            };
            self.local_server_handle = Some(child);

            match self.wait_until_ready(provider, runtime) {
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
                            | LanguageToolWorkerEvent::SwitchToRemoteServer(_)
                    );

                    let mut empty_string = String::default();
                    let outcome = self.process_event(event, provider, runtime, &mut empty_string);
                    if matches!(outcome, Outcome::Stop) {
                        let _ = self.event_sender.send(LanguageToolWorkerEvent::Kill);
                        return;
                    }
                    if needs_restart && matches!(self.status, WorkerStatus::Starting) {
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

    fn wait_until_ready(
        &mut self,
        provider: &mut CompletionProvider,
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
                    provider.set_colors(colors);
                    continue;
                }
                Ok(LanguageToolWorkerEvent::Kill) => return VerifyOutcome::Killed,
                Ok(other @ LanguageToolWorkerEvent::Stop)
                | Ok(other @ LanguageToolWorkerEvent::StopLocalServer)
                | Ok(other @ LanguageToolWorkerEvent::SetPort(_))
                | Ok(other @ LanguageToolWorkerEvent::RestartLocalServer(_))
                | Ok(other @ LanguageToolWorkerEvent::Start)
                | Ok(other @ LanguageToolWorkerEvent::Retry)
                | Ok(other @ LanguageToolWorkerEvent::SwitchToRemoteServer(_)) => {
                    return VerifyOutcome::Aborted(other);
                }
                Err(_) => {}
            }

            if runtime.block_on(provider.health_check()) {
                return VerifyOutcome::Ready;
            }
        }
        VerifyOutcome::Timeout(format!(
            "LanguageTool server at {} did not become ready within {:?}",
            self.active_address(),
            self.startup_timeout
        ))
    }

    fn port_is_free(port: u16) -> bool {
        std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
    }

    fn language_tool_dir() -> Option<PathBuf> {
        let mut candidates = vec![PathBuf::from("/usr/share/rhesis/LanguageTool")];

        // Windows: installer / portable zip layout, next to the executable.
        // Goes first, like every packaged location.
        #[cfg(windows)]
        if let Some(dir) = rhesis_platform::process::exe_dir() {
            candidates.insert(0, dir.join("../share/rhesis/LanguageTool"));
            candidates.insert(0, dir.join("LanguageTool"));
        }

        // Per-user install: $HOME/.local/share/rhesis/LanguageTool
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(".local/share/rhesis/LanguageTool"));
        }

        // Flatpak: standard app layout
        candidates.push(PathBuf::from("/app/share/rhesis/LanguageTool"));

        // AppImage: path relative to $APPDIR
        if let Ok(appdir) = std::env::var("APPDIR") {
            candidates.push(PathBuf::from(format!(
                "{appdir}/app/share/rhesis/LanguageTool"
            )));
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

    fn java_executable() -> PathBuf {
        #[cfg(windows)]
        if let Some(dir) = rhesis_platform::process::exe_dir() {
            let bundled = dir.join("jre").join("bin").join("java.exe");
            if bundled.is_file() {
                return bundled;
            }
        }
        PathBuf::from("java")
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
        if !Self::port_is_free(port) {
            return Err(format!(
                "port {port} is already in use; stop the process holding it first"
            ));
        }

        Self::ensure_server_properties(&lt_dir);

        let mut command = Command::new(Self::java_executable());
        command.args([
            "-cp",
            "languagetool-server.jar",
            "org.languagetool.server.HTTPServer",
        ]);
        // Without fastText there is no server.properties (and nothing to put
        // in it): LanguageTool exits on a --config file that does not exist.
        if lt_dir.join("server.properties").is_file() {
            command.args(["--config", "server.properties"]);
        }
        command
            .args(["--port", &port.to_string(), "--allow-origin"])
            .current_dir(&lt_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // A GUI app has no console: Windows would open a console window
        // for the JVM (no-op everywhere else).
        rhesis_platform::process::suppress_console(&mut command);

        let mut child = match command.spawn() {
            Ok(c) => c,
            Err(e) => {
                return Err(format!("failed to spawn LanguageTool: {e:?}"));
            }
        };

        // The JVM must not outlive Rhesis (a crash would otherwise keep the
        // port taken until the user kills it by hand). Job-object custody on
        // Windows; a no-op everywhere else.
        if let Err(e) = rhesis_platform::process::kill_child_on_exit(&child) {
            log::warn!("could not tie LanguageTool to the Rhesis process: {e}");
        }

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

    fn ensure_server_properties(lt_dir: &Path) {
        let props = lt_dir.join("server.properties");
        if props.is_file() {
            return;
        }
        for (model, binary) in FASTTEXT_LAYOUTS {
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
    use super::*;

    fn scratch_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("rhesis-lt-test-{}-{name}", std::process::id()));
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
