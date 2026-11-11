//! The single source of settings truth: one JSON file, whole-file atomic
//! writes, lenient reads, and the one-time migration from the pre-service
//! QSettings INI. On disk:
//!
//! ```json
//! { "schema_version": 1, "migrated_v1": false, "settings": { ... } }
//! ```
//!
//! Error policy: every read failure degrades to defaults (or a migration
//! seed) with a warning — a broken file must never keep the daemon from
//! serving text checks. A corrupt file is *renamed* (`.corrupt`), never
//! deleted, so a human can still see what was wrong.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use crate::settings::Settings;
use serde::{Deserialize, Serialize};

use crate::ini;

/// Current on-disk schema version. Bumped when the store layout changes;
/// the wire keeps its own protocol version (`PROTOCOL` in rhesis-constants).
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

/// The store: one file path plus the migration marker mirrored in memory
/// so saves keep the marker as-is. Single-owner by design (the daemon's
/// settings thread); the marker uses interior mutability to keep `load` on
/// `&self`.
pub struct Store {
    path: PathBuf,
    /// Environment has no configuration root (Refactor.md §6): the store
    /// never touches the disk — defaults everywhere, saves fail loudly.
    transient: bool,
    /// INI path override for hermetic tests: `None` resolves through
    /// `rhesis_platform::paths::qsettings_ini_path`.
    ini_path: Option<PathBuf>,
    migrated_v1: Cell<bool>,
}

impl Store {
    /// A store bound to an explicit JSON path.
    pub fn open(path: PathBuf) -> Store {
        Store {
            path,
            transient: false,
            ini_path: None,
            migrated_v1: Cell::new(false),
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
            ini_path: None,
            migrated_v1: Cell::new(false),
        }
    }

    /// The default store location: `service.json` in the config dir family
    /// (Refactor.md §6). `None` only when the environment provides no
    /// configuration root at all (the daemon then runs defaults-only).
    pub fn default_path() -> Option<PathBuf> {
        rhesis_platform::paths::config_dir().map(|dir| dir.join("service.json"))
    }

    /// Hermetic seam for tests (and for `ui-boot` tiers): pin the INI the
    /// migration would consult, instead of the real user configuration.
    pub fn with_ini_path(mut self, ini_path: Option<PathBuf>) -> Store {
        self.ini_path = ini_path;
        self
    }

    /// Load the current settings. Order of truth:
    /// 1. a readable, version-matching JSON file;
    /// 2. a one-time seed from the legacy QSettings INI (persisted so it
    ///    runs once — the INI is never rewritten, ready for rollback);
    /// 3. defaults.
    ///
    /// A transient store reads no file and returns plain defaults.
    pub fn load(&self) -> Settings {
        if self.transient {
            return Settings::default();
        }
        match self.read_disk() {
            Ok((settings, migrated_v1)) => {
                self.migrated_v1.set(migrated_v1);
                settings
            }
            Err(LoadFailure::NoFile) => self.seed_once("no settings store yet"),
            Err(LoadFailure::Unreadable(reason)) => {
                self.quarantine_corrupt(&reason);
                self.seed_once("corrupt settings store quarantined")
            }
        }
    }

    /// Persist `settings` atomically (temp file + rename in the same
    /// directory). The migration marker stays whatever `load` seeded.
    pub fn save(&self, settings: &Settings) -> Result<(), SaveError> {
        if self.transient {
            return Err(SaveError::Transient);
        }
        let disk = DiskStore {
            schema_version: SCHEMA_VERSION,
            migrated_v1: self.migrated_v1.get(),
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

    fn read_disk(&self) -> Result<(Settings, bool), LoadFailure> {
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
            return Ok((Settings::default(), disk.migrated_v1));
        }

        Ok((disk.settings, disk.migrated_v1))
    }

    /// Seed once (`migrated_v1` stays false for a fresh defaults store;
    /// a legacy INI that contributed values marks the store):
    /// 1. load → seeding happens only when no readable store file exists;
    /// 2. the seed writes the store immediately, so every later load takes
    ///    the file branch — even when the INI changes or disappears.
    fn seed_once(&self, context: &str) -> Settings {
        let (settings, migrated) = match self.migration_sources() {
            MigrationSeed::None => (Settings::default(), false),
            MigrationSeed::LegacyIni(ini_path) => match ini::read_ini(&ini_path) {
                Ok(parsed) => {
                    log::info!(
                        "{context}; migrating legacy keys from {}",
                        ini_path.display()
                    );
                    (
                        ini::settings_from_ini(&parsed, &mut |warn| log::warn!("{warn}")),
                        true,
                    )
                }
                Err(e) => {
                    log::warn!(
                        "{context}; legacy INI {} exists but is unreadable ({e:?}), \
                         using defaults",
                        ini_path.display()
                    );
                    (Settings::default(), false)
                }
            },
        };

        self.migrated_v1.set(migrated);
        if let Err(e) = self.save(&settings) {
            log::error!("could not persist the seeded settings store: {e}");
        }
        settings
    }

    /// Which legacy source applies: the INI path (pinned or platform
    /// resolved), only when the file exists. JSON always shadows the INI —
    /// this fn only runs when the JSON was missing/unreadable anyway.
    fn migration_sources(&self) -> MigrationSeed {
        let ini_path = match &self.ini_path {
            Some(pinned) => Some(pinned.clone()),
            None => rhesis_platform::paths::qsettings_ini_path(),
        };
        match ini_path {
            Some(ini_path) if ini_path.is_file() => MigrationSeed::LegacyIni(ini_path),
            _ => MigrationSeed::None,
        }
    }

    /// Rename an untrustworthy store file to `<path>.corrupt` (best effort)
    /// so the next `load` takes a fresh seed instead of failing forever,
    /// and the old bytes stay visible for diagnosis.
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
    /// One-time marker: the v1 store was seeded from the legacy QSettings
    /// INI. Informational for the migration logic (once a store file
    /// exists, the INI is never consulted again — see `load`).
    #[serde(default)]
    migrated_v1: bool,
    settings: Settings,
}

enum LoadFailure {
    /// No store file yet — the normal first boot.
    NoFile,
    /// A store file exists but cannot be trusted (unreadable or unparsable).
    Unreadable(String),
}

enum MigrationSeed {
    None,
    LegacyIni(PathBuf),
}

/// `<name>.json.tmp` next to the store file (same directory → same
/// filesystem for atomic replacement).
fn temp_path_of(path: &Path) -> PathBuf {
    let mut temp = path.as_os_str().to_os_string();
    temp.push(".tmp");
    PathBuf::from(temp)
}
