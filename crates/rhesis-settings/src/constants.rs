//! Constants of the settings crate.

/// Current on-disk schema version. Bumped when the store layout changes;
/// the wire keeps its own [`rhesis_protocol::PROTOCOL`] version.
pub const SCHEMA_VERSION: u32 = 1;
