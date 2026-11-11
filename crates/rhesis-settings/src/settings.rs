//! User-adjustable state the daemon owns: persisted by the settings store,
//! mirrored to the client, and override-able through `SetSettings`.
//!
//! The store schema IS this struct (Refactor.md §6) — field-for-field.
//! JSON keys keep the QML-era names (`embedded`, `externalAddress`, …)
//! so the migrated store diffs cleanly against the old INI; the wire is
//! position-driven postcard and ignores the naming.

use std::collections::BTreeMap;

use rhesis_constants::DEFAULT_PORT;
use serde::{Deserialize, Serialize};

/// User-adjustable state the daemon owns: persisted by the settings store,
/// mirrored to the client, and override-able through `SetSettings`.
/// The store schema IS this struct (Refactor.md §6) — field-for-field.
/// JSON keys keep the QML-era names (`embedded`, `externalAddress`, ...)
/// so the migrated store diffs cleanly against the old INI; the wire is
/// position-driven postcard and ignores the naming.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// The embedded LanguageTool server (vs. an external instance).
    pub embedded: bool,
    /// Port for the embedded server (also used as the daemon's LT port).
    pub port: u16,
    /// Base URL of the external server when `embedded` is false.
    pub external_address: String,
    /// Host the LanguageTool server while no client is connected (the idle
    /// shutdown grace is skipped while this is on).
    pub background: bool,
    pub show_tooltips: bool,
    pub show_debug_tooltips: bool,
    /// Locale override (e.g. "el_GR"); empty means system locale.
    pub language: String,
    /// Color table for rules and rule categories (`CATEGORY:x`, `RULE:y`).
    /// BTree: deterministic wire bytes + JSON keys, stable diffing.
    pub color_settings: BTreeMap<String, String>,
}

impl Default for Settings {
    /// Same defaults the QML `Settings` block shipped pre-refactor.
    fn default() -> Self {
        let color_settings = BTreeMap::from([
            ("CATEGORY:GRAMMAR".to_string(), "#ecc224".to_string()),
            ("CATEGORY:TYPOGRAPHY".to_string(), "#f63ef9".to_string()),
            ("CATEGORY:TYPOS".to_string(), "#1ae918".to_string()),
        ]);
        Self {
            embedded: true,
            port: DEFAULT_PORT,
            external_address: String::new(),
            background: false,
            show_tooltips: true,
            show_debug_tooltips: true,
            language: String::new(),
            color_settings,
        }
    }
}
