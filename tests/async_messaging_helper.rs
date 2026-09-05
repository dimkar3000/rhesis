use std::{sync::atomic::Ordering, thread, time::Duration};

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
