use std::pin::Pin;

use crossbeam::channel::Receiver;
use cxx_qt::CxxQtType;
use cxx_qt_lib::{QMap, QMapPair_QString_QVariant, QString, QVariant};

use rhesis_protocol::daemon_msg::DaemonMsg;
use rhesis_protocol::recommendation::Recommendation;
use rhesis_settings::settings::Settings;

use crate::connection::{Connection, ConnectionEvent};
use crate::constants::CLIENT_STATUS_DISCONNECTED;

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
                let color = QString::from(&r.color);
                let start = r.range.start;
                let length = r.range.length;
                (start, length, color)
            })
            .collect()
    }

    /// Wires this UI to the daemon (Refactor.md §5): the single-instance
    /// runner for later launches, then the connection loop feeding the
    /// fan-out thread. The helper receives the connection handle so its
    /// invoked commands and its `Drop` shutdown reach the same loop.
    ///
    /// Old shape preserved: called once from QML after both objects exist.
    /// Ownership moved: no worker starts here — the daemon owns it.
    pub fn start_message_thread(
        &mut self,
        helper: Pin<&mut crate::interop::async_messaging_helper::AsyncMessagingHelperRust>,
        helper_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::AsyncMessagingHelper>,
        highlighter_thread: cxx_qt::CxxQtThread<crate::interop::bridge::ffi::CustomHighlighter>,
    ) {
        if self.handle.is_some() {
            log::debug!("message thread already running");
            return;
        }

        let helper = helper.get_mut();

        // The single-instance runner dispatches "show" requests from later
        // launches; it needs this Qt thread and only exists once QML is up.
        let show_request = {
            let helper_thread = helper_thread.clone();
            move || {
                if let Err(e) =
                    helper_thread.queue(|_helper| crate::interop::bridge::ffi::appRaiseMainWindow())
                {
                    log::warn!("failed to queue show request to the UI thread: {e:?}");
                }
            }
        };
        if !rhesis_platform::single_instance::spawn_runner(show_request) {
            log::debug!("single-instance runner not spawned");
        }

        // The connection owns its own threads; this fan-out is only the
        // bridge between its event channel and the two Qt objects.
        let (events, incoming) = crossbeam::channel::unbounded::<ConnectionEvent>();
        helper.connection = Some(Connection::start(events));

        log::info!("starting message fan-out thread");
        self.handle = Some(std::thread::spawn(move || {
            let sink = QtMessageSink {
                helper_thread,
                highlighter_thread,
            };
            fan_out_messages(incoming, sink);
        }));
    }
}

/// Qt-side boundary conversion: a `Recommendation` rehydrated from the
/// daemon becomes the QVariant map the QML suggestion popup reads
/// (`value`, `tooltip`, ... keys — see the tests at the bottom). Lives
/// here, not as a `From` impl: both types are foreign now that the
/// recommendation struct lives in the rhesis-protocol crate.
pub(crate) fn recommendation_to_variant(r: &Recommendation) -> QVariant {
    let string_field = |value: &String| QVariant::from(&QString::from(value));

    let mut map = QMap::<QMapPair_QString_QVariant>::default();
    map.insert(QString::from("value"), string_field(&r.value));
    map.insert(QString::from("tooltip"), string_field(&r.tooltip));
    map.insert(QString::from("color"), string_field(&r.color));
    map.insert(QString::from("rule_id"), string_field(&r.rule_id));
    map.insert(QString::from("category_id"), string_field(&r.category_id));
    map.insert(QString::from("language"), string_field(&r.language));
    map.insert(QString::from("range_start"), QVariant::from(&r.range.start));
    map.insert(
        QString::from("range_length"),
        QVariant::from(&r.range.length),
    );
    QVariant::from(&map)
}

/// Delivers daemon frames to the UI thread: suggestions to the highlighter,
/// status changes, snapshot mirrors and client-local disconnects to the
/// messaging helper (top bar, settings page).
///
/// The production sink ([`QtMessageSink`]) queues onto the Qt event loops of
/// both UI objects. Tests drive [`fan_out_messages`] with an in-memory sink
/// instead, making the fan-out contract — one delivery per frame, stop on
/// the first undeliverable message or on channel close; a `Lost` event must
/// NOT stop the loop (reconnect replays follow it) — assertable without any
/// Qt objects.
pub(crate) trait MessageSink: Send {
    /// Queue a batch of suggestions for rendering. An `Err` means the UI side
    /// is gone; the fan-out loop must stop.
    fn send_suggestions(&mut self, suggestions: Vec<Recommendation>) -> Result<(), String>;

    /// Queue a status update (wire code + failure reason). An `Err` means
    /// the UI side is gone.
    fn send_status(&mut self, code: i32, reason: &str) -> Result<(), String>;

    /// Queue a settings snapshot mirror.
    fn send_settings(&mut self, settings: &Settings) -> Result<(), String>;

    /// Queue the client-local "no session" state (code 6).
    fn send_disconnected(&mut self) -> Result<(), String>;
}

/// Sole consumer of the connection's event channel: fans each event out to
/// `sink`. Stops on the first delivery failure (UI thread gone) or when the
/// channel closes (connection loop dropped); a `Lost` event only flips the
/// UI state — the loop stays open for the reconnect replay.
pub(crate) fn fan_out_messages<S: MessageSink>(
    receiver: Receiver<ConnectionEvent>,
    mut sink: S,
) -> S {
    loop {
        match receiver.recv() {
            Ok(ConnectionEvent::Wire(DaemonMsg::Suggestion(suggestions))) => {
                if let Err(e) = sink.send_suggestions(suggestions) {
                    log::error!("{e}, stopping the message thread");
                    return sink;
                }
            }
            Ok(ConnectionEvent::Wire(DaemonMsg::Status { code, reason })) => {
                if let Err(e) = sink.send_status(code, &reason) {
                    log::error!("{e}, stopping the message thread");
                    return sink;
                }
            }
            Ok(ConnectionEvent::Wire(DaemonMsg::Settings(settings))) => {
                if let Err(e) = sink.send_settings(&settings) {
                    log::error!("{e}, stopping the message thread");
                    return sink;
                }
            }
            // A connect frame belongs to a handshake the connection loop
            // already handled; nothing on an established session should ever
            // carry one. Log it and keep the channel alive.
            Ok(ConnectionEvent::Wire(DaemonMsg::Connect { protocol, busy })) => {
                log::warn!("unexpected connect frame mid-session: protocol={protocol} busy={busy}");
            }
            Ok(ConnectionEvent::Lost) => {
                if let Err(e) = sink.send_disconnected() {
                    log::error!("{e}, stopping the message thread");
                    return sink;
                }
                // No stop: the loop reconnects and will replay.
            }
            Err(e) => {
                log::debug!("event channel closed, fan-out stopping: {e:?}");
                return sink;
            }
        }
    }
}

/// Production sink: queues onto the two UI objects' Qt threads.
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

    fn send_status(&mut self, code: i32, reason: &str) -> Result<(), String> {
        let reason = QString::from(reason);
        self.helper_thread
            .queue(move |mut helper| {
                helper.as_mut().set_server_status(code);
                helper.as_mut().set_server_status_reason(reason);
            })
            .map_err(|e| format!("error queuing status to the UI thread: {e:?}"))
    }

    fn send_settings(&mut self, settings: &Settings) -> Result<(), String> {
        let settings = settings.clone();
        self.helper_thread
            .queue(move |mut helper| {
                helper.as_mut().apply_settings(&settings);
            })
            .map_err(|e| format!("error queuing settings to the UI thread: {e:?}"))
    }

    fn send_disconnected(&mut self) -> Result<(), String> {
        self.helper_thread
            .queue(move |mut helper| {
                helper
                    .as_mut()
                    .set_server_status(CLIENT_STATUS_DISCONNECTED);
                helper.as_mut().set_server_status_reason(QString::default());
            })
            .map_err(|e| format!("error queuing the disconnect state: {e:?}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ConnectionEvent;
    use crossbeam::channel::unbounded;
    use rhesis_protocol::recommendation::Range;

    // Wire status codes (DaemonMsg::Status): the fan-out tests' set.
    const STARTED: i32 = 2;
    const CONNECTING: i32 = 4;

    /// A one-word recommendation at `start` — the minimal suggestion shape
    /// the rendering pipeline consumes (a range to paint + a replacement).
    fn one_word(start: i32, value: &str) -> Recommendation {
        Recommendation {
            value: value.to_string(),
            range: Range { start, length: 1 },
            ..Default::default()
        }
    }

    /// Collected fan-out events, in delivery order.
    #[derive(Debug, PartialEq)]
    enum SpyEvent {
        Suggestions(Vec<Recommendation>),
        Status(i32, String),
        SettingsSnapshot(Settings),
        Disconnected,
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

        fn send_status(&mut self, code: i32, reason: &str) -> Result<(), String> {
            if self.fail_status {
                return Err("spy sink: UI thread is gone (status)".to_string());
            }
            self.events.push(SpyEvent::Status(code, reason.to_string()));
            Ok(())
        }

        fn send_settings(&mut self, settings: &Settings) -> Result<(), String> {
            self.events
                .push(SpyEvent::SettingsSnapshot(settings.clone()));
            Ok(())
        }

        fn send_disconnected(&mut self) -> Result<(), String> {
            self.events.push(SpyEvent::Disconnected);
            Ok(())
        }
    }

    #[test]
    fn fan_out_delivers_every_frame_then_stops_on_channel_close() {
        let (sender, receiver) = unbounded::<ConnectionEvent>();
        let sink = SpySink::new();

        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Settings(
                Settings::default(),
            )))
            .unwrap();
        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Status {
                code: STARTED,
                reason: String::new(),
            }))
            .unwrap();
        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Suggestion(vec![
                one_word(0, "a"),
                one_word(2, "b"),
            ])))
            .unwrap();
        sender.send(ConnectionEvent::Lost).unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        assert_eq!(
            sink.events,
            vec![
                SpyEvent::SettingsSnapshot(Settings::default()),
                SpyEvent::Status(STARTED, String::new()),
                SpyEvent::Suggestions(vec![one_word(0, "a"), one_word(2, "b")]),
                SpyEvent::Disconnected,
            ]
        );
    }

    #[test]
    fn lost_events_do_not_stop_the_fan_out() {
        let (sender, receiver) = unbounded::<ConnectionEvent>();
        let sink = SpySink::new();

        sender.send(ConnectionEvent::Lost).unwrap();
        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Status {
                code: CONNECTING,
                reason: String::new(),
            }))
            .unwrap();
        sender.send(ConnectionEvent::Lost).unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        // Both disconnects AND the replay status that follows them arrive:
        // the reconnect story is intact.
        assert_eq!(
            sink.events,
            vec![
                SpyEvent::Disconnected,
                SpyEvent::Status(CONNECTING, String::new()),
                SpyEvent::Disconnected,
            ]
        );
    }

    #[test]
    fn fan_out_stops_suggestions_when_the_ui_thread_is_gone() {
        let (sender, receiver) = unbounded::<ConnectionEvent>();
        let sink = SpySink {
            fail_suggestions: true,
            ..SpySink::new()
        };

        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Suggestion(vec![
                one_word(0, "a"),
            ])))
            .unwrap();
        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Status {
                code: STARTED,
                reason: String::new(),
            }))
            .unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        // The failed delivery stops the loop before the status is sent.
        assert!(sink.events.is_empty());
    }

    #[test]
    fn fan_out_stops_statuses_when_the_ui_thread_is_gone() {
        let (sender, receiver) = unbounded::<ConnectionEvent>();
        let sink = SpySink {
            fail_status: true,
            ..SpySink::new()
        };

        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Status {
                code: STARTED,
                reason: String::new(),
            }))
            .unwrap();
        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Suggestion(vec![
                one_word(0, "a"),
            ])))
            .unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        // The failed delivery stops the loop before the suggestion is sent.
        assert!(sink.events.is_empty());
    }

    #[test]
    fn a_connect_frame_is_skipped_not_fatal() {
        let (sender, receiver) = unbounded::<ConnectionEvent>();
        let sink = SpySink::new();

        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Connect {
                protocol: 1,
                busy: false,
            }))
            .unwrap();
        sender
            .send(ConnectionEvent::Wire(DaemonMsg::Status {
                code: STARTED,
                reason: String::new(),
            }))
            .unwrap();
        drop(sender);

        let sink = fan_out_messages(receiver, sink);

        assert_eq!(sink.events, vec![SpyEvent::Status(STARTED, String::new())]);
    }

    /// A filled-in recommendation covering every field the QML popup reads.
    fn sample_recommendation() -> Recommendation {
        Recommendation {
            range: Range {
                start: 4,
                length: 3,
            },
            value: "the".to_string(),
            color: "#112233".to_string(),
            rule_id: "RULE_1".to_string(),
            category_id: "TYPOS".to_string(),
            tooltip: "Possible spelling mistake".to_string(),
            language: "EN-US".to_string(),
        }
    }

    #[test]
    fn converts_to_qml_variant_map() {
        let recommendation = sample_recommendation();

        let variant = recommendation_to_variant(&recommendation);
        let map = variant
            .value::<QMap<QMapPair_QString_QVariant>>()
            .expect("conversion must produce a map");

        let string_field = |key: &str| -> String {
            map.get(&QString::from(key))
                .and_then(|value| value.value::<QString>())
                .unwrap_or_default()
                .to_string()
        };

        assert_eq!(string_field("value"), "the");
        assert_eq!(string_field("color"), "#112233");
        assert_eq!(string_field("rule_id"), "RULE_1");
        assert_eq!(string_field("category_id"), "TYPOS");
        assert_eq!(string_field("tooltip"), "Possible spelling mistake");
        assert_eq!(string_field("language"), "EN-US");

        let start = map
            .get(&QString::from("range_start"))
            .and_then(|value| value.value::<i32>())
            .expect("range_start must be an integer");
        let length = map
            .get(&QString::from("range_length"))
            .and_then(|value| value.value::<i32>())
            .expect("range_length must be an integer");
        assert_eq!(start, 4);
        assert_eq!(length, 3);
    }

    #[test]
    fn all_fields_are_exposed() {
        let variant = recommendation_to_variant(&sample_recommendation());
        let map = variant
            .value::<QMap<QMapPair_QString_QVariant>>()
            .expect("conversion must produce a map");

        let expected_keys = [
            "value",
            "tooltip",
            "color",
            "rule_id",
            "category_id",
            "language",
            "range_start",
            "range_length",
        ];
        let mut seen: Vec<String> = map.iter().map(|(key, _)| key.to_string()).collect();
        seen.sort();

        let mut expected: Vec<String> = expected_keys.iter().map(|k| k.to_string()).collect();
        expected.sort();

        assert_eq!(seen, expected);
    }

    #[test]
    fn defaults_are_zero_value() {
        let recommendation = Recommendation::default();
        assert_eq!(recommendation.range, Range::default());
        assert_eq!(recommendation.value, "");
        assert_eq!(recommendation.range.start, 0);
        assert_eq!(recommendation.range.length, 0);
    }
}
