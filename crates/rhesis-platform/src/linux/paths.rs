//! Linux configuration file locations Rhesis persists state into.

use std::{env, ffi::OsStr, path::PathBuf};

/// Directory that holds Rhesis's per-user configuration: `~/.config/rhesis`
/// (or `$XDG_CONFIG_HOME/rhesis`). Every file the service side writes lives
/// here (store `service.json`, the legacy QSettings INI the one-time
/// migration seeds from); the client consults all of it through the daemon.
pub fn config_dir() -> Option<PathBuf> {
    let config_root = match env::var_os("XDG_CONFIG_HOME") {
        Some(dir) => Some(dir),
        None => env::var_os("HOME").map(|home| {
            let mut config = home.to_os_string();
            config.push("/.config");
            config
        }),
    };
    config_dir_with(config_root.as_deref())
}

/// Location of the QSettings INI file the pre-service UI wrote (the source
/// of the one-time settings migration): `$XDG_CONFIG_HOME/rhesis/Rhesis.conf`.
pub fn qsettings_ini_path() -> Option<PathBuf> {
    match config_dir() {
        Some(dir) => Some(dir.join("Rhesis.conf")),
        None => {
            log::error!(
                "neither XDG_CONFIG_HOME nor HOME is set; \
                 cannot read persisted language, falling back to system locale"
            );
            None
        }
    }
}

/// Pure window of [`config_dir`] (tests pass directories; production passes
/// environment values). Mirrors the lookup `qsettings_ini_path` shipped
/// before the refactor: `XDG_CONFIG_HOME`/`HOME` win outright and missing
/// variables degrade to `None`.
fn config_dir_with(config_root: Option<&OsStr>) -> Option<PathBuf> {
    let config_root = match config_root {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => {
            log::debug!("no configuration root in the environment; using defaults");
            return None;
        }
    };
    Some(config_root.join("rhesis"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_root_is_joined_with_the_app_dir() {
        let dir = config_dir_with(Some(OsStr::new("/cfg/home")));
        assert_eq!(dir, Some(PathBuf::from("/cfg/home/rhesis")));
    }

    #[test]
    fn empty_or_missing_root_degrades_to_none() {
        assert_eq!(config_dir_with(Some(OsStr::new(""))), None);
        assert_eq!(config_dir_with(None), None);
    }
}
