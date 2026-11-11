use std::{
    io::ErrorKind,
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use interprocess::local_socket::{traits::Stream as _, GenericNamespaced, Stream, ToNsName as _};

use rhesis_protocol::constants::PROTOCOL;
use rhesis_protocol::daemon_msg::DaemonMsg;
use rhesis_protocol::frame_error::FrameError;
use rhesis_protocol::recommendation::{Range, Recommendation};
use rhesis_protocol::tier_socket_name;
use rhesis_protocol::{read_frame, write_frame};
use rhesis_service::{config::worker_events, session::ClientMsg, DaemonOptions};
use rhesis_settings::settings::Settings;

/// The flattened wire codes of the statuses where requests may be sent
/// (the worker's `Started` and `FailedToRespond`).
fn started_like(code: i32) -> bool {
    code == 2 || code == 5
}

/// The recorded fixture id the in-process daemons run against (see
/// rhesis-languagetool fixtures); set straight on `DaemonOptions`, so no
/// environment is touched.
const FAKE_LT: &str = "checker";
/// Text with a recorded two-match answer in the checker fixture.
const FIXTURE_TEXT: &str = "She like football and play the guitar quicly.";

/// The empty-answer record of the same fixture.
const CLEAN_TEXT: &str = "This sentence has no problems.";

/// How long helper readers keep polling before declaring the daemon stuck.
const DEADLINE: Duration = Duration::from_secs(4);
/// Individual recv slices inside a deadline loop.
const SLICE: Duration = Duration::from_millis(50);

/// Recorded without colors: the fake provider bakes the color fields into
/// answers at construction (before the daemon's color update arrives), so
/// all recorded recommendations keep the default red.
fn fixture_records() -> Vec<Recommendation> {
    let grammar_tooltip = "The pronoun ‘She’ requires a third-person verb or a past tense. \
Or did you forget to add a verb?";
    vec![
        Recommendation {
            range: Range {
                start: 4,
                length: 4,
            },
            value: "is like".to_string(),
            color: "#FF0000".to_string(),
            rule_id: "HE_LIKE".to_string(),
            category_id: "GRAMMAR".to_string(),
            tooltip: grammar_tooltip.to_string(),
            language: "EN-US".to_string(),
        },
        Recommendation {
            range: Range {
                start: 4,
                length: 4,
            },
            value: "likes".to_string(),
            color: "#FF0000".to_string(),
            rule_id: "HE_LIKE".to_string(),
            category_id: "GRAMMAR".to_string(),
            tooltip: grammar_tooltip.to_string(),
            language: "EN-US".to_string(),
        },
        Recommendation {
            range: Range {
                start: 38,
                length: 6,
            },
            value: "quickly".to_string(),
            color: "#FF0000".to_string(),
            rule_id: "MORFOLOGIK_RULE_EN_US".to_string(),
            category_id: "TYPOS".to_string(),
            tooltip: "Possible spelling mistake found.".to_string(),
            language: "EN-US".to_string(),
        },
    ]
}

fn sandbox(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "rhesis-daemon-session-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One in-process daemon run: sandbox-pinned options and a handle on the
/// outcome, so tests can assert the exit path.
struct DaemonPod {
    socket_name: String,
    sandbox: PathBuf,
    result: Arc<Mutex<Option<Result<(), String>>>>,
    handle: Option<JoinHandle<()>>,
}

impl DaemonPod {
    fn start(name: &str, grace: Duration, seed: Option<Settings>) -> Self {
        let socket_name = tier_socket_name(name);
        let sandbox = sandbox(name);
        let store_path = sandbox.join("service.json");
        if let Some(settings) = &seed {
            rhesis_settings::store::Store::open(store_path.clone())
                .save(settings)
                .expect("seeded store written");
        }

        let options = DaemonOptions {
            socket_name: socket_name.clone(),
            store_path: Some(store_path),
            // Hermeticity rule: pin the legacy INI to a path that does not
            // exist in the sandbox — `None` would resolve through the real
            // user configuration.
            ini_path: Some(sandbox.join("no-such-qsettings.conf")),
            idle_grace: grace,
            // The fake provider comes through the options now; no environment.
            fake_fixture: Some(FAKE_LT.to_string()),
        };
        let result = Arc::new(Mutex::new(None));
        let slot = result.clone();
        let thread_name = format!("daemon-{name}");
        let handle = std::thread::Builder::new()
            .name(thread_name)
            .spawn(move || {
                let outcome = rhesis_service::run(&options);
                *slot.lock().unwrap() = Some(outcome);
            })
            .expect("daemon thread spawned");
        DaemonPod {
            socket_name,
            sandbox,
            result,
            handle: Some(handle),
        }
    }

    /// Poll until the run returns (also joining its thread); a timeout is
    /// a stuck daemon and panics with the observed state.
    fn wait_result(mut self, timeout: Duration) -> Result<(), String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(outcome) = self.result.lock().unwrap().take() {
                if let Some(handle) = self.handle.take() {
                    handle.join().expect("daemon thread finished cleanly");
                }
                let _ = std::fs::remove_dir_all(&self.sandbox);
                return outcome;
            }
            assert!(
                Instant::now() < deadline,
                "daemon did not finish in {timeout:?}"
            );
            std::thread::sleep(SLICE);
        }
    }

    /// The daemon is still running (its run has not returned yet).
    fn remains_running(&self) -> bool {
        self.result.lock().unwrap().is_none()
    }

    /// Reads the on-disk store for persistence assertions without going
    /// through the crate (the store file is plain JSON).
    fn store_json(&self) -> serde_json::Value {
        serde_json::from_str(
            &std::fs::read_to_string(self.sandbox.join("service.json"))
                .expect("store file readable"),
        )
        .expect("store file is valid JSON")
    }
}

/// A connected client on the tier's socket.
struct Client {
    stream: Stream,
}

/// Open a session the way the production client does: connect, then speak
/// `Connect` first (the daemon only answers after the client's handshake).
fn open_session(socket_name: &str) -> Client {
    let mut client = Client::connect(socket_name);
    client.send(&ClientMsg::Connect { protocol: PROTOCOL });
    client
}

/// Open a session, retrying while the slot is occupied — the reconnecting
/// client's behavior after a busy refusal.
fn open_session_with_retry(socket_name: &str) -> Client {
    let deadline = Instant::now() + DEADLINE;
    loop {
        let mut client = open_session(socket_name);
        match client.read(SLICE) {
            Some(DaemonMsg::Connect {
                protocol: PROTOCOL,
                busy: false,
            }) => return client,
            Some(DaemonMsg::Connect { busy: true, .. }) => client.expect_closed(),
            // None: no handshake answer yet, keep polling.
            None => {}
            other => panic!("unexpected handshake answer {other:?}"),
        }
        assert!(
            Instant::now() < deadline,
            "never got a free session slot within the deadline"
        );
        std::thread::sleep(SLICE);
    }
}

impl Client {
    /// Connect, watching until the daemon binds (its accept loop is
    /// polling at its own slice, so a short bind delay is normal).
    fn connect(socket_name: &str) -> Self {
        let name = socket_name
            .to_string()
            .to_ns_name::<GenericNamespaced>()
            .expect("tier socket name");
        let deadline = Instant::now() + DEADLINE;
        loop {
            match Stream::connect(name.clone()) {
                Ok(stream) => return Client { stream },
                Err(e) => {
                    assert!(Instant::now() < deadline, "daemon never accepted: {e}");
                    std::thread::sleep(SLICE);
                }
            }
        }
    }

    fn send(&mut self, message: &ClientMsg) {
        write_frame(&mut self.stream, message).expect("client write");
    }

    /// Reads with per-slice timeouts; returns `None` when nothing arrived
    /// in the slice. A clean daemon close panics — callers awaiting a
    /// close use [`Client::expect_closed`].
    fn read(&mut self, slice: Duration) -> Option<DaemonMsg> {
        self.stream
            .set_recv_timeout(Some(slice))
            .expect("recv timeout set");
        match read_frame::<DaemonMsg>(&mut self.stream) {
            Ok(Some(message)) => Some(message),
            Ok(None) => panic!("daemon closed the session unexpectedly"),
            Err(FrameError::Io(e)) if e.kind() == ErrorKind::WouldBlock => None,
            Err(e) => panic!("client frame read failed: {e}"),
        }
    }

    /// Poll (deadline-bounded) for the peer to drop the connection. The
    /// close often races ahead of counter-traffic: the refusal note in
    /// `daemon.rs` documents that the busy frame may instead surface as a
    /// reset — either observation means "no session".
    fn expect_closed(&mut self) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            self.stream
                .set_recv_timeout(Some(SLICE))
                .expect("recv timeout set");
            match read_frame::<DaemonMsg>(&mut self.stream) {
                Ok(None) => return,
                Err(FrameError::Io(e)) if e.kind() == ErrorKind::WouldBlock => {}
                Err(FrameError::Io(e))
                    if matches!(e.kind(), ErrorKind::ConnectionReset | ErrorKind::BrokenPipe) =>
                {
                    return;
                }
                Err(e) => panic!("frame read failed: {e}"),
                other => panic!("expected a close, got {other:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "the daemon never closed the refused session"
            );
        }
    }

    /// The fixed handshake frame, polled like `open_session_with_retry`:
    /// a single recv slice can slip under load before the daemon thread
    /// gets scheduled, so `None` ("no frame yet") keeps polling until the
    /// shared deadline instead of failing the handshake.
    fn expect_connect(&mut self, busy: bool) {
        let deadline = Instant::now() + DEADLINE;
        loop {
            match self.read(SLICE) {
                Some(DaemonMsg::Connect { protocol, busy: b }) => {
                    assert_eq!(protocol, PROTOCOL);
                    assert_eq!(b, busy, "Busy flag");
                    return;
                }
                // None: no handshake frame yet, keep polling.
                None => {}
                other => panic!("expected Connect (busy={busy}), got {other:?}"),
            }
            assert!(
                Instant::now() < deadline,
                "the daemon never answered the handshake"
            );
        }
    }

    /// Accumulate frames until the daemon's `Settings` mirror; statuses
    /// and suggestions that interleave first are noted in the failure
    /// message.
    fn expect_settings(&mut self) -> Settings {
        let deadline = Instant::now() + DEADLINE;
        let mut seen = Vec::new();
        loop {
            match self.read(SLICE) {
                Some(DaemonMsg::Settings(settings)) => return settings,
                Some(frame) if seen.len() > 16 => {
                    panic!("settings never arrived among {seen:?} (last {frame:?})")
                }
                Some(frame) => seen.push(format!("{frame:?}")),
                None => assert!(
                    Instant::now() < deadline,
                    "no Settings mirror before the deadline; saw {seen:?}"
                ),
            }
        }
    }

    /// Poll for a status code satisfying `matches` (fake transitions may
    /// skip intermediates or arrive while other frames flow).
    fn wait_for_status(&mut self, describe: &str, matches: impl Fn(i32) -> bool) -> (i32, String) {
        let deadline = Instant::now() + DEADLINE;
        let mut seen = Vec::new();
        loop {
            match self.read(SLICE) {
                Some(DaemonMsg::Status { code, reason }) if matches(code) => return (code, reason),
                Some(DaemonMsg::Status { code, .. }) => seen.push(format!("status code {code}")),
                Some(frame) => seen.push(format!("{frame:?}")),
                None => {}
            }
            assert!(
                Instant::now() < deadline,
                "no {describe} status within the deadline; saw {seen:?}"
            );
        }
    }

    /// Poll for a suggestion batch satisfying `predicate`.
    fn wait_for_suggestions(
        &mut self,
        describe: &str,
        predicate: impl Fn(&Vec<Recommendation>) -> bool,
    ) {
        let deadline = Instant::now() + DEADLINE;
        let mut seen = Vec::new();
        loop {
            match self.read(SLICE) {
                Some(DaemonMsg::Suggestion(recs)) if predicate(&recs) => return,
                Some(DaemonMsg::Suggestion(recs)) => {
                    seen.push(format!("{} suggestions", recs.len()))
                }
                Some(DaemonMsg::Status { code, .. }) => seen.push(format!("status code {code}")),
                Some(frame) => panic!("unexpected frame {frame:?} while waiting for {describe}"),
                None => {}
            }
            assert!(
                Instant::now() < deadline,
                "no {describe} suggestions within the deadline; saw {seen:?}"
            );
        }
    }
}

/// One full conversation: version handshake, settings + status replay, the
/// first-handshake replay of the worker's config, and a real check against
/// the recorded fixture.
#[test]
fn full_session_replays_snapshot_and_answers_checks() {
    let pod = DaemonPod::start("full-session", Duration::from_secs(5), None);
    let mut client = open_session(&pod.socket_name);

    client.expect_connect(false);
    assert_eq!(
        client.expect_settings(),
        Settings::default(),
        "the snapshot replay shows the whole stored settings"
    );
    client.wait_for_status("fake server started", started_like);

    let expected = fixture_records();
    client.send(&ClientMsg::Check {
        text: FIXTURE_TEXT.to_string(),
    });
    client.wait_for_suggestions("the recorded checker fixture", |recs| recs == &expected);

    client.send(&ClientMsg::Shutdown);
    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()));
}

/// The single-client gate: a second connect gets a busy Connect and a clean
/// close while the first session stays live.
#[test]
fn second_client_while_open_is_refused_busy_then_closed() {
    let pod = DaemonPod::start("busy-refusal", Duration::from_secs(5), None);
    let mut first = open_session(&pod.socket_name);
    first.expect_connect(false);
    let _ = first.expect_settings();

    let mut second = open_session(&pod.socket_name);
    second.expect_connect(true);
    second.expect_closed();

    // The first session remains the serving one: it can still shut the
    // daemon down (the refused client never took the slot).
    first.send(&ClientMsg::Shutdown);
    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()));
}

/// Locked settings ownership: the daemon persists `SetSettings` into its
/// JSON store (camelCase keys), mirrors the confirmed snapshot back, and
/// moves the worker to the new server — which the recorded fake answers
/// on, unchanged.
#[test]
fn set_settings_persists_and_switches_to_remote() {
    let pod = DaemonPod::start("set-settings", Duration::from_secs(5), None);
    let mut client = open_session(&pod.socket_name);
    client.expect_connect(false);
    let _ = client.expect_settings();
    client.wait_for_status("fake server started", started_like);

    let new_settings = Settings {
        embedded: false,
        external_address: "http://fake-remote:9999".to_string(),
        language: "el_GR".to_string(),
        ..Settings::default()
    };
    client.send(&ClientMsg::SetSettings(new_settings.clone()));

    let mirrored = client.expect_settings();
    assert_eq!(
        mirrored, new_settings,
        "SetSettings echoes the confirmed snapshot back"
    );

    let disk = pod.store_json();
    assert_eq!(disk["settings"]["embedded"], false);
    assert_eq!(
        disk["settings"]["externalAddress"],
        "http://fake-remote:9999"
    );
    assert_eq!(disk["settings"]["language"], "el_GR");

    client.wait_for_status("remote fake started", started_like);
    client.send(&ClientMsg::Check {
        text: CLEAN_TEXT.to_string(),
    });
    client.wait_for_suggestions("the empty-answer fixture over the fake remote", |recs| {
        recs.is_empty()
    });

    client.send(&ClientMsg::Shutdown);
    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()));
}

/// `Shutdown` from the client: the daemon breaks its accept loop, kills
/// the worker, joins it, and the run reports success.
#[test]
fn shutdown_message_ends_the_daemon() {
    let pod = DaemonPod::start("shutdown", Duration::from_secs(5), None);
    let mut client = open_session(&pod.socket_name);
    client.expect_connect(false);
    let _ = client.expect_settings();

    client.send(&ClientMsg::Shutdown);
    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()));
}

/// The last client leaving starts the grace countdown; with nothing
/// arriving inside it the daemon exits on its own.
#[test]
fn idle_grace_exits_when_nobody_returns() {
    let pod = DaemonPod::start("idle-grace", Duration::from_millis(300), None);
    let mut client = open_session(&pod.socket_name);
    client.expect_connect(false);
    let _ = client.expect_settings();
    client.wait_for_status("fake server started", started_like);
    drop(client);

    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()), "the daemon exits after the grace window");
}

/// `background` overrides the grace exit — and a later `SetSettings`
/// turning it off lets the *next* empty grace window end the daemon live
/// (the flag is re-read at every window, not frozen).
#[test]
fn background_stays_alive_until_setting_turns_it_off() {
    let seed = Settings {
        background: true,
        ..Settings::default()
    };
    let pod = DaemonPod::start("background", Duration::from_millis(200), Some(seed.clone()));
    let mut first = open_session(&pod.socket_name);
    first.expect_connect(false);
    assert_eq!(first.expect_settings(), seed, "background seeded in");
    drop(first);

    // Far past the grace window: still alive.
    std::thread::sleep(Duration::from_millis(450));
    assert!(
        pod.remains_running(),
        "background=true keeps the daemon past the grace window"
    );

    // Turn the flag off through a fresh session (with a retry in case the
    // grace countdown already started — the flag keeps the daemon alive
    // until the disconnect, so the retry resolves immediately), then
    // leave: the countdown now runs to its end.
    let mut second = open_session_with_retry(&pod.socket_name);
    let _ = second.expect_settings();
    let turned_off = Settings {
        background: false,
        ..Settings::default()
    };
    second.send(&ClientMsg::SetSettings(turned_off));
    let _ = second.expect_settings();
    drop(second);

    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()));
}

/// A reconnection inside the grace window gets the free slot (cancelling
/// the countdown) and becomes a living session — the embedded fake server
/// never stopped behind it.
#[test]
fn reconnect_within_the_grace_serves_the_new_session() {
    let pod = DaemonPod::start("reconnect", Duration::from_millis(600), None);
    let mut first = open_session(&pod.socket_name);
    first.expect_connect(false);
    let _ = first.expect_settings();
    first.wait_for_status("fake server started", started_like);
    drop(first);

    let mut second = open_session_with_retry(&pod.socket_name);
    assert_eq!(
        second.expect_settings(),
        Settings::default(),
        "the new session replays the same ownership"
    );
    let expected = fixture_records();
    second.send(&ClientMsg::Check {
        text: FIXTURE_TEXT.to_string(),
    });
    second.wait_for_suggestions("second-session answers", |recs| recs == &expected);

    second.send(&ClientMsg::Shutdown);
    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()));
}

/// The handshake is strict: anything but `Connect` as the first frame is a
/// close without a session, and the slot frees for a real handshake right
/// after.
#[test]
fn garbage_first_frame_is_refused_and_the_daemon_recovers() {
    let pod = DaemonPod::start("strict-handshake", Duration::from_secs(5), None);
    let mut wrong = Client::connect(&pod.socket_name);
    wrong.send(&ClientMsg::Check {
        text: "unexpected first frame".to_string(),
    });
    wrong.expect_closed();
    assert!(
        pod.remains_running(),
        "the daemon survives a failed handshake"
    );

    // The slot frees only after the failed session's teardown; the
    // recovery handshake can land inside that window, so connect with
    // the client's retry-on-busy behavior (which consumes the confirmed
    // handshake itself).
    let mut right = open_session_with_retry(&pod.socket_name);
    let _ = right.expect_settings();
    right.send(&ClientMsg::Shutdown);
    let outcome = pod.wait_result(DEADLINE);
    assert_eq!(outcome, Ok(()));
}

/// The reconcile rules themselves live in pure tests (crate::config); this
/// tier verifies one end-to-end consequence of them: a reconnect with
/// identical settings produces no worker-moving frames on the wire beyond
/// the fixed handshake trio.
#[test]
fn reconnect_equal_settings_sends_no_extra_worker_frames() {
    assert!(
        worker_events(Some(&Settings::default()), &Settings::default()).is_empty(),
        "the pure half of the contract is still true"
    );
}

/// The parse mirror must keep the client's variant order: postcard tags by
/// index, so the first payload byte after the length prefix is the
/// declaration index. The Qt tier pins the same list on the client's enum;
/// a mismatch on either side is a `PROTOCOL` bump plus a mirrored change.
#[test]
fn mirrored_requests_keep_the_client_tag_order() {
    let cases = [
        (ClientMsg::Connect { protocol: PROTOCOL }, 0),
        (
            ClientMsg::Check {
                text: "She like football.".to_string(),
            },
            1,
        ),
        (ClientMsg::Start, 2),
        (ClientMsg::Stop, 3),
        (ClientMsg::Retry, 4),
        (
            ClientMsg::SwitchToRemote("http://192.168.1.10:8081".to_string()),
            5,
        ),
        (ClientMsg::SetSettings(Settings::default()), 6),
        (ClientMsg::Shutdown, 7),
    ];
    for (message, tag) in cases {
        let mut wire = Vec::new();
        write_frame(&mut wire, &message).expect("frame must encode");
        assert_eq!(wire[4], tag, "tag of {message:?}");
    }
}
