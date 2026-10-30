//! Shared helpers for the E2E tiers: every `tests/e2e/*.rs` target gets them
//! through `mod common;` (the directory is symlinked into each tier).
//!
//! The goal is the same hermeticity the QuickTest runner applies inside the
//! library (`src/ui_tests.rs :: isolate_environment`, the reference copy):
//! all writable XDG homes are redirected into a per-tier sandbox under
//! `build/ui-tests/`, so tests never touch a real user configuration and the
//! fake completion provider answers before anything could spawn a worker.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// Fixture id served to worker requests unless the caller already named one.
pub(crate) const DEFAULT_FAKE_FIXTURE: &str = "checker";

/// Every writable home the process could touch, paired with its sandbox
/// subdirectory name.
const XDG_ROOTS: [(&str, &str); 5] = [
    ("XDG_CONFIG_HOME", "config"),
    ("XDG_DATA_HOME", "data"),
    ("XDG_CACHE_HOME", "cache"),
    ("XDG_RUNTIME_DIR", "runtime"),
    ("HOME", "home"),
];

/// Per-tier sandbox next to the harness one (`name` is the leaf directory,
/// e.g. `boot-env`), so parallel tiers never share state.
pub(crate) fn sandbox_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("build/ui-tests")
        .join(name)
}

/// Redirects every XDG-style home (config, data, caches, sockets, `$HOME`)
/// into a fresh sandbox directory that is truncated at every launch. Must
/// run before Qt (or anything else) resolves its configuration, which the
/// callers guarantee by calling this first in `main`-order boot code.
pub(crate) fn isolate_environment(name: &str) -> Result<(), String> {
    for (variable, subdir) in XDG_ROOTS {
        let directory = sandbox_root(name).join(subdir);
        if let Err(e) = fs::remove_dir_all(&directory) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(format!("cannot truncate {}: {e}", directory.display()));
            }
        }
        fs::create_dir_all(&directory)
            .map_err(|e| format!("cannot create {}: {e}", directory.display()))?;
        // Same restriction mode as XDG_RUNTIME_DIR demands.
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("cannot restrict {}: {e}", directory.display()))?;
        std::env::set_var(variable, &directory);
    }
    Ok(())
}

/// Selects the recorded-checker fixture unless the caller already named one.
/// Worker selection reads the variable when the worker starts, so seeding it
/// before the boot code runs keeps the tier hermetic without touching
/// release behavior.
pub(crate) fn seed_fake_provider() {
    if std::env::var_os("RHESIS_FAKE_LT").is_none() {
        std::env::set_var("RHESIS_FAKE_LT", DEFAULT_FAKE_FIXTURE);
    }
}
