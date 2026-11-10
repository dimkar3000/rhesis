use std::path::PathBuf;

/// Directory holding the compiled `rhesis_<locale>.qm` catalogs, taken from
/// this install's `install.conf` via [`install::read_only`].
pub fn translations_dir() -> Option<PathBuf> {
    super::install::read_only().and_then(|setup| setup.translations_dir)
}
