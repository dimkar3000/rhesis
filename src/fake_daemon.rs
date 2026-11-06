//! Test-only daemon fixture: the real daemon's session semantics with the
//! worker replaced by a hard-coded protocol stub.
//!
//! Scope (Refactor.md §9): speaks the same wire — strict `Hello`
//! handshake, one busy-refused client slot, hand-shake replay
//! (`Settings` snapshot then the last status), the mirror-echo cycle for
//! `SetSettings`, and a recorded fixture for checks (the same values the
//! daemon-session tier asserts from a real store, see
//! `crates/rhesis-service/tests/unit/daemon_session.rs`). It has no worker,
//! no store file, no idle grace: everything lives in a few mutexes, so the
//! tiers (ui-boot, the Quick Test runner) can point their clients at this
//! instead of ever spawning java or touching a real profile.
//!
//! Where the real daemon differs on purpose:
//! - Shutdown ends the session but NOT this daemon: suites reuse it across
//!   test functions, and a client's `Drop` background-off rule (quitting
//!   without background hosting) would otherwise kill it mid-suite.
//! - No idle-grace: a fixture cannot strand itself, and tiers run as long
//!   as their suites do anyway.
//! - A second busy handshake replays the refusal from the shared thread —
//!   the exact frame bytes are what matters to the reconnect logic.
//!
//! Lives in `src/` because both consumers are lib users; the shipped
//! binary never touches this module. Qt-free, no QuickTest, offline.

use std::sync::{Arc, Mutex};

use interprocess::local_socket::traits::Listener as _;
use interprocess::local_socket::{
    GenericNamespaced, ListenerNonblockingMode, ListenerOptions, Stream, ToNsName as _,
};
use interprocess::TryClone as _;

use rhesis_protocol::constants::PROTOCOL;
use rhesis_protocol::{
    read_frame, write_frame, ClientMsg, DaemonMsg, Recommendation, Settings, WorkerStatus,
};

use crate::constants::*;

/// The settings the fixture boots with: §-conform defaults, background off
/// (a suite's `Drop` path never keeps this daemon running).
fn fixture_settings() -> Settings {
    Settings::default()
}

/// The fixture boots as if its embedded server were already serving (the
/// real daemon dispatches its store at boot and lands in `Started` for the
/// default snapshot); suites assert the connected steady state.
fn fixture_boot_status() -> WorkerStatus {
    WorkerStatus::Started
}

fn fixture_recommendations() -> Vec<Recommendation> {
    let grammar = |value: &str| Recommendation {
        range: rhesis_protocol::Range {
            start: 4,
            length: 4,
        },
        value: value.to_string(),
        // The fixture sets an empty color; the client resolves the
        // pipeline default. Daemon-session tier asserts #FF0000 after the
        // worker resolved it; the fixture daemon sends the resolved value.
        color: "#FF0000".to_string(),
        rule_id: "HE_LIKE".to_string(),
        category_id: "GRAMMAR".to_string(),
        tooltip: FIXTURE_GRAMMAR_TOOLTIP.to_string(),
        language: "EN-US".to_string(),
    };
    let spelling = Recommendation {
        range: rhesis_protocol::Range {
            start: 38,
            length: 6,
        },
        value: FIXTURE_QUICKLY.to_string(),
        color: "#FF0000".to_string(),
        rule_id: "MORFOLOGIK_RULE_EN_US".to_string(),
        category_id: "TYPOS".to_string(),
        tooltip: FIXTURE_SPELL_TOOLTIP.to_string(),
        language: "EN-US".to_string(),
    };
    vec![
        grammar(FIXTURE_LIKE_FIRST),
        grammar(FIXTURE_LIKE_SECOND),
        spelling,
    ]
}

/// Shared between the accept loop and the session thread: the mirrored
/// snapshot and the status the next replay uses.
struct FakeState {
    settings: Mutex<Settings>,
    status: Mutex<WorkerStatus>,
}

/// The one-client slot: `true` while a session thread owns the socket.
type Slot = Arc<Mutex<bool>>;

/// Start the fixture on a tier-unique socket name and publish it through
/// `RHESIS_SERVICE_SOCKET` (set before the first client reads the env, so
/// the caller must start this first). The pid suffix keeps parallel test
/// binaries from colliding on one name. Panics when the thread cannot
/// spawn; a bind failure prints and the tier fails on its own.
pub fn spawn() -> std::thread::JoinHandle<()> {
    let socket_name = rhesis_protocol::tier_socket_name("rhesis-fake-service");
    std::env::set_var("RHESIS_SERVICE_SOCKET", socket_name.clone());

    std::thread::Builder::new()
        .name("rhesis-fake-daemon".to_string())
        .spawn(move || run(&socket_name))
        .expect("spawn the fake daemon thread")
}

/// The fixture's whole life: bind, accept forever. Nothing here ever
/// returns — tiers share one process, so the daemon dies with the test
/// process; suites that "quit the service" only end a session.
fn run(socket_name: &str) {
    let Ok(name) = socket_name.to_ns_name::<GenericNamespaced>() else {
        eprintln!("fake daemon: invalid socket name {socket_name:?}");
        return;
    };
    let Ok(listener) = ListenerOptions::new()
        .name(name)
        .nonblocking(ListenerNonblockingMode::Accept)
        .create_sync()
    else {
        eprintln!("fake daemon: cannot bind {socket_name:?}");
        return;
    };

    let state = Arc::new(FakeState {
        settings: Mutex::new(fixture_settings()),
        status: Mutex::new(fixture_boot_status()),
    });
    let slot: Slot = Arc::new(Mutex::new(false));

    loop {
        match listener.accept() {
            Ok(stream) => {
                let occupied = *slot.lock().unwrap();
                if occupied {
                    // Match the real daemon's refusal: version frame with
                    // the busy flag, then close (the client sees either the
                    // frame or a reset — both mean "no session").
                    let mut refused = stream;
                    let _ = write_frame(
                        &mut refused,
                        &DaemonMsg::Hello {
                            protocol: PROTOCOL,
                            busy: true,
                        },
                    );
                    drop(refused);
                    continue;
                }
                *slot.lock().unwrap() = true;

                let session_state = state.clone();
                let session_slot = slot.clone();
                let Ok(handle) = std::thread::Builder::new()
                    .name("rhesis-fake-session".to_string())
                    .spawn(move || run_session(stream, session_state, session_slot))
                else {
                    *slot.lock().unwrap() = false;
                    continue;
                };
                drop(handle); // detached: the accept loop keeps serving
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(FAKE_ACCEPT_SLICE);
            }
            Err(_) => {
                eprintln!("fake daemon: listener failed");
                break;
            }
        }
    }
}

/// One session: strict handshake, replay, then the client's commands with
/// in-memory mirrors. Runs on its own thread, exactly like the real pump.
fn run_session(mut stream: Stream, state: Arc<FakeState>, slot: Slot) {
    // Reads live on their own clone so a blocking read never holds the
    // writer (nothing here shares the writer, but the layout mirrors the
    // real session for byte-for-byte far enough behavior).
    let mut read_side = match stream.try_clone() {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("fake daemon: cannot clone the session stream: {e:?}");
            *slot.lock().unwrap() = false;
            return;
        }
    };

    // --- strict handshake: the first frame must be a `Hello` ---------------
    let handshake = match read_frame::<ClientMsg>(&mut read_side) {
        Ok(Some(ClientMsg::Hello { protocol })) => {
            // A stale version is answered with OUR version so the client's
            // retire-and-respawn path has something to compare against,
            // then the session dies — the real daemon does the same.
            if protocol != PROTOCOL {
                eprintln!(
                    "fake daemon: client speaks protocol {protocol}, fixture speaks {PROTOCOL}"
                );
            }
            let _ = write_frame(
                &mut stream,
                &DaemonMsg::Hello {
                    protocol: PROTOCOL,
                    busy: false,
                },
            );
            protocol == PROTOCOL
        }
        Ok(Some(other)) => {
            eprintln!("fake daemon: first frame {other:?}, not a Hello; closing");
            false
        }
        Ok(None) => false,
        Err(e) => {
            eprintln!("fake daemon: handshake failed: {e}");
            false
        }
    };

    if handshake {
        serve(&mut read_side, &mut stream, &state);
    }

    // --- teardown (every path) ---------------------------------------------
    // The fixture itself outlives every session (see run): a Shutdown only
    // drops this socket.
    *slot.lock().unwrap() = false;
}

/// Post-handshake conversation. Ends when the client goes (socket EOF or
/// frame failure), asks for a daemon shutdown (session end, per run), or
/// sends protocol garbage.
fn serve(read_side: &mut Stream, write_side: &mut Stream, state: &Arc<FakeState>) {
    // The fixture speaks first: snapshot, then status (locked order §1).
    let stored = state.settings.lock().unwrap().clone();
    if write_frame(write_side, &DaemonMsg::Settings(stored)).is_err() {
        return;
    }
    let status = state.status.lock().unwrap().clone();
    if write_frame(write_side, &DaemonMsg::Status(status)).is_err() {
        return;
    }

    loop {
        let message = match read_frame::<ClientMsg>(read_side) {
            Ok(Some(message)) => message,
            Ok(None) => break,
            Err(e) => {
                eprintln!("fake daemon: client frame failed: {e}");
                break;
            }
        };
        match message {
            ClientMsg::Check { text } => {
                // The fake worker is instant: suggestion follows now.
                let suggestions = if text == FIXTURE_TEXT {
                    fixture_recommendations()
                } else {
                    Vec::new()
                };
                if write_frame(write_side, &DaemonMsg::Suggestion(suggestions)).is_err() {
                    break;
                }
            }
            ClientMsg::Start => {
                *state.status.lock().unwrap() = WorkerStatus::Started;
                if write_frame(write_side, &DaemonMsg::Status(WorkerStatus::Started)).is_err() {
                    break;
                }
            }
            ClientMsg::Stop => {
                *state.status.lock().unwrap() = WorkerStatus::Stopped;
                if write_frame(write_side, &DaemonMsg::Status(WorkerStatus::Stopped)).is_err() {
                    break;
                }
            }
            ClientMsg::Retry => {
                // The budget-less fixture simply reports running again.
                *state.status.lock().unwrap() = WorkerStatus::Started;
                if write_frame(write_side, &DaemonMsg::Status(WorkerStatus::Started)).is_err() {
                    break;
                }
            }
            ClientMsg::SetPort(port) => {
                let mut snapshot = state.settings.lock().unwrap().clone();
                snapshot.port = port;
                if !mirror_and_reply(write_side, state, snapshot) {
                    break;
                }
            }
            ClientMsg::SwitchToRemote(address) => {
                let mut snapshot = state.settings.lock().unwrap().clone();
                snapshot.embedded = false;
                snapshot.external_address = address.trim().to_string();
                if !mirror_and_reply(write_side, state, snapshot) {
                    break;
                }
            }
            ClientMsg::SetSettings(new_settings) => {
                // Locked order (§1) minus the store write: mirror, and the
                // "worker events" the real daemon derives reduce to the
                // status the replay will show next session.
                let embedded = new_settings.embedded;
                if !mirror_and_reply(write_side, state, new_settings.clone()) {
                    break;
                }
                let status = if embedded {
                    WorkerStatus::Started
                } else {
                    WorkerStatus::Connecting
                };
                *state.status.lock().unwrap() = status.clone();
                // The daemon's reconcile would report the transition only
                // when something actually changed; the fixture needs no
                // such bookkeeping (status replays are the next session's
                // problem, everything here answers synchronously).
                if write_frame(write_side, &DaemonMsg::Status(status)).is_err() {
                    break;
                }
            }
            ClientMsg::Shutdown => break,
            ClientMsg::Hello { .. } => {
                eprintln!("fake daemon: second Hello on an established session; closing");
                break;
            }
        }
    }
}

/// Mirror the snapshot at the client (the locked §1 order, store step
/// omitted) and remember it as the state's settings.
fn mirror_and_reply(write_side: &mut Stream, state: &Arc<FakeState>, snapshot: Settings) -> bool {
    if write_frame(write_side, &DaemonMsg::Settings(snapshot.clone())).is_err() {
        return false;
    }
    *state.settings.lock().unwrap() = snapshot;
    true
}
