//! Wire schema for the Rhesis daemon ↔ client socket.
//!
//! The daemon→client frames ([`daemon_msg::DaemonMsg`]), the
//! `Recommendation` payload, the length-prefixed framing and the tier
//! socket name live here, one definition both ends compile against. The
//! `Settings` snapshot payload is owned by the settings store crate
//! (`rhesis_settings::settings::Settings`); the client→daemon request
//! vocabulary lives client-side and the daemon parses it through its own
//! mirror enum, which must keep the client's variants in the same order
//! (postcard tags by index — both sides pin the order in their tests).
//! The transport (interprocess sockets) lives in the binaries.
//!
//! Framing per Refactor.md §3: `[u32 LE length][postcard payload]`, with a
//! shared frame cap enforced before any allocation or read. postcard is
//! non-self-describing, so the `Connect` handshake carrying
//! [`rhesis_constants::PROTOCOL`] is versioning load-bearing: a mismatch is
//! answered before any schema-sensitive data flows (the client shuts the
//! stale daemon down and respawns its own-install copy instead).
//!
//! Rules to keep the schema stable:
//! - any shipped change in the schema — these types, the settings
//!   snapshot, or either side's message enums — bumps
//!   [`rhesis_constants::PROTOCOL`] (starts at 1);
//! - every wire shape has a round-trip or tag-order test where it is
//!   defined (the drift tripwire);
//! - plain enums only: variant discriminants on the wire, no `#[serde(tag)]`.

pub mod daemon_msg;
pub mod frame_error;
pub mod recommendation;

use std::io::{ErrorKind, Read, Write};

use serde::{de::DeserializeOwned, Serialize};

use crate::frame_error::FrameError;

/// Maximum postcard payload length per frame, enforced on both sides before
/// any allocation or read. Ceiling chosen far above the biggest realistic
/// payload (a full text document + its suggestions) so only protocol abuse
/// or bugs ever hit it.
pub const FRAME_CAP: usize = 16 * 1024 * 1024;

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

/// Deterministic socket name for in-process test tiers (real daemon/session
/// code, tier-unique name). Production always uses
/// [`rhesis_constants::SERVICE_SOCKET_NAME`]; the pid suffix keeps parallel
/// test binaries from colliding on one name.
pub fn tier_socket_name(tier: &str) -> String {
    format!("{tier}-{}", std::process::id())
}
