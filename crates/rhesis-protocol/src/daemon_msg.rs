//! Daemon → client messages.

use rhesis_settings::settings::Settings;

use crate::recommendation::Recommendation;
use serde::{Deserialize, Serialize};

/// Daemon → client messages.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DaemonMsg {
    /// Handshake answer: accepted version + whether another client session
    /// currently owns the single-client slot (§3: one session v1).
    Connect { protocol: u32, busy: bool },
    /// The daemon-side settings snapshot (pushed after `SetSettings`).
    Settings(Settings),
    /// Worker status transition (always broadcast as it happens), carried
    /// flattened: `code` 0–5 is the daemon-reported status space (the
    /// worker crate maps its rich status onto it); 6 is the client's local
    /// "no session" state and never travels here. `reason` is the
    /// human-readable failure detail, empty unless the status failed.
    Status { code: i32, reason: String },
    /// Suggestion batch for the most recent accepted `Check`.
    Suggestion(Vec<Recommendation>),
}
