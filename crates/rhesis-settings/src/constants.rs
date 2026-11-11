//! Constants of the settings crate.

/// Factory port for the embedded LanguageTool server: the `port` field's
/// default (also the Windows installer's and the tray UI's fallback).
pub const DEFAULT_PORT: u16 = 2689;

/// Current on-disk schema version. Bumped when the store layout changes;
/// the wire keeps its own protocol version (`PROTOCOL` in rhesis-protocol).
pub const SCHEMA_VERSION: u32 = 1;
