use std::path::PathBuf;

use super::install::InstallationSetup;

/// Directory holding the compiled `rhesis_<locale>.qm` catalogs, taken from
/// this install's `install.conf` via [`InstallationSetup`].
pub fn translations_dir() -> Option<PathBuf> {
    InstallationSetup::read_only().and_then(|setup| setup.translations_dir)
}
