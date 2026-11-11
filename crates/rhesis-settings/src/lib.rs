//! Versioned JSON settings store owned by the Rhesis daemon.
//!
//! Layout per Refactor.md §6: one JSON file (`service.json` in the config
//! dir family), whole-file atomic writes per change. Reads are lenient
//! (unknown JSON fields are ignored) and every failure degrades to
//! defaults with a log line, never a panic: the daemon must boot to serve
//! text checks even with a broken file.

pub mod settings;
pub mod store;
