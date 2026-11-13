//! The client's single socket-aware module (Refactor.md §5).
//!
//! Everything that talks to the daemon lives here: the boot-time probe,
//! the detached spawning of a same-install daemon, the strict `Connect`
//! handshake with its retry budget, the persistent connection loop
//! (reconnect with backoff) and the outbound command path. Every other
//! module in the Qt tier is wire-blind: the connection loop delivers
//! [`DaemonMsg`] frames (plus the client-local session-lost event) into a
//! channel, and UI commands come back in through the command channel.
//!
//! Socket name and spawning share one tier seam: `RHESIS_SERVICE_SOCKET`
//! overrides [`SERVICE_SOCKET_NAME`] on both ends (the daemon resolves it
//! in `DaemonOptions::production`), so test tiers can run an in-process
//! daemon beside the client without ever touching the production socket.
//! With the connection unreachable, the loop respawns the same-install
//! daemon binary at a limited rate; it never spawns when the socket name
//! came from the environment (a tier harness owns its own daemon).

use std::io::{self, ErrorKind};
use std::time::{Duration, Instant};

use crossbeam::channel::{unbounded, Receiver, Sender};
use interprocess::local_socket::traits::Stream as _;
use interprocess::local_socket::{GenericNamespaced, Stream, ToNsName as _};
use interprocess::TryClone as _;

use rhesis_constants::{PROTOCOL, SERVICE_SOCKET_NAME};
use rhesis_protocol::daemon_msg::DaemonMsg;
use rhesis_protocol::frame_error::FrameError;
use rhesis_protocol::{read_frame, write_frame};
use rhesis_settings::settings::Settings;

use crate::constants::*;

/// Frames delivered from the daemon, plus the client-local session-lost
/// event. This is the fan-out's whole input contract.
pub enum ConnectionEvent {
    Wire(DaemonMsg),
    /// The session ended (daemon gone, stale version, frame garbage) and
    /// no new one is up yet: the UI reports `Disconnected` (code 6) until
    /// the reconnect replay arrives.
    Lost,
}

/// The client's request vocabulary, one per helper invokable; the
/// connection loop serializes it onto the socket. The daemon parses these
/// frames through its own mirror enum (same variants, same order —
/// postcard tags by index), so a shape change here is a `PROTOCOL` bump
/// mirrored on the daemon side.
#[derive(Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ClientMsg {
    /// Sent first; the daemon answers `DaemonMsg::Connect` with its own
    /// version + busy flag. Mismatch → the client tears down and respawns.
    Connect { protocol: u32 },
    /// Check this text; answers arrive as `DaemonMsg::Suggestion` batches
    /// (worker debounce applies; checks while down are dropped silently).
    Check { text: String },
    /// Start the embedded LanguageTool server.
    Start,
    /// Stop the embedded LanguageTool server.
    Stop,
    /// Clear the failure budget and try the current server again.
    Retry,
    /// Switch to an external LanguageTool at this base URL.
    SwitchToRemote(String),
    /// Apply a full settings snapshot (persisted, then mirrored back).
    SetSettings(Settings),
    /// Quit the daemon (tray "Quit service" or the final app quit while
    /// background hosting is off).
    Shutdown,
}

/// The handy `Debug` hides long check texts (documents sent per keystroke).
impl std::fmt::Debug for ClientMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientMsg::Connect { protocol } => write!(f, "Connect({protocol})"),
            ClientMsg::Check { text } => {
                write!(f, "Check({})", text.chars().take(24).collect::<String>())
            }
            ClientMsg::Start => write!(f, "Start"),
            ClientMsg::Stop => write!(f, "Stop"),
            ClientMsg::Retry => write!(f, "Retry"),
            ClientMsg::SwitchToRemote(address) => write!(f, "SwitchToRemote({address})"),
            ClientMsg::SetSettings(settings) => write!(f, "SetSettings({settings:?})"),
            ClientMsg::Shutdown => write!(f, "Shutdown"),
        }
    }
}

/// Handle the UI holds: what invokedables enqueue is what the loop sends.
/// Dropping the handle does not shut the daemon down (the mirror's
/// background-off `Drop` decides that, sending [`ClientMsg::Shutdown`]).
pub struct Connection {
    commands: Sender<ClientMsg>,
}

impl Connection {
    /// Start the connection loop. `events` receives every daemon frame and
    /// every session-lost event until the process ends; the loop thread
    /// owns them afterwards.
    pub fn start(events: Sender<ConnectionEvent>) -> Connection {
        let (commands_tx, commands_rx) = unbounded::<ClientMsg>();
        // A thread-name/style note: this loop is the only thread that ever
        // touches the socket, which is what the §5 "one wire module" claim
        // is about.
        match std::thread::Builder::new()
            .name("rhesis-connection".to_string())
            .spawn(move || run_loop(events, commands_rx))
        {
            Ok(_) => Connection {
                commands: commands_tx,
            },
            // Degrade to a disconnected UI: the mirror keeps its defaults
            // and the command channel just buffers (dropped on drop).
            Err(e) => {
                log::error!("connection loop spawn failed; running disconnected: {e:?}");
                Connection {
                    commands: commands_tx,
                }
            }
        }
    }

    /// Enqueue a command. Fire-and-forget: the wire never acknowledged
    /// commands anyway (§3) — the next mirrored snapshot or status frame is
    /// the confirmation. A dead loop is logged; the buffer is unbounded, so
    /// bursts of autosave checks can never block a Qt thread.
    pub fn send(&self, message: ClientMsg) {
        log::trace!("request {message:?}");
        if let Err(e) = self.commands.send(message) {
            log::warn!("command dropped, the connection loop is gone: {e:?}");
        }
    }
}

/// Boot-time settings probe (§5 startup re-order): reach a daemon —
/// spawning the same-install one if none is up — and return its persisted
/// snapshot, so `run()` can pick language, icon theme and background
/// behavior BEFORE Qt builds anything. The single-client slot (§3) makes
/// this strictly sequential: the probe owns the name until it gives up,
/// then [`Connection::start`] takes over.
///
/// `None` boots on protocol defaults; the moment the steady-state loop
/// completes its first handshake, the replay corrects the mirror.
pub fn boot_snapshot() -> Option<Settings> {
    let socket = socket_name();
    let deadline = Instant::now() + BOOT_BUDGET;
    let mut spawned = false;
    while Instant::now() < deadline {
        match handshake(&socket) {
            Ok(HandshakeOutcome::Session(stream)) => {
                if let Some(settings) = read_replay(stream, BOOT_REPLAY_CAP) {
                    log::info!(
                        "boot snapshot acquired{}",
                        if spawned {
                            " from the spawned daemon"
                        } else {
                            ""
                        }
                    );
                    return Some(settings);
                }
                return None;
            }
            Ok(HandshakeOutcome::Busy) => {
                // Transient (the single-instance guard makes a real race
                // near-impossible): boot on defaults; the steady-state loop
                // replays once the other client leaves.
                log::debug!("boot probe found the daemon busy with another client");
                return None;
            }
            Ok(HandshakeOutcome::Stale(stream)) => retire_stale(stream),
            Err(_) => {}
        }
        if spawning_allowed() && !spawned {
            spawn_daemon();
            spawned = true;
        }
        std::thread::sleep(PROBE_SLICE);
    }
    log::warn!("no daemon answered the boot probe within {BOOT_BUDGET:?}; booting on defaults");
    None
}

/// Socket name: the production name, or the `RHESIS_SERVICE_SOCKET` tier
/// override (shared with the daemon's `DaemonOptions::production()`).
pub(crate) fn socket_name() -> String {
    std::env::var("RHESIS_SERVICE_SOCKET")
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| SERVICE_SOCKET_NAME.to_string())
}

/// Whether respawning the same-install daemon is allowed at all. The tier
/// seam pins its own in-process daemon, so a spawner would fight it over
/// the pinned socket name.
fn spawning_allowed() -> bool {
    std::env::var_os("RHESIS_SERVICE_SOCKET").is_none()
}

/// The same-install daemon binary beside this executable (Flatpak, AppImage
/// and tarball layouts keep both binaries in one `bin/` directory; a dev
/// `cargo build --workspace` puts them side by side in `build/target/*`).
fn daemon_binary() -> Option<std::path::PathBuf> {
    rhesis_platform::process::exe_dir().map(|dir| {
        let filename = if cfg!(windows) {
            "rhesis-service.exe"
        } else {
            "rhesis-service"
        };

        dir.join(filename)
    })
}

/// Spawn the same-install daemon binary detached with no arguments — the
/// daemon builds its own live provider from its settings; the client never
/// forwards flags (the fake-vs-real choice never crosses a process
/// boundary). Best-effort: a missing or failing binary is logged and
/// retried after [`SPAWN_INTERVAL`]; the UI boots into `Disconnected`
/// meanwhile.
fn spawn_daemon() {
    if !spawning_allowed() {
        return;
    }
    match daemon_binary() {
        Some(binary) => {
            if let Err(e) = rhesis_platform::process::spawn_detached(binary, &[]) {
                log::warn!("cannot respawn the Rhesis service daemon ({e})");
            }
        }
        None => log::warn!("cannot respawn the Rhesis service daemon: no executable directory"),
    }
}

/// Spawn only when the last attempt is old enough, so a daemon that dies
/// at bind doesn't turn the reconnect loop into an exec loop.
fn spawn_daemon_if_due(last_spawn: &mut Option<Instant>) {
    let due = match last_spawn {
        Some(at) => at.elapsed() >= SPAWN_INTERVAL,
        None => true,
    };
    if due {
        *last_spawn = Some(Instant::now());
        spawn_daemon();
    }
}

/// Backoff law of the reconnect loop (tested below): double and cap.
fn grow_backoff(wait: Duration) -> Duration {
    (wait * 2).min(RETRY_MAX)
}

/// What a handshake produced for the caller.
enum HandshakeOutcome {
    /// Ready session: the daemon answered `Connect { protocol, busy: false }`
    /// and will replay `Settings` + `Status` next.
    Session(Stream),
    /// A live, current daemon is busy with another client: never respawn.
    Busy,
    /// A live daemon speaking an older protocol — the caller retires it.
    Stale(Stream),
}

fn handshake(socket: &str) -> Result<HandshakeOutcome, io::Error> {
    let name = socket
        .to_ns_name::<GenericNamespaced>()
        .map_err(|e| io::Error::new(ErrorKind::InvalidInput, e.to_string()))?;
    let mut stream = Stream::connect(name)?;
    write_frame(&mut stream, &ClientMsg::Connect { protocol: PROTOCOL })
        .map_err(|e| io::Error::new(ErrorKind::BrokenPipe, format!("Connect write: {e}")))?;
    match read_frame::<DaemonMsg>(&mut stream) {
        Ok(Some(DaemonMsg::Connect {
            protocol: PROTOCOL,
            busy: false,
        })) => Ok(HandshakeOutcome::Session(stream)),
        Ok(Some(DaemonMsg::Connect { busy: true, .. })) => Ok(HandshakeOutcome::Busy),
        Ok(Some(DaemonMsg::Connect { .. })) => Ok(HandshakeOutcome::Stale(stream)),
        // A replay frame instead of the greeting: schema garbage or a
        // racing peer — no session.
        Ok(Some(other)) => Err(io::Error::new(
            ErrorKind::InvalidData,
            format!("unexpected handshake frame {other:?}"),
        )),
        // The daemon may close before answering (its last session's Exit
        // path); a reset this early is routinely observed by the daemon
        // tier — treat both as "no session".
        Ok(None) => Err(io::Error::new(
            ErrorKind::ConnectionReset,
            "daemon closed before answering",
        )),
        Err(e) => Err(frame_error_to_io(e)),
    }
}

/// Session-error mapping for the loop's log lines: socket failures keep
/// their io identity, protocol garbage becomes `InvalidData` (both just
/// mean "session ends, reconnect").
fn frame_error_to_io(e: FrameError) -> io::Error {
    match e {
        FrameError::Io(e) => e,
        other => io::Error::new(ErrorKind::InvalidData, other.to_string()),
    }
}

/// Retire a stale daemon: ask it to quit (it exits and frees the name) and
/// detach without reading an answer — the caller respawns this build's
/// binary on the normal cadence (§3: version mismatch = client wins).
fn retire_stale(mut stale: Stream) {
    let _ = write_frame(&mut stale, &ClientMsg::Shutdown);
    drop(stale);
}

/// The daemon's fixed replay order after the greeting (session.rs): the
/// settings snapshot first, then the last worker status (`Status` without
/// a preceding `Settings` is impossible — bail). Boot callers stop
/// consuming here and drop the session.
fn read_replay(mut stream: Stream, timeout: Duration) -> Option<Settings> {
    stream.set_recv_timeout(Some(timeout)).ok()?;
    loop {
        match read_frame::<DaemonMsg>(&mut stream) {
            Ok(Some(DaemonMsg::Settings(settings))) => return Some(settings),
            Ok(Some(DaemonMsg::Status { code, .. })) => {
                log::warn!("boot replay reached status code {code} without a settings snapshot");
                return None;
            }
            Ok(Some(msg)) => log::debug!("skipping unexpected boot frame {msg:?}"),
            Ok(None) | Err(_) => {
                log::warn!("daemon dropped during the boot replay");
                return None;
            }
        }
    }
}

/// How a session ended, reported to the loop.
enum SessionEnd {
    /// Normal teardown (socket error, reader exit): reconnect.
    Lost,
    /// The client asked the daemon to quit: loop ends, no reconnect.
    Shutdown,
    /// The event consumer is gone (process winding down): loop ends.
    Abandoned,
}

/// Pump one live session end-to-end: a reader thread feeds daemon frames
/// into the event channel while the main path forwards UI commands onto
/// the socket; the first failure tears the session down. This thread owns
/// the socket.
fn serve_session(
    mut stream: Stream,
    events: &Sender<ConnectionEvent>,
    commands: &Receiver<ClientMsg>,
    pending: &mut Option<Settings>,
) -> SessionEnd {
    let Ok(mut reader) = stream.try_clone() else {
        log::warn!("cannot split the session socket; reconnecting");
        return SessionEnd::Lost;
    };
    let (incoming_tx, incoming_rx) = unbounded::<DaemonMsg>();
    let (done_tx, done_rx) = unbounded::<()>();
    let reader_result = std::thread::Builder::new()
        .name("rhesis-session-read".to_string())
        .spawn(move || {
            loop {
                match read_frame::<DaemonMsg>(&mut reader) {
                    Ok(Some(msg)) => {
                        if incoming_tx.send(msg).is_err() {
                            break; // consumer gone: the process is ending
                        }
                    }
                    Ok(None) => break, // daemon closed between frames
                    Err(e) => {
                        log::debug!("session frame read failed: {e}");
                        break;
                    }
                }
            }
            drop(incoming_tx);
            let _ = done_tx.send(());
        });
    if reader_result.is_err() {
        log::warn!("session reader spawn failed; reconnecting");
        return SessionEnd::Lost;
    }

    // Buffered settings go out first. The daemon's own replay (Settings +
    // Status right after the greeting) is already queued in the reader, but
    // that only mirrors old state — Apply survives a reconnect race only
    // if the new snapshot lands right behind it.
    if let Some(settings) = pending.take() {
        if let Err(e) = write_frame(&mut stream, &ClientMsg::SetSettings(settings.clone())) {
            log::warn!("buffered settings write failed: {e}");
            *pending = Some(settings);
            return SessionEnd::Lost;
        }
    }

    loop {
        crossbeam::select! {
            recv(incoming_rx) -> frame => match frame {
                Ok(msg) => {
                    if events.send(ConnectionEvent::Wire(msg)).is_err() {
                        // Fan-out consumer dropped: process is winding down.
                        return SessionEnd::Abandoned;
                    }
                }
                // The reader hung up without signaling `done` (only possible
                // via a spawn edge case); treat as a normal loss.
                Err(_) => return SessionEnd::Lost,
            },
            recv(done_rx) -> _ => return SessionEnd::Lost,
            recv(commands) -> command => match command {
                Ok(ClientMsg::SetSettings(settings)) => {
                    // A newer snapshot always supersedes a buffered one.
                    *pending = None;
                    if let Err(e) =
                        write_frame(&mut stream, &ClientMsg::SetSettings(settings.clone()))
                    {
                        log::warn!("settings write failed: {e}");
                        *pending = Some(settings);
                        return SessionEnd::Lost;
                    }
                }
                Ok(ClientMsg::Shutdown) => {
                    // Best-effort: the daemon quits as soon as it reads the
                    // frame and closes; the socket close mirrors it back.
                    if let Err(e) = write_frame(&mut stream, &ClientMsg::Shutdown) {
                        log::warn!("shutdown write failed: {e}");
                    }
                    return SessionEnd::Shutdown;
                }
                Ok(message) => {
                    if let Err(e) = write_frame(&mut stream, &message) {
                        log::debug!("request write failed: {e}");
                        return SessionEnd::Lost;
                    }
                }
                Err(_) => {
                    // Last Connection handle dropped: process is winding down.
                    return SessionEnd::Abandoned;
                }
            },
        }
    }
}

/// Wait between reconnect attempts. `false` ends the loop: a queued
/// [`ClientMsg::Shutdown`] with the daemon unreachable, or the UI dropping
/// the connection handle.
fn wait_disconnected(
    commands: &Receiver<ClientMsg>,
    wait: Duration,
    pending: &mut Option<Settings>,
) -> bool {
    let alarm = crossbeam::channel::after(wait);
    loop {
        crossbeam::select! {
            recv(alarm) -> _ => return true,
            recv(commands) -> command => match command {
                Ok(ClientMsg::Shutdown) => {
                    // Nothing to tell the daemon (no session); honor by not
                    // reconnecting.
                    log::info!("service shutdown requested while disconnected");
                    return false;
                }
                Ok(ClientMsg::SetSettings(settings)) => {
                    // Keep the newest snapshot; it is delivered on the next
                    // session start.
                    log::debug!("settings buffered while disconnected");
                    *pending = Some(settings);
                }
                Ok(message) => log::trace!("dropping {message:?} while disconnected"),
                Err(_) => return false,
            },
        }
    }
}

/// The whole connection lifetime: connect → handshake → session → wait.
fn run_loop(events: Sender<ConnectionEvent>, commands: Receiver<ClientMsg>) {
    let socket = socket_name();
    let mut backoff = RETRY_CHUNK;
    let mut last_spawn: Option<Instant> = None;
    let mut pending: Option<Settings> = None;

    loop {
        match handshake(&socket) {
            Ok(HandshakeOutcome::Session(stream)) => {
                log::info!("service session established");
                backoff = RETRY_CHUNK;
                match serve_session(stream, &events, &commands, &mut pending) {
                    SessionEnd::Lost => {
                        log::info!("service session lost; reporting disconnect");
                        if events.send(ConnectionEvent::Lost).is_err() {
                            break;
                        }
                    }
                    SessionEnd::Shutdown => {
                        log::info!("daemon shutdown acknowledged; connection loop ends");
                        break;
                    }
                    SessionEnd::Abandoned => break,
                }
            }
            Ok(HandshakeOutcome::Busy) => {
                // A live daemon holds another client in the single slot:
                // wait for it to leave, never respawn.
                log::debug!("daemon busy with another client; waiting");
            }
            Ok(HandshakeOutcome::Stale(stream)) => {
                // Pre-upgrade daemon still running its own protocol: retire
                // it so the respawn below can put up this build's version.
                retire_stale(stream);
                spawn_daemon_if_due(&mut last_spawn);
            }
            Err(_) => spawn_daemon_if_due(&mut last_spawn),
        }
        if !wait_disconnected(&commands, backoff, &mut pending) {
            break;
        }
        backoff = grow_backoff(backoff);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_then_caps() {
        assert_eq!(grow_backoff(RETRY_CHUNK), Duration::from_millis(100));
        assert_eq!(
            grow_backoff(grow_backoff(RETRY_CHUNK)),
            Duration::from_millis(200)
        );
        // Cap reached from below and respected at the cap.
        assert_eq!(grow_backoff(RETRY_MAX / 2), RETRY_MAX);
        assert_eq!(grow_backoff(RETRY_MAX), RETRY_MAX);
    }

    /// Every request survives the framing it rides on, and the postcard
    /// tag (first payload byte after the length prefix) stays the
    /// declaration index: the daemon parses these frames through its own
    /// mirror enum, so this order is the shared schema — reordering or
    /// inserting a variant must happen on both sides and bump `PROTOCOL`.
    #[test]
    fn requests_round_trip_and_pin_the_mirror_tag_order() {
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
            assert_eq!(
                read_frame::<ClientMsg>(&mut wire.as_slice())
                    .expect("frame must decode")
                    .expect("a written frame must come back before EOF"),
                message,
                "round trip of {message:?}"
            );
        }
    }
}
