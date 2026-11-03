//! Read-only payload locations baked at install time.

use std::{
    env,
    path::{Path, PathBuf},
};

/// Directory holding the compiled `rhesis_<locale>.qm` catalogs, resolved
/// the same way for every install layout (dev tree, native, per-user,
/// AppImage, Flatpak).
pub fn translations_dir() -> Option<PathBuf> {
    // Windows: the installer and the portable zip keep everything next to
    // the executable.
    #[cfg(windows)]
    if let Some(dir) = crate::process::exe_dir() {
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

    if let Some(path) = translations_dir_with_prefix("/usr/share") {
        return Some(path);
    }

    // Per-user install: $HOME/.local/share/rhesis/translations
    if let Ok(home) = env::var("HOME") {
        if let Some(candidate) = translations_dir_with_prefix(&format!("{home}/.local/share")) {
            return Some(candidate);
        }
    }

    // AppImage: path relative to $APPDIR
    if let Ok(appdir) = env::var("APPDIR") {
        if let Some(candidate) = translations_dir_with_prefix(&format!("{appdir}/app/share")) {
            return Some(candidate);
        }
    }

    // Flatpak: the files are installed in /app
    if let Some(path) = translations_dir_with_prefix("/app/share") {
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

fn translations_dir_with_prefix(prefix: &str) -> Option<PathBuf> {
    let candidate = Path::new(&format!("{prefix}/rhesis/translations")).to_path_buf();
    log::debug!("Looking for translations in: {candidate:?}");
    if candidate.exists() {
        log::info!("Found translations: {candidate:?}");

        return Some(candidate);
    }

    None
}
