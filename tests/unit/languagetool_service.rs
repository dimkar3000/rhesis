//! LanguageTool worker: debounce, dedupe, status transitions, teardown.
//!
//! Every test here is hermetic: the worker talks to a mock HTTP listener on
//! localhost, never to a java-spawned LanguageTool server. Server boot
//! against a real installation is out of scope by design (see `AGENTS.md`).

use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::atomic::{AtomicBool, Ordering},
    sync::Arc,
    thread,
    time::Duration,
};

use crossbeam::channel::RecvTimeoutError;
use cxx_qt_lib::QString;
use rhesis::languagetool::service::{
    LanguageToolWorker, LanguageToolWorkerEvent, Message, Suggestion, WorkerHandles, WorkerStatus,
    MAX_START_FAILURES,
};

// Note: the `ensure_server_properties` tests were moved back to
// `src/languagetool/service.rs` because they exercise private layout-detection
// logic that matters for deployment correctness. The `port_is_free` tests
// were removed — they test stdlib `TcpListener::bind`, not Rhesis-specific
// behavior.

fn wait_for(predicate: impl Fn() -> bool, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if predicate() {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    predicate()
}

fn start_worker() -> WorkerHandles {
    LanguageToolWorker::default()
        .with_startup_timeout(Duration::from_secs(2))
        .start()
        .expect("worker thread must spawn in tests")
}

fn start_worker_with_timeout(timeout: Duration) -> WorkerHandles {
    LanguageToolWorker::default()
        .with_startup_timeout(timeout)
        .start()
        .expect("worker thread must spawn in tests")
}

/// Drain status messages until `predicate` matches, or timeout.
fn wait_for_status(
    handles: &WorkerHandles,
    predicate: impl Fn(&WorkerStatus) -> bool,
    timeout: Duration,
) -> Option<WorkerStatus> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match handles.message_receiver.recv_timeout(remaining) {
            Ok(Message::StatusChanged(s)) => {
                if predicate(&s) {
                    return Some(s);
                }
            }
            Ok(Message::Suggestion(_)) => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Receive the next `Suggestion`, skipping over `StatusChanged` noise.
fn recv_suggestion(handles: &WorkerHandles, timeout: Duration) -> Option<Suggestion> {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match handles.message_receiver.recv_timeout(remaining) {
            Ok(Message::Suggestion(s)) => return Some(s),
            Ok(Message::StatusChanged(_)) => continue,
            Err(_) => return None,
        }
    }
    None
}

/// Assert no `Suggestion` arrives within `timeout` (status noise ignored).
fn assert_no_suggestion(handles: &WorkerHandles, timeout: Duration) {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        match handles.message_receiver.recv_timeout(remaining) {
            Ok(Message::Suggestion(s)) => {
                panic!("expected no suggestion, got: {s:?}")
            }
            Ok(Message::StatusChanged(_)) => continue,
            Err(RecvTimeoutError::Timeout) => return,
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }
}

/// Minimal HTTP mock: answers 200 to everything. Enough for `health_check`
/// (status only) and for `get_recommendation` (which then yields an empty
/// suggestion list but still delivers a `Suggestion` message).
fn spawn_mock_server() -> (u16, thread::JoinHandle<()>) {
    let (port, _, handle) = spawn_toggleable_mock_server();
    (port, handle)
}

/// Same mock, but a flag decides whether requests succeed (200 + valid
/// LanguageTool JSON) or fail (503). Lets a test simulate a server going
/// down mid-session and coming back.
fn spawn_toggleable_mock_server() -> (u16, Arc<AtomicBool>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
    let port = listener.local_addr().unwrap().port();
    let healthy = Arc::new(AtomicBool::new(true));
    let flag = healthy.clone();
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            if !flag.load(Ordering::SeqCst) {
                let _ = stream.write_all(
                    b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
                continue;
            }
            // Minimal valid LanguageToolDto JSON would be ideal, but even an
            // empty object is fine: the client then returns [] yet still
            // sends a Suggestion message.
            let body = r#"{"software":{"name":"","version":"","buildDate":"","apiVersion":1,"premium":false,"premiumHint":"","status":""},"warnings":{"incompleteResults":false},"language":{"name":"","code":"en-US","detectedLanguage":{"name":"","code":"en-US","confidence":1.0,"source":""}},"matches":[],"sentenceRanges":[],"extendedSentenceRanges":[]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (port, healthy, handle)
}

fn kill(handles: &WorkerHandles) {
    let _ = handles.event_sender.send(LanguageToolWorkerEvent::Kill);
}

#[test]
fn worker_starts_running_and_kill_stops_it() {
    let handles = start_worker();
    assert!(handles.running.load(Ordering::SeqCst));

    // Boot state is Stopped.
    let status = wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Stopped),
        Duration::from_secs(5),
    );
    assert!(status.is_some(), "worker should report Stopped on boot");

    kill(&handles);

    assert!(
        wait_for(
            || !handles.running.load(Ordering::SeqCst),
            Duration::from_secs(5)
        ),
        "worker should stop after Kill"
    );
}

#[test]
fn boots_in_stopped_and_drops_suggestions_until_started() {
    let handles = start_worker();

    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Stopped),
        Duration::from_secs(5)
    )
    .is_some());

    // Suggestions are dropped while Stopped: no Suggestion message.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "hello world",
        )))
        .unwrap();
    assert_no_suggestion(&handles, Duration::from_millis(800));

    kill(&handles);
}

#[test]
fn empty_and_whitespace_text_produce_no_suggestions() {
    // Uses a mock remote server so the worker reaches Started; empty input
    // must still produce nothing.
    let (port, _mock) = spawn_mock_server();
    let handles = start_worker();
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SwitchToRemoteServer(format!(
            "localhost:{port}"
        )))
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(10)
        )
        .is_some(),
        "worker should reach Started against the mock server"
    );

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "",
        )))
        .unwrap();
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            " \t ",
        )))
        .unwrap();

    assert_no_suggestion(&handles, Duration::from_millis(800));

    kill(&handles);
}

#[test]
fn repeated_text_is_deduplicated() {
    let (port, _mock) = spawn_mock_server();
    let handles = start_worker();
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SwitchToRemoteServer(format!(
            "localhost:{port}"
        )))
        .unwrap();
    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Started),
        Duration::from_secs(10)
    )
    .is_some());

    // Same text twice: only the first one should produce an event.
    let text = QString::from("some text to check");
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(text.clone()))
        .unwrap();
    thread::sleep(Duration::from_millis(700));
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(text))
        .unwrap();

    assert!(
        recv_suggestion(&handles, Duration::from_secs(10)).is_some(),
        "expected a suggestion message"
    );

    // The deduplicated second event must not produce another round trip
    assert_no_suggestion(&handles, Duration::from_millis(1000));

    kill(&handles);
}

#[test]
fn suggestions_flow_against_mock_server() {
    let (port, _mock) = spawn_mock_server();
    let handles = start_worker();

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SwitchToRemoteServer(format!(
            "localhost:{port}"
        )))
        .unwrap();
    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Started),
        Duration::from_secs(10)
    )
    .is_some());

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "hello world",
        )))
        .unwrap();

    assert!(
        recv_suggestion(&handles, Duration::from_secs(10)).is_some(),
        "expected a suggestion message"
    );

    kill(&handles);
}

#[test]
fn stop_local_server_is_harmless_when_nothing_runs() {
    let handles = start_worker();
    // Drain boot status.
    let _ = wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Stopped),
        Duration::from_secs(5),
    );

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::StopLocalServer)
        .unwrap();
    // Give the worker time to process it
    thread::sleep(Duration::from_millis(300));

    assert!(handles.running.load(Ordering::SeqCst));

    kill(&handles);
}

#[test]
fn stop_event_reports_stopped() {
    let (port, _mock) = spawn_mock_server();
    let handles = start_worker();
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SwitchToRemoteServer(format!(
            "localhost:{port}"
        )))
        .unwrap();
    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Started),
        Duration::from_secs(10)
    )
    .is_some());

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::Stop)
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Stopped),
            Duration::from_secs(5)
        )
        .is_some(),
        "Stop should report Stopped"
    );

    kill(&handles);
}

#[test]
fn start_failures_latch_to_failed_after_three() {
    // Occupy a port so the embedded server can never bind it: every Start
    // attempt fails fast (early exit) instead of waiting out the timeout.
    let _occupant = TcpListener::bind(("127.0.0.1", 0)).expect("bind occupant for failure test");
    let bad_port = _occupant.local_addr().unwrap().port();
    let handles = start_worker_with_timeout(Duration::from_secs(2));

    // Drain boot Stopped.
    let _ = wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Stopped),
        Duration::from_secs(5),
    );

    // Point the worker at the occupied port once (SetPort clears the budget,
    // so do it only once, then accumulate failures with Start alone).
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SetPort(bad_port))
        .unwrap();
    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Stopped),
        Duration::from_secs(5)
    )
    .is_some());

    for attempt in 1..=MAX_START_FAILURES {
        handles
            .event_sender
            .send(LanguageToolWorkerEvent::Start)
            .unwrap();
        if attempt < MAX_START_FAILURES {
            // Non-terminal failures return to Stopped (retryable).
            assert!(
                wait_for_status(
                    &handles,
                    |s| matches!(s, WorkerStatus::Stopped),
                    Duration::from_secs(15)
                )
                .is_some(),
                "attempt {attempt} should return to Stopped"
            );
            // Give the worker a beat to settle before the next Start so we
            // don't send Start while still Starting (which is ignored).
            thread::sleep(Duration::from_millis(200));
        }
    }

    // After 3 consecutive failures the worker must latch into Failed.
    let failed = wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Failed(_)),
        Duration::from_secs(30),
    );
    assert!(
        failed.is_some(),
        "worker should latch into Failed after 3 start failures"
    );

    // Further Start is ignored while Failed.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::Start)
        .unwrap();
    thread::sleep(Duration::from_millis(500));
    assert!(handles.running.load(Ordering::SeqCst));

    // Stop clears the latch back to Stopped.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::Stop)
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Stopped),
            Duration::from_secs(5)
        )
        .is_some(),
        "Stop should clear Failed back to Stopped"
    );

    kill(&handles);
}

#[test]
fn setport_while_started_restarts() {
    let (port_a, _mock_a) = spawn_mock_server();
    let (port_b, _mock_b) = spawn_mock_server();
    let handles = start_worker();

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SwitchToRemoteServer(format!(
            "localhost:{port_a}"
        )))
        .unwrap();
    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Started),
        Duration::from_secs(10)
    )
    .is_some());

    // Point the client at the second mock; suggestions must keep flowing.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SwitchToRemoteServer(format!(
            "localhost:{port_b}"
        )))
        .unwrap();
    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Started),
        Duration::from_secs(10)
    )
    .is_some());

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "hello again",
        )))
        .unwrap();
    assert!(
        recv_suggestion(&handles, Duration::from_secs(10)).is_some(),
        "suggestions should flow after address change"
    );

    kill(&handles);
}

#[test]
fn remote_outage_flips_status_and_recovers() {
    let (port, healthy, _mock) = spawn_toggleable_mock_server();
    let handles = start_worker();

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::SwitchToRemoteServer(format!(
            "localhost:{port}"
        )))
        .unwrap();
    assert!(wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::Started),
        Duration::from_secs(10)
    )
    .is_some());

    // A successful request keeps the status untouched.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "first text",
        )))
        .unwrap();
    assert!(recv_suggestion(&handles, Duration::from_secs(10)).is_some());

    // Server goes down: the next request must flip the status to
    // FailedToRespond (the outage becomes visible in the top bar).
    healthy.store(false, Ordering::SeqCst);
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "second text while down",
        )))
        .unwrap();
    let failed = wait_for_status(
        &handles,
        |s| matches!(s, WorkerStatus::FailedToRespond(_)),
        Duration::from_secs(10),
    );
    assert!(
        failed.is_some(),
        "a failing request while Started must flip to FailedToRespond"
    );
    let status = failed.unwrap();
    assert!(
        status.reason().contains("503"),
        "the failure reason should mention the HTTP error: {status:?}"
    );

    // Manual retry while the server is still down: the re-connect fails and
    // must land back in FailedToRespond (typing keeps retrying) instead of
    // dead-ending in Stopped.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::Retry)
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Connecting),
            Duration::from_secs(5)
        )
        .is_some(),
        "Retry in remote mode should first report Connecting"
    );
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::FailedToRespond(_)),
            Duration::from_secs(15)
        )
        .is_some(),
        "a failed reconnect attempt must return to FailedToRespond, not Stopped"
    );

    // Server comes back: the next successful request recovers to Started.
    healthy.store(true, Ordering::SeqCst);
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "third text while up",
        )))
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(10)
        )
        .is_some(),
        "a successful request must recover from FailedToRespond"
    );

    // Suggestions keep flowing after recovery.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "fourth text",
        )))
        .unwrap();
    assert!(recv_suggestion(&handles, Duration::from_secs(10)).is_some());

    kill(&handles);
}
