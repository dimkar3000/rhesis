//! Contract constants shared across the Rhesis crates: everything more
//! than one crate must agree on. Crate-internal tuning knobs stay in their
//! owning crates — values only one crate reads never travel here.

/// Wire format version, sent first by both peers as `Connect`. A mismatch
/// means the peers disagree about the wire types at compile time (e.g. a
/// pre-upgrade daemon still lingering in its idle grace).
pub const PROTOCOL: u32 = 1;

/// Socket the daemon binds: the abstract-namespace UDS name on Linux
/// (Flatpak ↔ host share the network namespace) and the same-named pipe on
/// Windows, resolved by the `interprocess` layer.
pub const SERVICE_SOCKET_NAME: &str = "io.github.dimkar3000.rhesis.service";

/// Factory port for the embedded LanguageTool server: the settings' `port`
/// field default (also the Windows installer's and the tray UI's fallback).
pub const DEFAULT_PORT: u16 = 2689;
