//! Translation state for the Qt tier. Location lookups live in
//! `rhesis-platform`; the persisted language override lives with the daemon
//! (its settings store) — this module only carries the translations
//! directory the language manager points at.

use rhesis_platform::resources;

pub struct LanguageManagerRust {
    pub translations_dir: String,
}

impl Default for LanguageManagerRust {
    fn default() -> Self {
        let translations_dir = resources::translations_dir()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default();
        Self { translations_dir }
    }
}
