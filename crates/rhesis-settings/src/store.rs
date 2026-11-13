use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug)]
pub enum SaveError {
    /// A transient store has nowhere to write (no configuration root).
    Transient,
    Io(std::io::Error),
    Serialize(serde_json::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Transient => {
                write!(f, "no configuration root; settings are not persisted")
            }
            SaveError::Io(e) => write!(f, "writing {}", e),
            SaveError::Serialize(e) => write!(f, "serializing the store: {e}"),
        }
    }
}

impl std::error::Error for SaveError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SaveError::Io(e) => Some(e),
            SaveError::Serialize(e) => Some(e),
            SaveError::Transient => None,
        }
    }
}

/// The store: one file path, single-owner by design (the daemon's settings
/// thread).
pub struct Store {
    path: PathBuf,
    /// Environment has no configuration root (Refactor.md §6): the store
    /// never touches the disk — defaults everywhere, saves fail loudly.
    transient: bool,
}

impl Store {
    /// A store bound to an explicit JSON path.
    pub fn open(path: PathBuf) -> Store {
        Store {
            path,
            transient: false,
        }
    }

    /// A store for environments with no configuration root at all: loads
    /// are plain defaults and saves report failure, without inventing a
    /// file location for a run that should persist nothing. The daemon
    /// keeps serving text checks in that degraded state (and warns once at
    /// startup that settings are not persisted).
    pub fn transient() -> Store {
        Store {
            path: PathBuf::new(),
            transient: true,
        }
    }

    /// The default store location: `service.json` in the config dir family
    /// (Refactor.md §6). `None` only when the environment provides no
    /// configuration root at all (the daemon then runs defaults-only).
    pub fn default_path() -> Option<PathBuf> {
        rhesis_platform::paths::config_dir().map(|dir| dir.join("service.json"))
    }

    /// Load the current settings. Order of truth:
    /// 1. a readable, version-matching JSON file;
    /// 2. defaults, materialized once so every later load takes the file
    ///    branch.
    ///
    /// A transient store reads no file and returns plain defaults.
    pub fn load(&self) -> Settings {
        if self.transient {
            return Settings::default();
        }
        match self.read_disk() {
            Ok(settings) => settings,
            Err(LoadFailure::NoFile) => self.materialize("no settings store yet"),
            Err(LoadFailure::Unreadable(reason)) => {
                self.quarantine_corrupt(&reason);
                self.materialize("corrupt settings store quarantined")
            }
        }
    }

    /// Persist `settings` atomically (temp file + rename in the same
    /// directory).
    pub fn save(&self, settings: &Settings) -> Result<(), SaveError> {
        if self.transient {
            return Err(SaveError::Transient);
        }
        let disk = DiskStore {
            schema_version: SCHEMA_VERSION,
            settings: settings.clone(),
        };

        let payload = serde_json::to_vec_pretty(&disk).map_err(SaveError::Serialize)?;

        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(SaveError::Io)?;
        }
        let tmp_path = temp_path_of(&self.path);
        std::fs::write(&tmp_path, &payload).map_err(|io| {
            let _ = std::fs::remove_file(&tmp_path);
            SaveError::Io(io)
        })?;
        std::fs::rename(&tmp_path, &self.path).map_err(|io| {
            // Clean up the leftover temp file when the final step fails
            // (disk full, permissions on the destination, ...).
            let _ = std::fs::remove_file(&tmp_path);
            SaveError::Io(io)
        })?;
        Ok(())
    }

    fn read_disk(&self) -> Result<Settings, LoadFailure> {
        let contents = match std::fs::read_to_string(&self.path) {
            Ok(contents) => contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(LoadFailure::NoFile);
            }
            Err(e) => return Err(LoadFailure::Unreadable(format!("{e:?}"))),
        };

        let disk: DiskStore = serde_json::from_str(&contents)
            .map_err(|e| LoadFailure::Unreadable(format!("invalid store JSON: {e}")))?;

        if disk.schema_version != SCHEMA_VERSION {
            // A version the binary does not know: refuse to guess, log at
            // error level, keep the file intact for a compatible upgrade.
            log::error!(
                "settings store {} has schema_version {} but this binary knows {}; \
                 using defaults until a matching version runs",
                self.path.display(),
                disk.schema_version,
                SCHEMA_VERSION
            );
            return Ok(Settings::default());
        }

        Ok(disk.settings)
    }

    /// First boot (or post-quarantine): persist the defaults so the store
    /// file exists from the start and every later load takes the file
    /// branch.
    fn materialize(&self, context: &str) -> Settings {
        let settings = Settings::default();
        if let Err(e) = self.save(&settings) {
            log::error!("could not persist the default settings store ({context}): {e}");
        }
        settings
    }

    /// Rename an untrustworthy store file to `<path>.corrupt` (best effort)
    /// so the next `load` starts fresh instead of failing forever, and the
    /// old bytes stay visible for diagnosis.
    fn quarantine_corrupt(&self, reason: &str) {
        let corrupt = self.path.with_extension("json.corrupt");
        log::error!(
            "settings store {} is unreadable ({reason}); \
             quarantining it to {} and starting fresh",
            self.path.display(),
            corrupt.display()
        );
        let _ = std::fs::rename(&self.path, corrupt);
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct DiskStore {
    schema_version: u32,
    settings: Settings,
}

enum LoadFailure {
    /// No store file yet — the normal first boot.
    NoFile,
    /// A store file exists but cannot be trusted (unreadable or unparsable).
    Unreadable(String),
}

/// `<name>.json.tmp` next to the store file (same directory → same
/// filesystem for atomic replacement).
fn temp_path_of(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".tmp");
    PathBuf::from(temp)
}
