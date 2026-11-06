//! Headless Rhesis service: the daemon that owns the LanguageTool server
//! and every setting, plus the single-client pump that serves the Qt UI
//! over a local socket (Refactor.md §1–§3).
//!
//! Crate layout:
//! - [`config`] — snapshot → worker-event translation (the reconcile core);
//! - [`daemon`] — process lifecycle: options, shared state, accept loop,
//!   idle grace, worker reaping;
//! - [`session`] — the one-session conversation: handshake, replay,
//!   translation, relay.

pub mod config;
mod constants;
pub mod daemon;
pub mod session;

/// The process-facing API: run the daemon with resolved options.
pub use daemon::{run, DaemonOptions};
