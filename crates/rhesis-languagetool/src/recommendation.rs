//! The suggestion structs now live in `rhesis-protocol`: a single definition
//! shared by the worker, the client and the wire (Refactor.md §1). Kept as a
//! re-export module so the established `rhesis_languagetool::recommendation`
//! paths (worker internals, test tiers, the Qt tier) keep working.

pub use rhesis_protocol::{Range, Recommendation};
