use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread::JoinHandle,
};

use crossbeam::channel::{Receiver, Sender};
use cxx_qt_lib::QString;

use crate::languagetool::service::{LanguageToolWorker, LanguageToolWorkerEvent, Message};

pub struct AsyncMessagingHelperRust {
    /// Clone of the worker's event channel, used to send UI events to it
    pub event_sender: Option<Sender<LanguageToolWorkerEvent>>,
    /// Clone of the worker's message channel, used to receive results from it.
    /// Sole owner is the dispatcher thread (see `CustomHighlighterRust`);
    /// do not clone it elsewhere to avoid stealing messages.
    pub message_receiver: Option<Receiver<Message>>,

    /// True while the LanguageTool worker thread is alive
    pub worker_running: Arc<AtomicBool>,
    pub worker_thread: Option<JoinHandle<()>>,

    // Mirrors `WorkerStatus` for QML (`qproperty` in bridge.rs).
    // Status codes: 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed.
    pub server_status: i32,
    pub server_status_reason: QString,
}

impl Default for AsyncMessagingHelperRust {
    fn default() -> Self {
        Self {
            event_sender: None,
            message_receiver: None,
            worker_running: Arc::new(AtomicBool::new(false)),
            worker_thread: None,
            server_status: 0, // Stopped
            server_status_reason: QString::default(),
        }
    }
}

impl Drop for AsyncMessagingHelperRust {
    fn drop(&mut self) {
        if self.worker_running.swap(false, Ordering::SeqCst) {
            if let Some(sender) = &self.event_sender {
                log::info!("Sending Kill to LanguageTool worker");
                if let Err(e) = sender.send(LanguageToolWorkerEvent::Kill) {
                    log::warn!("failed to send Kill to LanguageTool worker: {e:?}");
                }
            }
        }
        // Detach the worker thread; it exits on its own after processing Kill.
        self.worker_thread.take();
    }
}

impl AsyncMessagingHelperRust {
    /// Make sure the LanguageTool worker is running, starting it in a
    /// standalone thread if it isn't. The worker boots in `Stopped`.
    pub fn ensure_worker_running(&mut self) {
        if self.worker_running.load(Ordering::SeqCst) {
            return;
        }

        if let Some(thread) = self.worker_thread.take() {
            // The previous worker died; make sure its thread finished.
            log::debug!("joining previous LanguageTool worker thread");
            let _ = thread.join();
        }

        log::info!("Starting LanguageTool worker thread");

        let handles = LanguageToolWorker::default().start();

        self.event_sender = Some(handles.event_sender);
        self.message_receiver = Some(handles.message_receiver);
        self.worker_running = handles.running;
        self.worker_thread = Some(handles.thread);
    }

    fn send(&self, event: LanguageToolWorkerEvent) {
        match &self.event_sender {
            Some(sender) => {
                if let Err(e) = sender.send(event) {
                    log::warn!("failed to send event to the worker: {e:?}");
                }
            }
            None => log::warn!("event dropped, LanguageTool worker not running"),
        }
    }

    pub fn start_server(&mut self) {
        self.ensure_worker_running();
        log::info!("start_server requested");
        self.send(LanguageToolWorkerEvent::Start);
    }

    pub fn stop_server(&mut self) {
        self.ensure_worker_running();
        log::info!("stop_server requested");
        self.send(LanguageToolWorkerEvent::Stop);
    }

    pub fn set_server_port(&mut self, port: &str) {
        self.ensure_worker_running();
        match port.parse::<u16>() {
            Ok(p) => {
                log::info!("set_server_port {p}");
                self.send(LanguageToolWorkerEvent::SetPort(p));
            }
            Err(_) => log::warn!("invalid port {port:?}, ignoring"),
        }
    }

    pub fn retry_server(&mut self) {
        self.ensure_worker_running();
        log::info!("retry_server requested");
        self.send(LanguageToolWorkerEvent::Retry);
    }

    pub fn restart(&mut self, embedded: bool, address: &str) {
        log::info!("restart called, embedded={embedded}, address={address}");

        self.ensure_worker_running();

        let sender = match &self.event_sender {
            Some(sender) => sender,
            None => {
                log::error!("LanguageTool worker has no event sender");
                return;
            }
        };

        let event = if embedded {
            match address.parse() {
                Ok(port) => LanguageToolWorkerEvent::RestartLocalServer(port),
                Err(_) => {
                    log::warn!("invalid port {address:?} in restart, falling back to 2689");
                    LanguageToolWorkerEvent::RestartLocalServer(2689)
                }
            }
        } else {
            // Disabling the embedded server stops it.
            LanguageToolWorkerEvent::Stop
        };
        if let Err(e) = sender.send(event) {
            log::warn!("failed to send restart event to the worker: {e:?}");
        }
    }
}
