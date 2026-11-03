//! LanguageTool worker machinery, extracted from the Qt application: the
//! worker thread that owns the embedded LanguageTool server's lifetime, the
//! HTTP client that transforms `/v2/check` answers into recommendations,
//! and the recorded-fixture shoreline used by the harness tiers.
//!
//! Suggestion payload types (`Recommendation`, `WorkerStatus`) are
//! String-based and defined once in rhesis-protocol (re-exported here);
//! this crate is UI-free and the Qt tier converts at its boundary.

pub mod client;
pub mod models;
mod provider;
pub mod recommendation;
pub mod service;
