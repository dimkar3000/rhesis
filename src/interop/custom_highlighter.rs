use std::pin::Pin;

use cxx_qt::CxxQtType;
use cxx_qt_lib::QString;

use crate::{
    interop::recommendation::Recommendation,
    languagetool::service::{Message, Suggestion, WorkerStatus},
};

#[derive(Default)]
pub struct CustomHighlighterRust {
    pub(crate) recommendations: Vec<Recommendation>,

    pub(crate) handle: Option<std::thread::JoinHandle<()>>,
}

impl CustomHighlighterRust {
    pub fn highlight_block(&self) -> Vec<(i32, i32, QString)> {
        self.recommendations
            .iter()
            .map(|r| {
                let color = r.color.clone();
                let start = r.range.start;
                let length = r.range.length;
                (start, length, color)
            })
            .collect()
    }

    // Split a status update into the numeric QML code (see
    // `WorkerStatus::code`: 0 = Stopped, 1 = Starting, 2 = Started,
    // 3 = Failed) plus the human-readable failure reason (empty unless Failed).
    fn status_to_qml(status: &WorkerStatus) -> (i32, QString) {
        let reason = match status {
            WorkerStatus::Failed(reason) => QString::from(reason.as_str()),
            _ => QString::default(),
        };
        (status.code(), reason)
    }

    /// Sole consumer of the worker's message channel. Fans out suggestions to
    /// the highlighter and status changes to the messaging helper (top bar).
    pub fn start_message_thread(
        &mut self,
        helper: Pin<&mut crate::interop::async_messaging_helper::AsyncMessagingHelperRust>,
        helper_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
        highlighter_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::CustomHighlighter>,
    ) {
        if self.handle.is_some() {
            log::debug!("highlighter message thread already running");
            return;
        }

        let helper = helper.get_mut();
        helper.ensure_worker_running();

        let receiver = match &helper.message_receiver {
            Some(receiver) => receiver.clone(),
            None => {
                log::error!("LanguageTool worker has no message receiver");
                return;
            }
        };

        log::info!("starting highlighter message thread");
        self.handle = Some(std::thread::spawn(move || loop {
            match receiver.recv() {
                Ok(Message::Suggestion(Suggestion(suggestions))) => {
                    match highlighter_thread.queue(move |mut highlighter| {
                        highlighter.as_mut().rust_mut().recommendations = suggestions;
                        highlighter.as_mut().rehighlight();
                    }) {
                        Ok(_) => (),
                        Err(e) => {
                            log::error!(
                                "error queuing suggestions to the UI thread, stopping: {e:?}"
                            );
                            break;
                        }
                    };
                }
                Ok(Message::StatusChanged(status)) => {
                    let (status_code, reason_str) = Self::status_to_qml(&status);
                    match helper_thread.queue(move |mut helper| {
                        helper.as_mut().set_server_status(status_code);
                        helper.as_mut().set_server_status_reason(reason_str);
                    }) {
                        Ok(_) => (),
                        Err(e) => {
                            log::error!("error queuing status to the UI thread, stopping: {e:?}");
                            break;
                        }
                    };
                }
                Err(e) => {
                    log::warn!("suggestion channel closed, highlighter thread stopping: {e:?}");
                    break;
                }
            }
        }));
    }
}
