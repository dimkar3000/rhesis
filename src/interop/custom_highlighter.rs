use std::pin::Pin;

use crossbeam::channel::Receiver;
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
    // 3 = Failed, 4 = Connecting, 5 = FailedToRespond) plus the
    // human-readable failure reason (empty unless the status failed).
    fn status_to_qml(status: &WorkerStatus) -> (i32, QString) {
        let reason = match status {
            WorkerStatus::Failed(reason) | WorkerStatus::FailedToRespond(reason) => {
                QString::from(reason.as_str())
            }
            _ => QString::default(),
        };
        (status.code(), reason)
    }

    /// Sole consumer of the worker's message channel. Spawns the fan-out
    /// thread delivering suggestions to the highlighter and status changes to
    /// the messaging helper (top bar).
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

        // The single-instance runner dispatches "show" requests from later
        // launches; it needs this Qt thread and only exists once QML is up.
        if crate::single_instance::spawn_runner(helper_thread.clone()).is_none() {
            log::debug!("single-instance runner not spawned");
        }

        let receiver = match &helper.message_receiver {
            Some(receiver) => receiver.clone(),
            None => {
                log::error!("LanguageTool worker has no message receiver");
                return;
            }
        };

        log::info!("starting highlighter message thread");
        self.handle = Some(std::thread::spawn(move || {
            let sink = QtMessageSink {
                helper_thread,
                highlighter_thread,
            };
            fan_out_messages(receiver, sink);
        }));
    }
}

/// Delivers worker messages to the UI thread: suggestions go to the
/// highlighter, status changes go to the messaging helper (top bar).
///
/// The production sink ([`QtMessageSink`]) queues onto the Qt event loops of
/// both UI objects. Tests drive [`fan_out_messages`] with an in-memory sink
/// instead, making the fan-out contract — one delivery per message, stop on
/// the first undeliverable message or on channel close — assertable without
/// any Qt objects.
pub(crate) trait MessageSink: Send {
    /// Queue a batch of suggestions for rendering. An `Err` means the UI side
    /// is gone; the fan-out loop must stop.
    fn send_suggestions(&mut self, suggestions: Vec<Recommendation>) -> Result<(), String>;

    /// Queue a status update. An `Err` means the UI side is gone.
    fn send_status(&mut self, status: WorkerStatus) -> Result<(), String>;
}

/// Production sink: queues onto the two UI objects' Qt threads, mirroring the
/// un-referenced fan-out that used to live inline in the message thread.
struct QtMessageSink {
    helper_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
    highlighter_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::CustomHighlighter>,
}

impl MessageSink for QtMessageSink {
    fn send_suggestions(&mut self, suggestions: Vec<Recommendation>) -> Result<(), String> {
        self.highlighter_thread
            .queue(move |mut highlighter| {
                highlighter.as_mut().rust_mut().recommendations = suggestions;
                highlighter.as_mut().rehighlight();
            })
            .map_err(|e| format!("error queuing suggestions to the UI thread: {e:?}"))
    }

    fn send_status(&mut self, status: WorkerStatus) -> Result<(), String> {
        let (status_code, reason_str) = CustomHighlighterRust::status_to_qml(&status);
        self.helper_thread
            .queue(move |mut helper| {
                helper.as_mut().set_server_status(status_code);
                helper.as_mut().set_server_status_reason(reason_str);
            })
            .map_err(|e| format!("error queuing status to the UI thread: {e:?}"))
    }
}

/// Sole consumer of the worker's message channel: fans each message out to
/// `sink`, stopping at the first delivery failure (UI thread gone) or when
/// the channel closes (worker exited).
pub(crate) fn fan_out_messages<S: MessageSink>(receiver: Receiver<Message>, mut sink: S) -> S {
    loop {
        match receiver.recv() {
            Ok(Message::Suggestion(Suggestion(suggestions))) => {
                if let Err(e) = sink.send_suggestions(suggestions) {
                    log::error!("{e}, stopping the message thread");
                    return sink;
                }
            }
            Ok(Message::StatusChanged(status)) => {
                if let Err(e) = sink.send_status(status) {
                    log::error!("{e}, stopping the message thread");
                    return sink;
                }
            }
            Err(e) => {
                log::warn!("suggestion channel closed, message thread stopping: {e:?}");
                return sink;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interop::recommendation::test_support::one_word;
    use crossbeam::channel::unbounded;

    /// Collected fan-out events, in delivery order.
    #[derive(Debug, PartialEq)]
    enum SpyEvent {
        Suggestions(Vec<Recommendation>),
        Status(WorkerStatus),
    }

    /// In-memory [`MessageSink`]: records every call, optionally failing all
    /// sends of one kind to drive the stop-on-error contract.
    struct SpySink {
        events: Vec<SpyEvent>,
        fail_suggestions: bool,
        fail_status: bool,
    }

    impl SpySink {
        fn new() -> Self {
            Self {
                events: Vec::new(),
                fail_suggestions: false,
                fail_status: false,
            }
        }
    }

    impl MessageSink for SpySink {
        fn send_suggestions(&mut self, suggestions: Vec<Recommendation>) -> Result<(), String> {
            if self.fail_suggestions {
                return Err("spy sink: UI thread is gone (suggestions)".to_string());
            }
            self.events.push(SpyEvent::Suggestions(suggestions));
            Ok(())
        }

        fn send_status(&mut self, status: WorkerStatus) -> Result<(), String> {
            if self.fail_status {
                return Err("spy sink: UI thread is gone (status)".to_string());
            }
            self.events.push(SpyEvent::Status(status));
            Ok(())
        }
    }

    #[test]
    fn status_to_qml_maps_codes() {
        let cases = [
            (WorkerStatus::Stopped, 0, ""),
            (WorkerStatus::Starting, 1, ""),
            (WorkerStatus::Started, 2, ""),
            (WorkerStatus::Failed("jar broke".into()), 3, "jar broke"),
            (WorkerStatus::Connecting, 4, ""),
            (
                WorkerStatus::FailedToRespond("timeout".into()),
                5,
                "timeout",
            ),
        ];
        for (status, code, reason) in cases {
            let (qcode, qreason) = CustomHighlighterRust::status_to_qml(&status);
            assert_eq!(qcode, code, "code for {status:?}");
            assert_eq!(qreason.to_string(), reason, "reason for {status:?}");
        }
    }

    #[test]
    fn fan_out_delivers_every_message_then_stops_on_channel_close() {
        let (sender, receiver) = unbounded::<Message>();
        let sink = SpySink::new();

        sender
            .send(Message::StatusChanged(WorkerStatus::Started))
            .unwrap();
        sender
            .send(Message::Suggestion(Suggestion(vec![
                one_word(0, "a"),
                one_word(2, "b"),
            ])))
            .unwrap();
        sender
            .send(Message::StatusChanged(WorkerStatus::Failed(
                "reason".into(),
            )))
            .unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        assert_eq!(
            sink.events,
            vec![
                SpyEvent::Status(WorkerStatus::Started),
                SpyEvent::Suggestions(vec![one_word(0, "a"), one_word(2, "b")]),
                SpyEvent::Status(WorkerStatus::Failed("reason".into())),
            ]
        );
    }

    #[test]
    fn fan_out_stops_suggestions_when_the_ui_thread_is_gone() {
        let (sender, receiver) = unbounded::<Message>();
        let sink = SpySink {
            fail_suggestions: true,
            ..SpySink::new()
        };

        sender
            .send(Message::Suggestion(Suggestion(vec![one_word(0, "a")])))
            .unwrap();
        sender
            .send(Message::StatusChanged(WorkerStatus::Started))
            .unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        // The failed delivery stops the loop before the status is sent.
        assert!(sink.events.is_empty());
    }

    #[test]
    fn fan_out_stops_statuses_when_the_ui_thread_is_gone() {
        let (sender, receiver) = unbounded::<Message>();
        let sink = SpySink {
            fail_status: true,
            ..SpySink::new()
        };

        sender
            .send(Message::StatusChanged(WorkerStatus::Started))
            .unwrap();
        sender
            .send(Message::Suggestion(Suggestion(vec![one_word(0, "a")])))
            .unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        // The failed delivery stops the loop before the suggestion is sent.
        assert!(sink.events.is_empty());
    }
}
