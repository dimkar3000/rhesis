//! The Linux install identity: one per-user configuration file
//! (`install.conf` beside the daemon's settings store) carrying the flavor
//! and the resolved payload locations. The client seeds it from a pure
//! derivation of the binary location; both the client and the daemon read
//! it, and a moved install writes it back. No wrapper process exports
//! anything — the file is the only handoff between packaging and runtime.

use std::fmt::Write as _;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use super::paths;

/// Name of the configuration file inside [`paths::config_dir`].
const FILE_NAME: &str = "install.conf";

/// Config file keys. Unknown keys are left alone, so an older app reading
/// a newer file keeps working.
const KEY_FLAVOR: &str = "flavor";
const KEY_TRANSLATIONS: &str = "translations";
const KEY_LANGUAGETOOL: &str = "languagetool";
const KEY_JAVA: &str = "java";
const KEY_AUTOSTART: &str = "autostart-exec";

/// The flavors Rhesis ships on Linux. Flatpak and AppImage are identified
/// by the variables their runtimes inject (`FLATPAK_ID`, `APPIMAGE`); the
/// tarball install is the plain remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallationFlavor {
    LinuxLocal,
    LinuxFlatpak,
    LinuxAppImage,
}

impl InstallationFlavor {
    pub fn name(self) -> &'static str {
        match self {
            InstallationFlavor::LinuxLocal => "linux-local",
            InstallationFlavor::LinuxFlatpak => "linux-flatpak",
            InstallationFlavor::LinuxAppImage => "linux-appimage",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<InstallationFlavor> {
        match value {
            "linux-local" => Some(InstallationFlavor::LinuxLocal),
            "linux-flatpak" => Some(InstallationFlavor::LinuxFlatpak),
            "linux-appimage" => Some(InstallationFlavor::LinuxAppImage),
            _ => None,
        }
    }
}

/// The resolved install identity: the flavor plus every payload location,
/// sourced either from this install's own `install.conf` (the
/// `autostart_record` then says what the autostart entry ran last) or from
/// a pure location derivation, which carries no record (`None`).
#[derive(Debug, Clone)]
pub struct InstallationSetup {
    pub flavor: InstallationFlavor,
    /// Directory of the compiled `rhesis_<locale>.qm` catalogs. `None` for
    /// a dev run whose working directory has no `translations` folder.
    pub translations_dir: Option<PathBuf>,
    /// LanguageTool payload directory (`languagetool-server.jar` lives
    /// here). `None` for a dev run without a setup.sh download.
    pub languagetool_dir: Option<PathBuf>,
    /// Java executable the LanguageTool server is started with.
    pub java: PathBuf,
    /// The Exec line (with ` --hidden`) recorded for the OS autostart
    /// entry, when the on-disk configuration describes this very install —
    /// a record written by a foreign flavor or copy never leaks.
    pub autostart_record: Option<String>,
}

impl InstallationSetup {
    /// Client entry point: resolve the install and keep the configuration on
    /// disk in sync with it (seed on first run, write back when the payload
    /// moved). Dev builds (`cargo run` / `cargo test`) resolve the working
    /// directory instead, so development never touches a real install's config.
    pub fn ensure() -> Option<InstallationSetup> {
        Self::resolve(true)
    }

    /// Daemon entry point: resolve read-only. Same resolution as
    /// [`ensure`], never writing the configuration file.
    pub fn read_only() -> Option<InstallationSetup> {
        Self::resolve(false)
    }

    fn resolve(writes: bool) -> Option<InstallationSetup> {
        // Dev runs never touch an install configuration; their locations
        // come from the working directory alone.
        if cfg!(debug_assertions) {
            return Some(dev_setup());
        }

        let exe_dir = env::current_exe().ok()?.parent()?.to_path_buf();
        let derived = derive(&exe_dir);

        let Some(config_root) = paths::config_dir() else {
            log::warn!("no configuration root; resolving paths without persisting");
            return Some(derived);
        };

        match Conf::load(&config_root) {
            // Exactly us: the read path, autostart record included.
            Some(conf) if conf.meets(&derived) => Some(InstallationSetup {
                flavor: conf.flavor,
                translations_dir: Some(conf.translations),
                languagetool_dir: Some(conf.languagetool),
                java: conf.java,
                autostart_record: conf.autostart,
            }),
            // Same flavor, but the recorded LanguageTool location is gone:
            // this install moved (re-extracted payload, relocated AppImage) —
            // write the fresh derivation back and keep the autostart record.
            Some(conf) if conf.flavor == derived.flavor && !conf.languagetool.is_dir() => {
                let record = conf.autostart;
                if writes {
                    if let Some(conf) = derived.conf_from(record.clone()) {
                        if let Err(e) = conf.save(&config_root) {
                            log::error!("failed to write back the moved install: {e:?}");
                        }
                    }
                }
                Some(InstallationSetup {
                    autostart_record: record,
                    ..derived
                })
            }
            // A foreign flavor or a sibling copy of the same flavor: the file
            // describes some other install and is left alone. This process
            // serves itself from the derivation and gets no record.
            Some(_) => Some(derived),
            // Nothing on disk yet: seed it for this install.
            None => {
                if writes {
                    if let Some(conf) = derived.conf_from(None) {
                        if let Err(e) = conf.save(&config_root) {
                            log::error!("failed to seed the install configuration: {e:?}");
                        }
                    }
                }
                Some(derived)
            }
        }
    }

    fn conf_from(&self, autostart: Option<String>) -> Option<Conf> {
        Some(Conf {
            flavor: self.flavor,
            translations: self.translations_dir.clone()?,
            languagetool: self.languagetool_dir.clone()?,
            java: self.java.clone(),
            autostart,
        })
    }
}

/// Record (replace) the autostart Exec line in the configuration. Only the
/// conf describing this running install is touched, and only when it is
/// present — the toggle has just ensured the file for this install.
pub fn record_autostart(line: &str) {
    let Some(config_root) = paths::config_dir() else {
        return;
    };
    let Some(exe_dir) = env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    else {
        return;
    };
    let derived = derive(&exe_dir);
    if let Some(mut conf) = Conf::load(&config_root) {
        if conf.meets(&derived) {
            conf.autostart = Some(line.to_string());
            if let Err(e) = conf.save(&config_root) {
                log::error!("failed to record the autostart exec line: {e:?}");
            }
        }
    }
}

/// The payload root of this running install — the rhesis folder holding
/// `bin/`, `lib/`, `LanguageTool/`, `translations/` — for the in-binary
/// uninstall. `None` in dev runs (nothing installed).
pub fn payload_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    super::process::exe_dir()?.parent().map(Path::to_path_buf)
}

/// One file-level alias: `install.conf`'s location inside the app
/// configuration tree.
pub fn path() -> Option<PathBuf> {
    paths::config_dir().map(|dir| Conf::conf_path(&dir))
}

/// One parsed configuration file.
#[derive(Clone)]
pub(crate) struct Conf {
    pub flavor: InstallationFlavor,
    pub translations: PathBuf,
    pub languagetool: PathBuf,
    pub java: PathBuf,
    pub autostart: Option<String>,
}

impl Conf {
    /// Strict parse: all four location keys must be present; a missing one,
    /// an unknown flavor value or a line without `=` makes the whole file
    /// unreadable and triggers a re-seed.
    pub(crate) fn parse(text: &str) -> Option<Conf> {
        let mut flavor = None;
        let mut translations = None;
        let mut languagetool = None;
        let mut java = None;
        let mut autostart = None;

        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let (key, value) = line.split_once('=')?;
            let value = value.trim();
            match key.trim() {
                KEY_FLAVOR => flavor = InstallationFlavor::parse(value),
                KEY_TRANSLATIONS => translations = Self::non_empty(value).map(PathBuf::from),
                KEY_LANGUAGETOOL => languagetool = Self::non_empty(value).map(PathBuf::from),
                KEY_JAVA => java = Self::non_empty(value).map(PathBuf::from),
                KEY_AUTOSTART => autostart = Self::non_empty(value),
                _ => {}
            }
        }

        Some(Conf {
            flavor: flavor?,
            translations: translations?,
            languagetool: languagetool?,
            java: java?,
            autostart,
        })
    }

    pub(crate) fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str("# Rhesis install configuration; maintained by the app.\n");

        let _ = writeln!(out, "{KEY_FLAVOR}={}", self.flavor.name());
        let _ = writeln!(out, "{KEY_TRANSLATIONS}={}", self.translations.display());
        let _ = writeln!(out, "{KEY_LANGUAGETOOL}={}", self.languagetool.display());
        let _ = writeln!(out, "{KEY_JAVA}={}", self.java.display());
        if let Some(line) = &self.autostart {
            let _ = writeln!(out, "{KEY_AUTOSTART}={line}");
        }
        out
    }

    pub(crate) fn load(config_root: &Path) -> Option<Conf> {
        let text = fs::read_to_string(Self::conf_path(config_root)).ok()?;
        Conf::parse(&text)
    }

    /// Whether the file describes exactly this running install.
    fn meets(&self, setup: &InstallationSetup) -> bool {
        self.flavor == setup.flavor
            && Some(&self.translations) == setup.translations_dir.as_ref()
            && Some(&self.languagetool) == setup.languagetool_dir.as_ref()
            && self.java == setup.java
    }

    fn save(&self, config_root: &Path) -> Result<(), std::io::Error> {
        fs::create_dir_all(config_root)?;
        fs::write(Self::conf_path(config_root), self.serialize())
    }

    fn conf_path(config_root: &Path) -> PathBuf {
        config_root.join(FILE_NAME)
    }

    fn non_empty(value: &str) -> Option<String> {
        if value.is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }
}

/// Pure derivation for an installed payload, from where the binary sits
/// and the variables the runtimes inject (`APPIMAGE`, `FLATPAK_ID`).
/// Flatpak and AppImage payloads live under `$<root>/bin` with their data
/// in `$<root>/share/rhesis`; the tarball keeps translations beside the
/// rhesis folder. Injection keeps every caller testable.
pub(crate) fn derive_with(
    exe_dir: &Path,
    appimage: Option<&str>,
    flatpak_id: Option<&str>,
) -> InstallationSetup {
    let root = exe_dir
        .parent()
        .expect("the install binary always sits in a directory");

    let share = root.join("share").join("rhesis");
    let flavor;
    let (translations_dir, languagetool_dir) = if flatpak_id.is_some() || appimage.is_some() {
        flavor = if flatpak_id.is_some() {
            InstallationFlavor::LinuxFlatpak
        } else {
            InstallationFlavor::LinuxAppImage
        };
        (share.join("translations"), share.join("LanguageTool"))
    } else {
        flavor = InstallationFlavor::LinuxLocal;
        (root.join("translations"), root.join("LanguageTool"))
    };

    InstallationSetup {
        flavor,
        translations_dir: Some(translations_dir),
        languagetool_dir: Some(languagetool_dir),
        java: root.join("lib").join("jre").join("bin").join("java"),
        autostart_record: None,
    }
}

fn dev_setup() -> InstallationSetup {
    let cwd = std::env::current_dir().ok();
    InstallationSetup {
        flavor: InstallationFlavor::LinuxLocal,
        translations_dir: cwd.as_deref().and_then(dev_translations_dir),
        languagetool_dir: None,
        java: PathBuf::from("java"),
        autostart_record: None,
    }
}

fn dev_translations_dir(dir: &Path) -> Option<PathBuf> {
    let candidate = dir.join("translations");
    candidate.is_dir().then_some(candidate)
}

/// Derivation for the running process: injected `APPIMAGE` / `FLATPAK_ID`
/// (flatpak and the AppImage runtime place them in every launch).
fn derive(exe_dir: &Path) -> InstallationSetup {
    let appimage = env::var("APPIMAGE").ok().filter(|p| !p.is_empty());
    let flatpak_id = env::var("FLATPAK_ID").ok().filter(|p| !p.is_empty());
    derive_with(exe_dir, appimage.as_deref(), flatpak_id.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn derived_local(root: &str) -> InstallationSetup {
        derive_with(Path::new(root).join("bin").as_path(), None, None)
    }

    #[test]
    fn conf_round_trips() {
        let conf = Conf {
            flavor: InstallationFlavor::LinuxAppImage,
            translations: PathBuf::from("/apps/rhesis/share/rhesis/translations"),
            languagetool: PathBuf::from("/apps/rhesis/share/rhesis/LanguageTool"),
            java: PathBuf::from("/apps/rhesis/lib/jre/bin/java"),
            autostart: Some("/apps/Rhesis.AppImage --hidden".to_string()),
        };
        let parsed = Conf::parse(&conf.serialize()).expect("our own file parses back");
        assert_eq!(parsed.flavor, conf.flavor);
        assert_eq!(parsed.translations, conf.translations);
        assert_eq!(parsed.languagetool, conf.languagetool);
        assert_eq!(parsed.java, conf.java);
        assert_eq!(
            parsed.autostart.as_deref(),
            Some("/apps/Rhesis.AppImage --hidden")
        );
    }

    #[test]
    fn incomplete_or_garbage_files_are_unreadable() {
        // All location keys are required.
        let partial = "flavor=linux-local\ntranslations=/x\nlanguagetool=/y\n";
        assert!(
            Conf::parse(partial).is_none(),
            "missing java makes the file unreadable"
        );

        // Unknown flavor cannot resolve.
        let bad_flavor = "flavor=snap\ntranslations=/x\nlanguagetool=/y\njava=/j\n";
        assert!(Conf::parse(bad_flavor).is_none());

        // A comment-only file is not an install configuration.
        assert!(Conf::parse("# Rhesis install configuration\n").is_none());

        // Unknown keys are ignored, comments are skipped.
        let future = "# header\n; comment\nunknown=whatever\n\
                      flavor=linux-flatpak\ntranslations=/x\nlanguagetool=/y\njava=/j\n";
        let conf = Conf::parse(future).expect("unknown keys are forward-compatible");
        assert_eq!(conf.flavor, InstallationFlavor::LinuxFlatpak);
    }

    #[test]
    fn derivation_matches_each_shipped_layout() {
        // Tarball install: /usr/share/rhesis/bin/rhesis → data beside the
        // rhesis folder.
        let local = derived_local("/usr/share/rhesis");
        assert_eq!(local.flavor, InstallationFlavor::LinuxLocal);
        assert_eq!(
            local.translations_dir.as_deref(),
            Some(Path::new("/usr/share/rhesis/translations"))
        );
        assert_eq!(
            local.languagetool_dir.as_deref(),
            Some(Path::new("/usr/share/rhesis/LanguageTool"))
        );
        assert_eq!(
            local.java,
            PathBuf::from("/usr/share/rhesis/lib/jre/bin/java")
        );

        // Flatpak: /app/bin/rhesis, data under /app/share/rhesis (the
        // manifest's cp -r share/* /app/share/).
        let flatpak = derive_with(
            Path::new("/app/bin"),
            None,
            Some("io.github.dimkar3000.rhesis"),
        );
        assert_eq!(flatpak.flavor, InstallationFlavor::LinuxFlatpak);
        assert_eq!(
            flatpak.translations_dir.as_deref(),
            Some(Path::new("/app/share/rhesis/translations"))
        );

        // AppImage: the payload runs from $APPDIR/app/bin; the image path
        // matters for detection only.
        let appimage = derive_with(
            Path::new("/tmp/.mount_rhesis/app/bin"),
            Some("/apps/Rhesis.AppImage"),
            None,
        );
        assert_eq!(appimage.flavor, InstallationFlavor::LinuxAppImage);
        assert_eq!(
            appimage.languagetool_dir.as_deref(),
            Some(Path::new(
                "/tmp/.mount_rhesis/app/share/rhesis/LanguageTool"
            ))
        );
    }

    #[test]
    fn meets_gate_separates_owned_foreign_and_moved() {
        let derived = derived_local("/usr/share/rhesis");
        let ours = Conf {
            flavor: InstallationFlavor::LinuxLocal,
            translations: derived.translations_dir.clone().unwrap(),
            languagetool: derived.languagetool_dir.clone().unwrap(),
            java: derived.java.clone(),
            autostart: None,
        };
        assert!(ours.meets(&derived), "exact match reads as ours");

        // Moved: same flavor, different paths.
        let moved = Conf {
            languagetool: PathBuf::from("/elsewhere/rhesis/LanguageTool"),
            ..ours.clone()
        };
        assert!(!moved.meets(&derived));

        // Foreign flavor (an AppImage conf read by a local install).
        let foreign = Conf {
            flavor: InstallationFlavor::LinuxAppImage,
            ..ours.clone()
        };
        assert!(!foreign.meets(&derived));
    }

    #[test]
    fn dev_translations_fall_back_to_the_working_directory() {
        let root = std::env::temp_dir().join("rhesis-dev-translations-test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("translations")).expect("scratch translations dir creatable");
        assert_eq!(dev_translations_dir(&root), Some(root.join("translations")));

        let _ = fs::remove_dir_all(&root);
        assert_eq!(dev_translations_dir(&root), None, "no folder, no fallback");
    }
}
