use std::{
    env,
    path::{Path, PathBuf},
};

/// Directory holding the compiled `rhesis_<locale>.qm` catalogs, resolved
/// the same way for every install layout (dev tree, native, per-user,
/// AppImage, Flatpak).
pub fn find_translations_dir() -> Option<PathBuf> {
    if let Some(path) = find_translations_dir_with_prefix("/usr/share") {
        return Some(path);
    }

    // Per-user install: $HOME/.local/share/rhesis/translations
    if let Ok(home) = env::var("HOME") {
        if let Some(candidate) = find_translations_dir_with_prefix(&format!("{home}/.local/share"))
        {
            return Some(candidate);
        }
    }

    // AppImage: path relative to $APPDIR
    if let Ok(appdir) = env::var("APPDIR") {
        if let Some(candidate) = find_translations_dir_with_prefix(&format!("{appdir}/app/share")) {
            return Some(candidate);
        }
    }

    // Flatpak: the files are installed in /app
    if let Some(path) = find_translations_dir_with_prefix("/app/share") {
        return Some(path);
    }

    // Current working directory (dev fallback)
    let candidate = Path::new("./translations");
    if candidate.is_dir() {
        log::info!("Found translations in CWD");
        return Some(candidate.to_path_buf());
    }

    log::error!("Failed to find translations directory.");

    None
}

fn find_translations_dir_with_prefix(prefix: &str) -> Option<PathBuf> {
    let candidate = Path::new(&format!("{prefix}/rhesis/translations")).to_path_buf();
    log::debug!("Looking for translations in: {candidate:?}");
    if candidate.exists() {
        log::info!("Found translations: {candidate:?}");

        return Some(candidate);
    }

    None
}

/// Language override persisted by the settings page (`settings.language` in
/// the QML `Settings` store), e.g. `"el_GR"`. Empty means "system locale".
/// Reads the same QSettings INI file the QML side writes, so there is a
/// single source of truth and no extra dependency.
pub fn persisted_language() -> String {
    let config_home = env::var("XDG_CONFIG_HOME").unwrap_or_else(|_| {
        env::var("HOME").map(|home| format!("{home}/.config")).unwrap_or_default()
    });
    if config_home.is_empty() {
        return String::new();
    }
    let ini_path = Path::new(&config_home).join("rhesis/Rhesis.conf");
    let contents = std::fs::read_to_string(&ini_path).unwrap_or_default();

    // Minimal INI scan: `language=<code>` inside [General] (or before any
    // section header, which QSettings also treats as General).
    let mut in_general = true;
    for line in contents.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_general = line.eq_ignore_ascii_case("[general]");
            continue;
        }
        if in_general {
            if let Some(value) = line.strip_prefix("language=") {
                return value.trim().to_string();
            }
        }
    }
    String::new()
}

pub struct LanguageManagerRust {
    pub translations_dir: String,
}

impl Default for LanguageManagerRust {
    fn default() -> Self {
        let translations_dir = find_translations_dir()
            .map(|dir| dir.to_string_lossy().to_string())
            .unwrap_or_default();
        Self { translations_dir }
    }
}
