//! Versioned JSON settings store owned by the Rhesis daemon.
//!
//! Layout per Refactor.md §6: one JSON file (`service.json` in the config
//! dir family), whole-file atomic writes per change, a one-time migration
//! from the pre-service QSettings INI (read-only — the INI stays behind for
//! rollback), and a migration marker so a later-appearing stale INI never
//! overwrites the live store. Reads are lenient (unknown JSON fields are
//! ignored) and every failure degrades to defaults with a log line, never a
//! panic: the daemon must boot to serve text checks even with a broken file.

pub mod constants;
pub mod ini;
pub mod settings;
pub mod store;
