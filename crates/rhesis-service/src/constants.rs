//! Daemon lifecycle constants.

use std::time::Duration;

/// How often the accept loop wakes between connection attempts to service
/// end signals and the grace countdown. Slice only — a connect is answered
/// within this window; the session itself is fully synchronous.
pub(crate) const ACCEPT_SLICE: Duration = Duration::from_millis(50);
