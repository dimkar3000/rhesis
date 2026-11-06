//! Constants both ends of the daemon ↔ client socket compile against.
//!
//! Everything a stale daemon could disagree with a fresh client about:
//! the wire version, the service socket name, the frame cap, the
//! embedded-server factory port and the client-local disconnected code.
//! Re-exported from the crate root, so the historical paths
//! (`rhesis_protocol::PROTOCOL`, …) keep working unchanged.

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
/// carries the six daemon-reported [`crate::WorkerStatus`] states.
pub const CLIENT_STATUS_DISCONNECTED: i32 = 6;

/// Factory port for the embedded LanguageTool server (also the Windows
/// installer's and the tray UI's fallback).
pub const DEFAULT_PORT: u16 = 2689;
