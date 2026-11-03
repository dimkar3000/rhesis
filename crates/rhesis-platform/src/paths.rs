//! Configuration file locations Rhesis persists state into.

use std::{env, ffi::OsStr, path::PathBuf};

/// Directory that holds Rhesis's per-user configuration: `~/.config/rhesis`
/// (or `$XDG_CONFIG_HOME/rhesis`) on Linux, `%APPDATA%\rhesis` on Windows.
/// Every file the service side writes lives here (store `service.json`, the
/// legacy QSettings INI the one-time migration seeds from); the client
/// consults all of it through the daemon.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        config_dir_with(env::var_os("APPDATA").as_deref())
    }

    #[cfg(not(windows))]
    {
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
}

/// Pure window of [`config_dir`] (tests pass directories; production passes
/// environment values). Mirrors the lookup `qsettings_ini_path` shipped
/// before the refactor: `XDG_CONFIG_HOME`/`%APPDATA%` win outright and
/// missing variables degrade to `None`.
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

/// Location of the QSettings INI file the pre-service UI wrote (the source
/// of the one-time settings migration). Windows: `%APPDATA%\rhesis\Rhesis.ini`
/// (the UI forced the INI format, QSettings would otherwise use the
/// registry); Linux: `$XDG_CONFIG_HOME/rhesis/Rhesis.conf`.
#[cfg(windows)]
pub fn qsettings_ini_path() -> Option<PathBuf> {
    match config_dir() {
        Some(dir) => Some(dir.join("Rhesis.ini")),
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
