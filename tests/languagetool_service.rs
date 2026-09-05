use std::{
    io::{Read, Write},
    net::TcpListener,
    net::TcpStream,
    path::Path,
    process::Command,
    sync::atomic::Ordering,
    thread,
    time::Duration,
};

use crossbeam::channel::RecvTimeoutError;
use cxx_qt_lib::QString;
use rhesis::languagetool::service::{
    LanguageToolWorker, LanguageToolWorkerEvent, Message, Suggestion, WorkerHandles, WorkerStatus,
    MAX_START_FAILURES,
};

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

fn wait_for_server(port: u16, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if TcpStream::connect(("localhost", port)).is_ok() {
            return true;
        }
        thread::sleep(Duration::from_millis(300));
    }
    false
}

/// The embedded server needs a java runtime and a LanguageTool installation
/// (jar plus server.properties, without which the JVM exits immediately).
/// Accepts both the setup.sh layout and the build-common.sh artifacts layout.
fn language_tool_available() -> bool {
    for dir in ["./build/LanguageTool", "./build/artifacts/LanguageTool"] {
        if Path::new(&format!("{dir}/languagetool-server.jar")).exists()
            && Path::new(&format!("{dir}/server.properties")).exists()
        {
            if Command::new("java").arg("-version").output().is_ok() {
                return true;
            }
            eprintln!("skipping: java not found");
            return false;
        }
    }
    eprintln!("skipping: LanguageTool not found in ./build/LanguageTool or ./build/artifacts/LanguageTool");
    false
}

fn start_worker() -> WorkerHandles {
    LanguageToolWorker::default()
        .with_startup_timeout(Duration::from_secs(2))
        .start()
}

fn start_worker_with_timeout(timeout: Duration) -> WorkerHandles {
    LanguageToolWorker::default()
        .with_startup_timeout(timeout)
        .start()
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
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock server");
    let port = listener.local_addr().unwrap().port();
    let handle = thread::spawn(move || {
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
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
    (port, handle)
}

fn kill(handles: &WorkerHandles) {
    let _ = handles
        .event_sender
        .send(LanguageToolWorkerEvent::Kill);
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

    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Stopped),
            Duration::from_secs(5)
        )
        .is_some()
    );

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
        .send(LanguageToolWorkerEvent::ChangeAddress(
            "localhost".to_string(),
            port,
        ))
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
        .send(LanguageToolWorkerEvent::ChangeAddress(
            "localhost".to_string(),
            port,
        ))
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(10)
        )
        .is_some()
    );

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
        .send(LanguageToolWorkerEvent::ChangeAddress(
            "localhost".to_string(),
            port,
        ))
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(10)
        )
        .is_some()
    );

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
        .send(LanguageToolWorkerEvent::ChangeAddress(
            "localhost".to_string(),
            port,
        ))
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(10)
        )
        .is_some()
    );

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
    let _occupant =
        TcpListener::bind(("127.0.0.1", 0)).expect("bind occupant for failure test");
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
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Stopped),
            Duration::from_secs(5)
        )
        .is_some()
    );

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
        .send(LanguageToolWorkerEvent::ChangeAddress(
            "localhost".to_string(),
            port_a,
        ))
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(10)
        )
        .is_some()
    );

    // Point the client at the second mock; suggestions must keep flowing.
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::ChangeAddress(
            "localhost".to_string(),
            port_b,
        ))
        .unwrap();
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(10)
        )
        .is_some()
    );

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
fn embedded_server_delivers_recommendations() {
    if !language_tool_available() {
        return;
    }

    let port = 26890;
    let handles = LanguageToolWorker::default().start();

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::RestartLocalServer(port))
        .unwrap();

    assert!(
        wait_for_server(port, Duration::from_secs(60)),
        "LanguageTool server did not come up on port {port}"
    );
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(60)
        )
        .is_some(),
        "worker should report Started"
    );
    thread::sleep(Duration::from_secs(2));

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::UpdateSuggestions(QString::from(
            "This is a test with a misspelled word teh test.",
        )))
        .unwrap();

    match recv_suggestion(&handles, Duration::from_secs(30)) {
        Some(Suggestion(recommendations)) => {
            assert!(
                !recommendations.is_empty(),
                "expected recommendations from the embedded server"
            );
        }
        None => panic!("expected suggestions, got timeout"),
    }

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::StopLocalServer)
        .unwrap();
    kill(&handles);
}

#[test]
fn restarting_local_server_respawns_it() {
    if !language_tool_available() {
        return;
    }

    let port = 26891;
    let handles = LanguageToolWorker::default().start();

    handles
        .event_sender
        .send(LanguageToolWorkerEvent::RestartLocalServer(port))
        .unwrap();
    assert!(wait_for_server(port, Duration::from_secs(60)));
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(60)
        )
        .is_some()
    );

    // A second restart must be harmless (old process killed, new one started)
    handles
        .event_sender
        .send(LanguageToolWorkerEvent::RestartLocalServer(port))
        .unwrap();
    assert!(wait_for_server(port, Duration::from_secs(60)));
    assert!(
        wait_for_status(
            &handles,
            |s| matches!(s, WorkerStatus::Started),
            Duration::from_secs(60)
        )
        .is_some()
    );

    kill(&handles);
    assert!(wait_for(
        || !handles.running.load(Ordering::SeqCst),
        Duration::from_secs(5)
    ));
}
