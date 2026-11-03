//! Wire schema for the Rhesis daemon ↔ client socket.
//!
//! Everything that crosses the wire lives here in a single definition both
//! ends compile against: the message enums, the shared payload types
//! (`Settings`, `WorkerStatus`, `Recommendation`) and the length-prefixed
//! framing. The transport (interprocess sockets) lives in the binaries; the
//! connection loop must only ever speak this module's shapes.
//!
//! Framing per Refactor.md §3: `[u32 LE length][postcard payload]`, with a
//! shared frame cap. postcard is non-self-describing, so the `Hello`
//! handshake carrying [`PROTOCOL`] is versioning load-bearing: a mismatch is
//! answered before any schema-sensitive data flows (the client shuts the
//! stale daemon down and respawns its own-install copy instead).
//!
//! Rules to keep the schema stable:
//! - any change in the types or enum shapes bumps [`PROTOCOL`] (starts at 1);
//! - every message variant has a round-trip test (the drift tripwire);
//! - plain enums only: variant discriminants on the wire, no `#[serde(tag)]`.

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Write};

use serde::{de::DeserializeOwned, Deserialize, Serialize};

/// Wire format version, sent first in both directions via the `Hello` frames.
/// A mismatch means the peers disagree about this module's types at compile
/// time (e.g. a pre-upgrade daemon still lingering in its idle grace).
pub const PROTOCOL: u32 = 1;

/// Socket the daemon binds: the abstract-namespace UDS name on Linux
/// (Flatpak ↔ host share the network namespace) and the same-named pipe on
/// Windows, resolved by the `interprocess` layer.
pub const SERVICE_SOCKET_NAME: &str = "io.github.dimkar3000.rhesis.service";

/// Maximum postcard payload length per frame, enforced on both sides before
/// any allocation or read. Ceiling chosen far above the biggest realistic
/// payload (a full text document + its suggestions) so only protocol abuse
/// or bugs ever hit it.
pub const FRAME_CAP: usize = 16 * 1024 * 1024;

/// Status code the *client* reports locally when no daemon session exists
/// (QML's `server_status`). Never encoded on the wire: the wire only ever
/// carries the six daemon-reported [`WorkerStatus`] states.
pub const CLIENT_STATUS_DISCONNECTED: i32 = 6;

/// Deterministic socket name for in-process test tiers (real daemon/session
/// code, tier-unique name). Production always uses [`SERVICE_SOCKET_NAME`];
/// the pid suffix keeps parallel test binaries from colliding on one name.
pub fn tier_socket_name(tier: &str) -> String {
    format!("{tier}-{}", std::process::id())
}

/// User-adjustable state the daemon owns: persisted by the settings store,
/// mirrored to the client, and override-able through `SetSettings`.
/// The store schema IS this struct (Refactor.md §6) — field-for-field.
/// JSON keys keep the QML-era names (`embedded`, `externalAddress`, ...)
/// so the migrated store diffs cleanly against the old INI; the wire is
/// position-driven postcard and ignores the naming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The embedded LanguageTool server (vs. an external instance).
    pub embedded: bool,
    /// Port for the embedded server (also used as the daemon's LT port).
    pub port: u16,
    /// Base URL of the external server when `embedded` is false.
    pub external_address: String,
    /// Host the LanguageTool server while no client is connected (the idle
    /// shutdown grace is skipped while this is on).
    pub background: bool,
    pub show_tooltips: bool,
    pub show_debug_tooltips: bool,
    /// Locale override (e.g. "el_GR"); empty means system locale.
    pub language: String,
    /// Color table for rules and rule categories (`CATEGORY:x`, `RULE:y`).
    /// BTree: deterministic wire bytes + JSON keys, stable diffing.
    pub color_settings: BTreeMap<String, String>,
}

impl Default for Settings {
    /// Same defaults the QML `Settings` block shipped pre-refactor.
    fn default() -> Self {
        let color_settings = BTreeMap::from([
            ("CATEGORY:GRAMMAR".to_string(), "#ecc224".to_string()),
            ("CATEGORY:TYPOGRAPHY".to_string(), "#f63ef9".to_string()),
            ("CATEGORY:TYPOS".to_string(), "#1ae918".to_string()),
        ]);
        Self {
            embedded: true,
            port: DEFAULT_PORT,
            external_address: String::new(),
            background: false,
            show_tooltips: true,
            show_debug_tooltips: true,
            language: String::new(),
            color_settings,
        }
    }
}

/// Factory port for the embedded LanguageTool server (also the Windows
/// installer's and the tray UI's fallback).
pub const DEFAULT_PORT: u16 = 2689;

/// Lifecycle of the LanguageTool server connection, as the worker reports
/// it. Moved verbatim from the worker crate: one definition for the wire,
/// the daemon's session pump and the client's status mirror.
///
/// QML status codes (see [`WorkerStatus::code`]): 0–5 here; the client-local
/// "no session" state maps to [`CLIENT_STATUS_DISCONNECTED`] (6) and never
/// appears on the wire.
#[derive(Debug, Clone, PartialEq, Default, Eq, Serialize, Deserialize)]
pub enum WorkerStatus {
    #[default]
    Stopped,
    /// Spawning/respawning; waiting for the embedded server to come up.
    Starting,
    /// Waiting for an external LanguageTool to answer (nothing is spawned).
    Connecting,
    Started,
    /// A connect/start attempt failed; `Stopped` retries are still possible
    /// while the failure budget is not exhausted (`MAX_START_FAILURES`).
    Failed(String),
    /// The server answered at least once but requests now fail; the next
    /// successful request automatically flips back to `Started`.
    FailedToRespond(String),
}

impl WorkerStatus {
    /// Numeric code exposed to QML via the status mirror property:
    /// 0 = Stopped, 1 = Starting, 2 = Started, 3 = Failed, 4 = Connecting,
    /// 5 = FailedToRespond (see the client for 6 = Disconnected).
    pub fn code(&self) -> i32 {
        match self {
            WorkerStatus::Stopped => 0,
            WorkerStatus::Starting => 1,
            WorkerStatus::Started => 2,
            WorkerStatus::Failed(_) => 3,
            WorkerStatus::Connecting => 4,
            WorkerStatus::FailedToRespond(_) => 5,
        }
    }

    /// Human-readable failure detail; empty unless the status is a failure.
    pub fn reason(&self) -> String {
        match self {
            WorkerStatus::Failed(r) => r.clone(),
            WorkerStatus::FailedToRespond(r) => r.clone(),
            _ => String::new(),
        }
    }

    /// `true` whenever requests may be sent: the server either answers
    /// (Started) or is expected to answer again on the next try.
    pub fn is_started(&self) -> bool {
        matches!(
            self,
            WorkerStatus::Started | WorkerStatus::FailedToRespond(_)
        )
    }
}

/// A text range to highlight plus the replacement and its metadata.
/// Colors come from the consumer's rule table (`color: ""` in fixtures
/// still means the default red, applied at rendering time). UI-free:
/// the Qt tier converts to its own types at the rendering boundary.
#[derive(Default, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recommendation {
    pub range: Range,
    pub value: String,
    pub color: String,
    pub rule_id: String,
    pub category_id: String,
    pub tooltip: String,
    pub language: String,
}

/// Half-open text range the recommendation applies to.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub start: i32,
    pub length: i32,
}

/// Client → daemon messages, in handshake order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// Sent first; the daemon answers `DaemonMsg::Hello` with its own
    /// version + busy flag. Mismatch → the client tears down and respawns.
    Hello { protocol: u32 },
    /// Check this text; answers arrive as `DaemonMsg::Suggestion` batches
    /// (worker debounce applies; `Checked` while down is dropped silently).
    Check { text: String },
    /// Start the embedded LanguageTool server (`Start` event verbatim).
    Start,
    /// Stop the embedded LanguageTool server.
    Stop,
    /// Change the embedded server's port (also stored by the daemon).
    SetPort(u16),
    /// Clear the failure budget and try the current server again.
    Retry,
    /// Switch to an external LanguageTool at this base URL.
    SwitchToRemote(String),
    /// Apply a full settings snapshot (persisted, then mirrored back).
    SetSettings(Settings),
    /// Quit the daemon (the client's "Quit service" / final quit).
    Shutdown,
}

/// Daemon → client messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DaemonMsg {
    /// Handshake answer: accepted version + whether another client session
    /// currently owns the single-client slot (§3: one session v1).
    Hello { protocol: u32, busy: bool },
    /// The daemon-side settings snapshot (pushed after `SetSettings`).
    Settings(Settings),
    /// Worker status transition (always broadcast as it happens).
    Status(WorkerStatus),
    /// Suggestion batch for the most recent accepted `Check`.
    Suggestion(Vec<Recommendation>),
}

/// Frame-level failure with enough context for a clean reconnect decision
/// (session teardown + `Disconnected`; the stream never resyncs, §3).
#[derive(Debug)]
pub enum FrameError {
    /// Socket/pipe failure unrelated to the data (keep the session).
    Io(std::io::Error),
    /// postcard failed to serialize (should be impossible for these enum
    /// shapes; treated as a bug, session ends).
    Encode(postcard::Error),
    /// postcard deserialization or shape mismatch (garbage/racing versions).
    Decode(postcard::Error),
    /// Header announced, or payload produced, more than [`FRAME_CAP`] bytes.
    TooLarge { length: usize },
    /// The stream ended mid-frame (peer died between bytes, not frames).
    Truncated,
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Io(e) => write!(f, "io error on the frame stream: {e}"),
            FrameError::Encode(e) => write!(f, "postcard encoding failed: {e}"),
            FrameError::Decode(e) => write!(f, "postcard decoding failed: {e}"),
            FrameError::TooLarge { length } => {
                write!(f, "frame of {length} bytes exceeds the cap")
            }
            FrameError::Truncated => write!(f, "frame stream ended mid-frame"),
        }
    }
}

impl std::error::Error for FrameError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            FrameError::Io(e) => Some(e),
            FrameError::Encode(e) | FrameError::Decode(e) => Some(e),
            FrameError::TooLarge { .. } | FrameError::Truncated => None,
        }
    }
}

/// Serialize `msg` and write one length-prefixed frame, flushed.
pub fn write_frame<T: Serialize>(writer: &mut impl Write, msg: &T) -> Result<(), FrameError> {
    let payload = postcard::to_allocvec(msg).map_err(FrameError::Encode)?;
    if payload.len() > FRAME_CAP {
        return Err(FrameError::TooLarge {
            length: payload.len(),
        });
    }
    // Cap check above keeps this conversion in range on any platform.
    let header = u32::try_from(payload.len()).map_err(|_| FrameError::TooLarge {
        length: payload.len(),
    })?;
    writer
        .write_all(&header.to_le_bytes())
        .map_err(FrameError::Io)?;
    writer.write_all(&payload).map_err(FrameError::Io)?;
    writer.flush().map_err(FrameError::Io)?;
    Ok(())
}

/// Read one frame and decode it. Returns `Ok(None)` only on a clean
/// stream-end *at a frame boundary* (peer closed between messages); a
/// torn frame (partial header or payload) is [`FrameError::Truncated`] —
/// the reconnect path treats every other error the same way: teardown.
pub fn read_frame<T: DeserializeOwned>(reader: &mut impl Read) -> Result<Option<T>, FrameError> {
    let Some(header) = read_header(reader)? else {
        return Ok(None);
    };
    let length = u32::from_le_bytes(header) as usize;
    if length > FRAME_CAP {
        return Err(FrameError::TooLarge { length });
    }
    let payload = read_payload(reader, length)?;
    let msg = postcard::from_bytes(&payload).map_err(FrameError::Decode)?;
    Ok(Some(msg))
}

/// Precise 4-byte header read distinguishing "clean close before a new
/// frame" (0 bytes) from "torn frame" (1–3 bytes) — `read_exact` cannot.
fn read_header(reader: &mut impl Read) -> Result<Option<[u8; 4]>, FrameError> {
    let mut header = [0u8; 4];
    let mut filled = 0;
    while filled < header.len() {
        match reader.read(&mut header[filled..]) {
            Ok(0) => {
                return if filled == 0 {
                    Ok(None)
                } else {
                    Err(FrameError::Truncated)
                };
            }
            Ok(n) => filled += n,
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    Ok(Some(header))
}

/// Exact payload read; anything short of `length` bytes means the peer
/// stopped mid-frame.
fn read_payload(reader: &mut impl Read, length: usize) -> Result<Vec<u8>, FrameError> {
    let mut payload = vec![0u8; length];
    let mut filled = 0;
    while filled < length {
        match reader.read(&mut payload[filled..]) {
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => filled += n,
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            Err(e) => return Err(FrameError::Io(e)),
        }
    }
    Ok(payload)
}
