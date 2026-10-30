use std::{
    env,
    path::{Path, PathBuf},
};

/// Directory holding the compiled `rhesis_<locale>.qm` catalogs, resolved
/// the same way for every install layout (dev tree, native, per-user,
/// AppImage, Flatpak).
pub fn find_translations_dir() -> Option<PathBuf> {
    // Windows: the installer and the portable zip keep everything next to
    // the executable.
    #[cfg(windows)]
    if let Some(dir) = exe_dir() {
        for candidate in [
            dir.join("translations"),
            dir.join("../share/rhesis/translations"),
        ] {
            log::debug!("Looking for translations in: {candidate:?}");
            if candidate.is_dir() {
                log::info!("Found translations: {candidate:?}");
                return Some(candidate);
            }
        }
    }

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

/// Directory containing the running executable (Windows install layout root).
#[cfg(windows)]
pub fn exe_dir() -> Option<PathBuf> {
    match env::current_exe() {
        Ok(exe) => exe.parent().map(Path::to_path_buf),
        Err(e) => {
            log::warn!("cannot determine the executable location: {e:?}");
            None
        }
    }
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

/// Location of the QSettings INI file shared with the QML `Settings` store.
/// Windows: `%APPDATA%\rhesis\Rhesis.ini` (main.rs forces the INI format,
/// QSettings would otherwise use the registry).
#[cfg(windows)]
fn settings_ini_path() -> Option<PathBuf> {
    match env::var_os("APPDATA") {
        Some(dir) => Some(Path::new(&dir).join("rhesis").join("Rhesis.ini")),
        None => {
            log::error!(
                "APPDATA is not set; cannot read persisted language, \
                 falling back to system locale"
            );
            None
        }
    }
}

#[cfg(not(windows))]
fn settings_ini_path() -> Option<PathBuf> {
    let config_home = match env::var("XDG_CONFIG_HOME") {
        Ok(dir) => dir,
        Err(_) => match env::var("HOME") {
            Ok(home) => format!("{home}/.config"),
            Err(e) => {
                log::error!(
                    "neither XDG_CONFIG_HOME nor HOME is set ({e:?}); \
                     cannot read persisted language, falling back to system locale"
                );
                return None;
            }
        },
    };
    Some(Path::new(&config_home).join("rhesis/Rhesis.conf"))
}

/// Language override persisted by the settings page (`settings.language` in
/// the QML `Settings` store), e.g. `"el_GR"`. Empty means "system locale".
/// Reads the same QSettings INI file the QML side writes, so there is a
/// single source of truth and no extra dependency.
pub fn persisted_language() -> String {
    let Some(ini_path) = settings_ini_path() else {
        return String::new();
    };
    let contents = match std::fs::read_to_string(&ini_path) {
        Ok(contents) => contents,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // First run: the QML Settings store has not persisted anything yet.
            log::debug!("no persisted settings at {ini_path:?}, using system locale");
            return String::new();
        }
        Err(e) => {
            log::error!(
                "failed to read persisted settings at {ini_path:?}: {e:?}; \
                 falling back to system locale"
            );
            return String::new();
        }
    };

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
