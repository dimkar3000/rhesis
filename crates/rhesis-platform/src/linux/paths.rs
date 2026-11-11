//! Linux configuration file locations Rhesis persists state into.

use std::{env, ffi::OsStr, path::PathBuf};

/// Directory that holds Rhesis's per-user configuration: `~/.config/rhesis`
/// (or `$XDG_CONFIG_HOME/rhesis`). Every file the service side writes lives
/// here (the `service.json` store); the client consults all of it through
/// the daemon.
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

/// Pure window of [`config_dir`] (tests pass directories; production passes
/// environment values). `$XDG_CONFIG_HOME`/`HOME` win outright and missing
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
