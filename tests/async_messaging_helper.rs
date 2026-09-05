use std::{net::TcpStream, sync::atomic::Ordering, thread, time::Duration};

use rhesis::interop::async_messaging_helper::AsyncMessagingHelperRust;

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

#[test]
fn starts_uninitialized() {
    let helper = AsyncMessagingHelperRust::default();

    assert!(helper.event_sender.is_none());
    assert!(helper.message_receiver.is_none());
    assert!(!helper.worker_running.load(Ordering::SeqCst));
    assert_eq!(helper.server_status, 0); // Stopped
}

#[test]
fn ensure_worker_running_starts_the_worker_once() {
    let mut helper = AsyncMessagingHelperRust::default();

    helper.ensure_worker_running();
    assert!(helper.event_sender.is_some());
    assert!(helper.message_receiver.is_some());
    assert!(helper.worker_running.load(Ordering::SeqCst));

    // Idempotent: a second call must not break anything
    helper.ensure_worker_running();
    assert!(helper.event_sender.is_some());
    assert!(helper.worker_running.load(Ordering::SeqCst));
}

#[test]
fn restart_without_embedded_keeps_worker_alive() {
    let mut helper = AsyncMessagingHelperRust::default();

    // Stop path: no embedded server is spawned, so no java needed
    helper.restart(false, "2689");

    assert!(helper.event_sender.is_some());
    assert!(helper.worker_running.load(Ordering::SeqCst));
    thread::sleep(Duration::from_millis(300));
    assert!(helper.worker_running.load(Ordering::SeqCst));
}

#[test]
fn control_methods_keep_worker_alive() {
    let mut helper = AsyncMessagingHelperRust::default();

    helper.stop_server();
    assert!(helper.worker_running.load(Ordering::SeqCst));

    helper.set_server_port("2690");
    assert!(helper.worker_running.load(Ordering::SeqCst));

    helper.retry_server();
    // Retry spawns a start attempt in the background (fast failure without
    // java/jar); the worker thread itself must stay alive.
    thread::sleep(Duration::from_millis(300));
    assert!(helper.worker_running.load(Ordering::SeqCst));

    helper.stop_server();
    thread::sleep(Duration::from_millis(300));
    assert!(helper.worker_running.load(Ordering::SeqCst));
}

#[test]
fn drop_sends_kill_and_worker_stops() {
    let running = {
        let mut helper = AsyncMessagingHelperRust::default();
        helper.ensure_worker_running();
        assert!(helper.worker_running.load(Ordering::SeqCst));
        helper.worker_running.clone()
    };

    // The clone is the flag the worker thread itself uses, so once it flips
    // we know the worker processed Kill (sent from Drop) and exited
    assert!(
        wait_for(|| !running.load(Ordering::SeqCst), Duration::from_secs(5)),
        "worker should stop after the helper is dropped"
    );
}

#[test]
fn helper_can_restart_a_dead_worker() {
    let mut helper = AsyncMessagingHelperRust::default();

    helper.ensure_worker_running();
    let old_sender = helper.event_sender.clone().unwrap();

    // Kill the worker behind the helper's back
    old_sender
        .send(rhesis::languagetool::service::LanguageToolWorkerEvent::Kill)
        .unwrap();
    let stopped = wait_for(
        || !helper.worker_running.load(Ordering::SeqCst),
        Duration::from_secs(5),
    );
    assert!(stopped, "worker should stop after Kill");

    // The helper notices the worker is gone and restarts it
    helper.ensure_worker_running();
    assert!(helper.worker_running.load(Ordering::SeqCst));
    assert!(helper.event_sender.is_some());
}

/// The embedded server needs a java runtime and a LanguageTool installation.
fn language_tool_available() -> bool {
    use std::path::Path;
    use std::process::Command;
    for dir in ["./build/LanguageTool", "./build/artifacts/LanguageTool"] {
        if Path::new(&format!("{dir}/languagetool-server.jar")).exists()
            && Path::new(&format!("{dir}/server.properties")).exists()
            && Command::new("java").arg("-version").output().is_ok()
        {
            return true;
        }
    }
    eprintln!("skipping: LanguageTool not found in ./build/LanguageTool or ./build/artifacts/LanguageTool");
    false
}

fn wait_for_server(port: u16, timeout: Duration) -> bool {
    wait_for(|| TcpStream::connect(("localhost", port)).is_ok(), timeout)
}

fn wait_for_port_free(port: u16, timeout: Duration) -> bool {
    wait_for(|| TcpStream::connect(("localhost", port)).is_err(), timeout)
}

/// Dropping the helper must SIGKILL the running server (cross-platform
/// `Child::kill`) and join the worker, so the port is free the moment drop
/// returns — no orphaned server blocking the next start.
#[test]
fn drop_kills_running_server_and_frees_port() {
    if !language_tool_available() {
        return;
    }
    let port = 26892;

    let mut helper = AsyncMessagingHelperRust::default();
    helper.restart(true, &port.to_string());
    assert!(
        wait_for_server(port, Duration::from_secs(60)),
        "embedded server should start"
    );

    drop(helper);

    assert!(
        wait_for_port_free(port, Duration::from_secs(10)),
        "server port must be free right after drop (joined worker kills java)"
    );
}

/// Killing the app mid-startup must not leave a server behind either: the
/// worker is joined on drop, killing a partially started child on its way out.
#[test]
fn drop_during_startup_leaves_no_server() {
    if !language_tool_available() {
        return;
    }
    let port = 26893;

    let mut helper = AsyncMessagingHelperRust::default();
    helper.restart(true, &port.to_string());
    drop(helper);

    // Past any spawn attempt: either java never started, or terminate_child
    // already killed it during the joined shutdown.
    thread::sleep(Duration::from_secs(5));
    assert!(
        wait_for_port_free(port, Duration::from_secs(10)),
        "no server may remain after dropping a starting helper"
    );
}
