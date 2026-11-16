//! In-binary uninstall (invoked with `rhesis --uninstall`) for the Linux
//! flavors. What is removed is decided by the flavor resolved from
//! `install.conf` / the running binary — no filesystem guessing:
//!
//! - tarball: the whole payload folder, the application-menu entry, the
//!   configuration folder (`install.conf` included) and the autostart
//!   entry that does not belong to the flatpak flavor. Locations needing
//!   root (a global install under /usr) fail with a sudo command for what
//!   is left behind.
//! - AppImage: its autostart entry and, best effort, the image file. The
//!   configuration folder stays — another flavor may still use it.
//! - flatpak: packaged and managed by flatpak itself; the binary only
//!   points at the command that removes it.

use std::{env, fs, path::PathBuf};

use super::constants::APP_ID;
use super::install::{self, InstallationFlavor, InstallationSetup};
use super::{autostart, paths};

/// Remove this install per its flavor, or fail with a user-facing message
/// (development builds have nothing installed).
pub fn uninstall() -> Result<String, String> {
    let Some(resolved) = InstallationSetup::read_only() else {
        return Err("could not determine this install's location; nothing was removed".to_string());
    };
    match resolved.flavor {
        InstallationFlavor::LinuxFlatpak => uninstall_flatpak(),
        InstallationFlavor::LinuxAppImage => uninstall_appimage(),
        InstallationFlavor::LinuxLocal => uninstall_local(&resolved.flavor),
    }
}

fn uninstall_flatpak() -> Result<String, String> {
    Ok(format!(
        "the application is managed by flatpak; nothing to remove here. \
         Run `flatpak uninstall {APP_ID}` to delete it \
         (add --delete-data to remove its settings too)"
    ))
}

fn uninstall_appimage() -> Result<String, String> {
    let entry_removed = autostart::uninstall_entry();
    let entry = if entry_removed {
        "the autostart entry"
    } else {
        "nothing else"
    };

    let image = env::var("APPIMAGE").ok().filter(|p| !p.is_empty());
    match image {
        Some(image) => match fs::remove_file(&image) {
            Ok(()) => Ok(format!("removed the AppImage and {entry}")),
            Err(e) => {
                let reason = format!("the AppImage {image} could not be deleted: {e}");
                if entry_removed {
                    Ok(format!("removed {entry}; {reason}"))
                } else {
                    Ok(reason)
                }
            }
        },
        None => {
            if entry_removed {
                Ok(format!("removed {entry}; the AppImage file itself could not be located — delete it manually"))
            } else {
                Ok("nothing found to remove; the AppImage file could not be located".to_string())
            }
        }
    }
}

fn uninstall_local(flavor: &InstallationFlavor) -> Result<String, String> {
    let Some(payload) = install::payload_root() else {
        return Err("development build; nothing is installed to remove".to_string());
    };

    let mut removed: Vec<String> = Vec::new();
    let mut needs_root: Vec<String> = Vec::new();

    if autostart::uninstall_entry() {
        removed.push("the autostart entry".to_string());
    }

    // The application-menu entry, wherever an install (this one or an
    // earlier one) wrote it — same two locations the installer uses.
    // Flatpak and AppImage own their own desktop entries; only the local
    // tarball flavor writes to the user and system app dirs.
    let mut desktop_removed = false;
    for path in apps_desktop_files(flavor) {
        match fs::remove_file(&path) {
            Ok(()) => desktop_removed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                needs_root.push(path.display().to_string())
            }
            Err(e) => log::warn!("could not remove the application-menu entry {path:?}: {e:?}"),
        }
    }
    if desktop_removed {
        removed.push("the application-menu entry".to_string());
    }

    // The payload folder: binaries, JRE, LanguageTool, translations. The
    // running daemon holds no locks that block unlinking on Linux.
    match fs::remove_dir_all(&payload) {
        Ok(()) => removed.push("the application files".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            needs_root.push(payload.display().to_string())
        }
        Err(e) => log::warn!("could not remove the application folder {payload:?}: {e:?}"),
    }

    // The configuration folder: install.conf and the daemon's settings
    // store (an old install's leftover QSettings INI goes with the folder).
    if let Some(config) = paths::config_dir() {
        match fs::remove_dir_all(&config) {
            Ok(()) => removed.push("the configuration".to_string()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                needs_root.push(config.display().to_string())
            }
            Err(e) => log::warn!("could not remove the configuration folder {config:?}: {e:?}"),
        }
    }

    if !needs_root.is_empty() {
        let commands = needs_root
            .iter()
            .map(|path| format!("  sudo rm -rf '{path}'"))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(format!(
            "these locations are outside your account and need root \
             (run to finish the uninstall):\n{commands}"
        ));
    }

    if removed.is_empty() {
        return Ok("nothing found to remove".to_string());
    }
    Ok(format!("removed {}", removed.join(", ")))
}

/// The application-menu entry locations the installer can have written,
/// based on the install flavor. Flatpak and AppImage own their own desktop
/// entries, so only the local tarball flavor writes to the user and system
/// app dirs; a stale global entry from an earlier install is part of the
/// cleanup too.
fn apps_desktop_files(flavor: &InstallationFlavor) -> Vec<PathBuf> {
    let entry = format!("{APP_ID}.desktop");
    let mut paths = Vec::new();
    match flavor {
        InstallationFlavor::LinuxLocal => {
            if let Some(home) = env::var_os("HOME") {
                paths.push(
                    PathBuf::from(home)
                        .join(".local/share/applications")
                        .join(&entry),
                );
            }
            paths.push(PathBuf::from("/usr/share/applications").join(entry));
        }
        InstallationFlavor::LinuxFlatpak | InstallationFlavor::LinuxAppImage => {}
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::Path;
    use std::sync::{MutexGuard, PoisonError};

    /// The dev guard: nothing installed means the local uninstall refuses
    /// instead of deleting the development working tree.
    #[test]
    fn development_run_refuses_to_uninstall() {
        // payload_root() is None in dev builds, which is what gates this.
        if install::payload_root().is_none() {
            assert!(uninstall_local(&InstallationFlavor::LinuxLocal)
                .unwrap_err()
                .starts_with("development build"));
        }
    }

    // The AppImage path pins the same process-global variables as the
    // autostart tests, so it takes the shared lock.
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        xdg: Option<OsString>,
        appimage: Option<OsString>,
    }

    impl EnvGuard {
        fn pin(root: &Path, image: &Path) -> Self {
            let lock = autostart::ENV_LOCK
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let xdg = env::var_os("XDG_CONFIG_HOME");
            let appimage = env::var_os("APPIMAGE");
            env::set_var("XDG_CONFIG_HOME", root);
            env::set_var("APPIMAGE", image);
            Self {
                _lock: lock,
                xdg,
                appimage,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.xdg.take() {
                Some(v) => env::set_var("XDG_CONFIG_HOME", v),
                None => env::remove_var("XDG_CONFIG_HOME"),
            }
            match self.appimage.take() {
                Some(v) => env::set_var("APPIMAGE", v),
                None => env::remove_var("APPIMAGE"),
            }
        }
    }

    #[test]
    fn appimage_uninstall_removes_entry_and_image() {
        let root = std::env::temp_dir().join("rhesis-uninstall-appimage");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("autostart")).expect("scratch autostart dir creatable");
        let image = root.join("Rhesis.AppImage");
        fs::write(&image, b"image").expect("scratch image creatable");
        let entry = root.join("autostart").join(format!("{APP_ID}.desktop"));
        fs::write(
            &entry,
            format!("[Desktop Entry]\nExec={}\n", image.display()),
        )
        .expect("scratch entry creatable");

        let _env = EnvGuard::pin(&root, &image);

        let message = uninstall_appimage().expect("the AppImage uninstall succeeds");
        assert!(message.contains("removed the AppImage"), "{message}");
        assert!(message.contains("the autostart entry"), "{message}");
        assert!(!image.exists());
        assert!(!entry.exists());
    }
}
