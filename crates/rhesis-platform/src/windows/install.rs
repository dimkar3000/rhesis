//! The Windows install identity: mirrors the Linux module — one per-user
//! configuration file (`install.conf` beside the daemon's settings store)
//! carrying the flavor and the resolved payload locations. Both the
//! installer and the portable zip keep everything next to the executable,
//! so the seed derivation reads no environment at all.

use std::fmt::Write as _;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

use super::paths;

/// Name of the configuration file inside [`paths::config_dir`].
const FILE_NAME: &str = "install.conf";

/// Config file keys. Unknown keys are left alone.
const KEY_FLAVOR: &str = "flavor";
const KEY_TRANSLATIONS: &str = "translations";
const KEY_LANGUAGETOOL: &str = "languagetool";
const KEY_JAVA: &str = "java";
const KEY_AUTOSTART: &str = "autostart-exec";

/// The single Windows payload flavor: whether a portable zip or the Inno
/// Setup installation produced it, the layout is the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flavor {
    Windows,
}

impl Flavor {
    pub fn name(self) -> &'static str {
        match self {
            Flavor::Windows => "windows",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Flavor> {
        match value {
            "windows" => Some(Flavor::Windows),
            _ => None,
        }
    }
}

/// Everything the running process needs from its installation after one
/// successful resolution.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub flavor: Flavor,
    /// Directory of the compiled `rhesis_<locale>.qm` catalogs.
    pub translations_dir: Option<PathBuf>,
    /// LanguageTool payload directory (`languagetool-server.jar` lives
    /// here).
    pub languagetool_dir: Option<PathBuf>,
    /// Java executable the LanguageTool server is started with.
    pub java: PathBuf,
    /// The Run-value line (with ` --hidden`) recorded for the autostart
    /// entry, when the on-disk configuration describes this very copy —
    /// a record written by another copy never leaks.
    pub autostart_record: Option<String>,
}

/// Record (replace) the autostart Run-value line in the configuration.
/// Only the conf of this running copy is touched.
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
    if let Some(mut conf) = load(&config_root) {
        if conf.meets(&derived) {
            conf.autostart = Some(line.to_string());
            if let Err(e) = save(&config_root, &conf) {
                log::error!("failed to record the autostart run value: {e:?}");
            }
        }
    }
}

/// Client entry point: resolve the install and keep the configuration on
/// disk in sync with it (seed on first run, write back when the payload
/// moved). Dev builds (`cargo run` / `cargo test`) resolve purely and
/// never touch the file.
pub fn ensure() -> Option<Resolved> {
    resolve(true)
}

/// Daemon entry point: resolve read-only. Same resolution as [`ensure`],
/// never writing the configuration file.
pub fn read_only() -> Option<Resolved> {
    resolve(false)
}

/// The payload root of this running copy — the directory holding
/// `rhesis.exe`, `rhesis-service.exe`, `jre/`, `LanguageTool/` — for the
/// in-binary uninstall. `None` in dev runs (nothing installed).
pub fn payload_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    super::process::exe_dir()
}

/// One file-level alias: `install.conf`'s location inside the app
/// configuration tree.
pub fn path() -> Option<PathBuf> {
    paths::config_dir().map(|dir| conf_path(&dir))
}

/// Pure derivation of where a layout keeps everything, before any file is
/// consulted.
pub(crate) struct Derived {
    pub flavor: Flavor,
    pub translations_dir: Option<PathBuf>,
    pub languagetool_dir: Option<PathBuf>,
    pub java: PathBuf,
}

impl From<Derived> for Resolved {
    fn from(d: Derived) -> Resolved {
        Resolved {
            flavor: d.flavor,
            translations_dir: d.translations_dir,
            languagetool_dir: d.languagetool_dir,
            java: d.java,
            autostart_record: None,
        }
    }
}

/// One parsed configuration file.
#[derive(Clone)]
pub(crate) struct Conf {
    pub flavor: Flavor,
    pub translations: PathBuf,
    pub languagetool: PathBuf,
    pub java: PathBuf,
    pub autostart: Option<String>,
}

impl Conf {
    /// Whether the file describes exactly this running copy.
    fn meets(&self, derived: &Derived) -> bool {
        self.flavor == derived.flavor
            && Some(&self.translations) == derived.translations_dir.as_ref()
            && Some(&self.languagetool) == derived.languagetool_dir.as_ref()
            && self.java == derived.java
    }
}

/// Strict parse: all four location keys must be present; a missing one, an
/// unknown flavor value or a line without `=` makes the whole file
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
            KEY_FLAVOR => flavor = Flavor::parse(value),
            KEY_TRANSLATIONS => translations = non_empty(value).map(PathBuf::from),
            KEY_LANGUAGETOOL => languagetool = non_empty(value).map(PathBuf::from),
            KEY_JAVA => java = non_empty(value).map(PathBuf::from),
            KEY_AUTOSTART => autostart = non_empty(value),
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

pub(crate) fn serialize(conf: &Conf) -> String {
    let mut out = String::new();
    out.push_str("# Rhesis install configuration; maintained by the app.\n");

    let _ = writeln!(out, "{KEY_FLAVOR}={}", conf.flavor.name());
    let _ = writeln!(out, "{KEY_TRANSLATIONS}={}", conf.translations.display());
    let _ = writeln!(out, "{KEY_LANGUAGETOOL}={}", conf.languagetool.display());
    let _ = writeln!(out, "{KEY_JAVA}={}", conf.java.display());
    if let Some(line) = &conf.autostart {
        let _ = writeln!(out, "{KEY_AUTOSTART}={line}");
    }
    out
}

/// Pure derivation for the Windows payload: the executable dir is the
/// install root itself (both the Inno Setup layout and the portable zip),
/// so everything is a fixed join from it.
pub(crate) fn derive_with(exe_dir: &Path) -> Derived {
    Derived {
        flavor: Flavor::Windows,
        translations_dir: Some(exe_dir.join("translations")),
        languagetool_dir: Some(exe_dir.join("LanguageTool")),
        java: exe_dir.join("jre").join("bin").join("java.exe"),
    }
}

fn conf_path(config_root: &Path) -> PathBuf {
    config_root.join(FILE_NAME)
}

fn load(config_root: &Path) -> Option<Conf> {
    let text = fs::read_to_string(conf_path(config_root)).ok()?;
    parse(&text)
}

fn save(config_root: &Path, conf: &Conf) -> Result<(), std::io::Error> {
    fs::create_dir_all(config_root)?;
    fs::write(conf_path(config_root), serialize(conf))
}

fn resolve(writes: bool) -> Option<Resolved> {
    // Dev runs read their locations from `dev-install.conf` beside the
    // working directory instead of probing the filesystem; the file is
    // never written by dev builds.
    if cfg!(debug_assertions) {
        return Some(dev_config());
    }

    let exe_dir = env::current_exe().ok()?.parent()?.to_path_buf();
    let derived = derive(&exe_dir);

    let Some(config_root) = paths::config_dir() else {
        log::warn!("APPDATA is not set; resolving paths without persisting");
        return Some(Resolved::from(derived));
    };

    match load(&config_root) {
        // Exactly this copy: the read path, autostart record included.
        Some(conf) if conf.meets(&derived) => Some(Resolved {
            flavor: conf.flavor,
            translations_dir: Some(conf.translations),
            languagetool_dir: Some(conf.languagetool),
            java: conf.java,
            autostart_record: conf.autostart,
        }),
        // One flavor on Windows; a missing recorded LanguageTool location
        // means the payload moved or was re-extracted elsewhere — write
        // the fresh derivation back and keep the autostart record.
        Some(conf) if !conf.languagetool.is_dir() => {
            let record = conf.autostart;
            if writes {
                if let Some(conf) = derived_conf(&derived, record.clone()) {
                    if let Err(e) = save(&config_root, &conf) {
                        log::error!("failed to write back the moved install: {e:?}");
                    }
                }
            }
            Some(Resolved {
                flavor: derived.flavor,
                translations_dir: derived.translations_dir,
                languagetool_dir: derived.languagetool_dir,
                java: derived.java,
                autostart_record: record,
            })
        }
        // A sibling copy lives where the file points (installer + portable
        // coexisting): leave the file alone, serve this process from the
        // derivation, no record leaks.
        Some(_) => Some(Resolved::from(derived)),
        // Nothing on disk yet: seed it.
        None => {
            if writes {
                if let Some(conf) = derived_conf(&derived, None) {
                    if let Err(e) = save(&config_root, &conf) {
                        log::error!("failed to seed the install configuration: {e:?}");
                    }
                }
            }
            Some(Resolved::from(derived))
        }
    }
}

fn derived_conf(derived: &Derived, autostart: Option<String>) -> Option<Conf> {
    Some(Conf {
        flavor: derived.flavor,
        translations: derived.translations_dir.clone()?,
        languagetool: derived.languagetool_dir.clone()?,
        java: derived.java.clone(),
        autostart,
    })
}

/// Dev builds read their locations from `dev-install.conf` beside the
/// working directory instead of probing the filesystem. Missing keys
/// degrade: translations/languagetool to `None`, java to bare `java` from
/// PATH. A missing file yields the same defaults.
fn dev_config() -> Resolved {
    let Ok(dir) = std::env::current_dir() else {
        return dev_defaults();
    };
    let Ok(text) = fs::read_to_string(dir.join("dev-install.conf")) else {
        return dev_defaults();
    };
    parse_dev_conf(&text)
}

fn dev_defaults() -> Resolved {
    Resolved {
        flavor: Flavor::Windows,
        translations_dir: None,
        languagetool_dir: None,
        java: PathBuf::from("java"),
        autostart_record: None,
    }
}

fn parse_dev_conf(text: &str) -> Resolved {
    let mut translations_dir = None;
    let mut languagetool_dir = None;
    let mut java = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            KEY_TRANSLATIONS => translations_dir = non_empty(value).map(PathBuf::from),
            KEY_LANGUAGETOOL => languagetool_dir = non_empty(value).map(PathBuf::from),
            KEY_JAVA => java = non_empty(value).map(PathBuf::from),
            _ => {}
        }
    }
    Resolved {
        flavor: Flavor::Windows,
        translations_dir,
        languagetool_dir,
        java: java.unwrap_or_else(|| PathBuf::from("java")),
        autostart_record: None,
    }
}

/// Derivation for the running process (no environment injection needed on
/// Windows).
fn derive(exe_dir: &Path) -> Derived {
    derive_with(exe_dir)
}

fn non_empty(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn derived(root: &str) -> Derived {
        derive_with(Path::new(root))
    }

    #[test]
    fn conf_round_trips() {
        let conf = Conf {
            flavor: Flavor::Windows,
            translations: PathBuf::from(r"C:\Rhesis\translations"),
            languagetool: PathBuf::from(r"C:\Rhesis\LanguageTool"),
            java: PathBuf::from(r"C:\Rhesis\jre\bin\java.exe"),
            autostart: Some(r#""C:\Rhesis\rhesis.exe" --hidden"#.to_string()),
        };
        let parsed = parse(&serialize(&conf)).expect("our own file parses back");
        assert_eq!(parsed.flavor, conf.flavor);
        assert_eq!(parsed.translations, conf.translations);
        assert_eq!(parsed.languagetool, conf.languagetool);
        assert_eq!(parsed.java, conf.java);
        assert_eq!(
            parsed.autostart.as_deref(),
            Some(r#""C:\Rhesis\rhesis.exe" --hidden"#)
        );
    }

    #[test]
    fn incomplete_or_garbage_files_are_unreadable() {
        let partial = "flavor=windows\ntranslations=C:\\x\nlanguagetool=C:\\y\n";
        assert!(
            parse(partial).is_none(),
            "missing java makes the file unreadable"
        );

        assert!(
            parse("flavor=linux-local\n").is_none(),
            "a foreign flavor is unreadable here"
        );

        // Comments and unknown keys are tolerated.
        let with_extras = "# header\n; note\nunknown=1\nflavor=windows\ntranslations=C:\\x\nlanguagetool=C:\\y\njava=C:\\j\n";
        assert!(parse(with_extras).is_some());
    }

    #[test]
    fn derivation_matches_the_payload_layout() {
        let d = derived(r"C:\Program Files\Rhesis");
        assert_eq!(
            d.translations_dir.as_deref(),
            Some(Path::new(r"C:\Program Files\Rhesis\translations"))
        );
        assert_eq!(
            d.languagetool_dir.as_deref(),
            Some(Path::new(r"C:\Program Files\Rhesis\LanguageTool"))
        );
        assert_eq!(
            d.java,
            PathBuf::from(r"C:\Program Files\Rhesis\jre\bin\java.exe")
        );
    }

    #[test]
    fn meets_gate_separates_owned_moved_and_other_copy() {
        let derived = derived(r"C:\Rhesis");
        let ours = Conf {
            flavor: Flavor::Windows,
            translations: derived.translations_dir.clone().unwrap(),
            languagetool: derived.languagetool_dir.clone().unwrap(),
            java: derived.java.clone(),
            autostart: None,
        };
        assert!(ours.meets(&derived));

        let moved = Conf {
            languagetool: PathBuf::from(r"D:\Elsewhere\LanguageTool"),
            ..ours.clone()
        };
        assert!(!moved.meets(&derived));
    }
}
