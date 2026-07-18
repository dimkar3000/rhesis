use std::{
    io::{BufRead, BufReader},
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

use cxx_qt_lib::QString;
use tokio::{
    sync::watch::{channel, Receiver, Sender},
    task::JoinHandle,
    time::sleep,
};

use crate::languagetool::{
    client::LanguageToolClient,
    service::{Message, Suggestion},
};

pub struct AsyncMessagingHelperRust {
    pub message_sender: Sender<Message>,
    pub message_receiver: Receiver<Message>,
    pub suggestion_sender: Sender<Suggestion>,
    pub suggestion_receiver: Receiver<Suggestion>,

    pub languagetool_handle: Option<Child>,

    pub handle: Option<JoinHandle<()>>,
}

impl Default for AsyncMessagingHelperRust {
    fn default() -> Self {
        let (message_sender, message_receiver) =
            channel::<Message>(Message::Suggestion(QString::default()));
        let (suggestion_sender, suggestion_receiver) = channel::<Suggestion>(Suggestion::default());

        Self {
            message_sender,
            message_receiver,
            suggestion_sender,
            suggestion_receiver,
            languagetool_handle: None,
            handle: None,
        }
    }
}

impl Drop for AsyncMessagingHelperRust {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.as_ref() {
            log::info!("aborting messaging thread");
            handle.abort();
        }
        if let Some(mut child) = self.languagetool_handle.take() {
            log::info!("Killing LanguageTool");
            let _ = child.kill();
        }
    }
}

impl AsyncMessagingHelperRust {
    pub fn start_async_worker(&mut self, port: &str) {
        let mut message_receiver = self.message_receiver.clone();
        let suggestion_sender = self.suggestion_sender.clone();

        let mut client = LanguageToolClient::new_local(port);

        self.handle = Some(tokio::spawn(async move {
            let mut last_text = QString::default();
            loop {
                let _ = message_receiver.changed().await;

                loop {
                    let debounce = sleep(Duration::from_millis(300));
                    tokio::pin!(debounce);
                    tokio::select! {
                        _ = &mut debounce => break,
                        _ = message_receiver.changed() => {}
                    }
                }
                let message = message_receiver.borrow().clone();
                let text = match message {
                    Message::Suggestion(x) => x,
                    Message::UpdateColors(x) => {
                        client.set_colors(x);
                        if !last_text.trimmed().is_empty() {
                            let suggestions =
                                client.get_recommendation(last_text.to_string()).await;
                            let _ = suggestion_sender.send(Suggestion(suggestions));
                        }
                        continue;
                    }
                };

                if text == last_text || text.trimmed().is_empty() {
                    continue;
                }

                last_text = text.clone();

                let suggestions = client.get_recommendation(text.to_string()).await;
                let _ = suggestion_sender.send(Suggestion(suggestions));
            }
        }));
    }

    pub fn restart(&mut self, embedded: bool, port: &str) {
        log::info!("restart called, embedded={embedded}, port={port}");
        if let Some(mut child) = self.languagetool_handle.take() {
            log::trace!("aborting LanguageTool");
            let _ = child.kill();
        }

        if let Some(handle) = self.handle.take() {
            log::trace!("aborting messaging job");
            handle.abort();
        }

        log::trace!("restarting messaging job.");
        self.start_async_worker(port);

        let port = port.to_string();
        if embedded {
            log::trace!("Starting LanguageTool at: {port}");
            self.setup_child(port);
        }
    }

    /// Search for the path to LanguageTool in 3 places.
    /// - First next to the executable for release artifacts
    /// - Second inside the ${CWS}/build folder for local dev
    /// - Third in the CWD for general use
    fn language_tool_dir() -> PathBuf {
        if let Ok(dir) = std::env::var("RHESIS_LANGUAGETOOL_DIR") {
            let p = PathBuf::from(dir);
            if p.is_dir() {
                return p;
            }
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                if let Some(prefix) = exe_dir.parent() {
                    let flatpak_path = prefix.join("LanguageTool");
                    if flatpak_path.is_dir() {
                        return flatpak_path;
                    }
                }
            }
        }
        let build_path = PathBuf::from("./build/LanguageTool");
        if build_path.is_dir() {
            return build_path;
        }
        PathBuf::from("./LanguageTool")
    }

    fn setup_child(&mut self, port: String) {
        let lt_dir = Self::language_tool_dir();
        let java_path = std::env::var("PATH").unwrap_or_default();
        log::info!("LanguageTool dir: {:?}, PATH: {}", lt_dir, java_path);

        let result = Command::new("java")
            .args([
                "-cp",
                "languagetool-server.jar",
                "org.languagetool.server.HTTPServer",
                "--config",
                "server.properties",
                "--port",
                &port,
                "--allow-origin",
            ])
            .current_dir(&lt_dir)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn();

        let mut child = match result {
            Ok(c) => c,
            Err(e) => {
                log::error!("Failed to spawn LanguageTool: {e:?}");
                return;
            }
        };

        // everything under here is probably over-engineered

        if let Some(out) = child.stdout.take() {
            tokio::spawn(async move {
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
            tokio::spawn(async move {
                let reader = BufReader::new(error);
                for line in reader.lines() {
                    match line {
                        Ok(line) => log::trace!("[LanguageTool]: {line}"),
                        Err(e) => log::error!("Error from stderr reader: {e:?}"),
                    }
                }
            });
        }

        self.languagetool_handle = Some(child);
    }
}
