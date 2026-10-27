//! Autostart at login.
//!
//! Linux: an XDG autostart desktop entry in `~/.config/autostart/`. The
//! `Exec` line adapts to the install flavor (flatpak, AppImage, native) so
//! the entry survives across distribution methods and passes `--hidden` so
//! the window never flashes at login (see `appHideRootWindow` in helper.h).
//!
//! Windows (design, not yet implemented): a `HKCU\Software\Microsoft\
//! Windows\CurrentVersion\Run` value pointing at the executable, guarded by
//! the same `is_enabled`/`set_enabled` interface — the QML toggle can be
//! wired without further changes.

use std::fs;
use std::path::PathBuf;

/// Flatpak application id: also doubles as the desktop entry name, which is
/// what makes XDG autostart entries idempotent across package flavors.
const APP_ID: &str = "io.github.dimkar3000.rhesis";

/// CLI flag launching into the tray without flashing the window; ignored
/// when background hosting is disabled (helper.h keeps the window visible
/// so the app can never strand itself without a tray icon).
const HIDDEN_FLAG: &str = "--hidden";

/// Descriptive error for the QML toggle; details are logged where they occur.
const SET_ERROR_MESSAGE: &str = "could not write the autostart entry";

#[cfg(not(windows))]
fn autostart_dir() -> Option<PathBuf> {
    // XDG_CONFIG_HOME is honored by the desktop environments themselves;
    // empty (unset in some flatpak setups) falls back to $HOME/.config.
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir));
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
}

#[cfg(not(windows))]
fn autostart_file() -> Option<PathBuf> {
    Some(autostart_dir()?.join(format!("{APP_ID}.desktop")))
}

/// Desktop files quote `Exec` arguments with `"`; a path with embedded
/// quotes never comes from our install flavors, so plain quoting is enough.
fn exec_escape(path: &str) -> String {
    if path.contains(' ') {
        format!("\"{path}\" {HIDDEN_FLAG}")
    } else {
        format!("{path} {HIDDEN_FLAG}")
    }
}

/// Build the `Exec` line for the current install flavor. Flatpak is detected
/// from the in-sandbox marker file; AppImage exposes the original image path
/// in `$APPIMAGE` (the extracted mount point from `current_exe()` is wiped at
/// exit and would go stale between sessions).
#[cfg(not(windows))]
fn exec_command() -> String {
    if PathBuf::from("/.flatpak-info").exists() {
        return format!("flatpak run {APP_ID} {HIDDEN_FLAG}");
    }

    if let Ok(appimage) = std::env::var("APPIMAGE") {
        if !appimage.is_empty() {
            return exec_escape(&appimage);
        }
    }

    match std::env::current_exe() {
        Ok(exe) => {
            let path = exe.to_string_lossy().to_string();
            if path.is_empty() {
                format!("{APP_ID} {HIDDEN_FLAG}")
            } else {
                exec_escape(&path)
            }
        }
        Err(_) => format!("{APP_ID} {HIDDEN_FLAG}"),
    }
}

#[cfg(not(windows))]
fn entry_contents() -> String {
    // StartupNotify=false is the freedesktop standard (honored by GNOME,
    // Cinnamon/Mint, MATE, XFCE, LXQt); X-KDE-StartupNotify rides along for
    // KDE's older lookup path. Each entry shows two things: never showing a
    // spinner waiting for a hidden-to-tray launch.
    format!(
        "[Desktop Entry]\n\
         Type=Application\n\
         Name=Rhesis\n\
         Exec={}\n\
         Icon={APP_ID}\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-KDE-StartupNotify=false\n",
        exec_command()
    )
}

/// Only entries written by this module count as enabled: the header, the
/// display name and the `--hidden` flag are all values we control, which
/// keeps a hand-edited entry pointing elsewhere from confusing the toggle.
#[cfg(not(windows))]
fn is_our_entry(contents: &str) -> bool {
    contents.starts_with("[Desktop Entry]")
        && contents.contains("\nName=Rhesis\n")
        && contents.contains(HIDDEN_FLAG)
}

#[cfg(not(windows))]
pub fn is_enabled() -> bool {
    autostart_file()
        .map(|file| {
            fs::read_to_string(&file)
                .map(|contents| is_our_entry(&contents))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

#[cfg(not(windows))]
pub fn set_enabled(enable: bool) -> Result<(), &'static str> {
    let file = autostart_file().ok_or(SET_ERROR_MESSAGE)?;

    if !enable {
        if let Err(e) = fs::remove_file(&file) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::error!("failed to remove autostart entry at {file:?}: {e:?}");
                return Err(SET_ERROR_MESSAGE);
            }
        }
        return Ok(());
    }

    let dir = file.parent().expect("autostart file always has a parent");
    if let Err(e) = fs::create_dir_all(dir) {
        log::error!("failed to create autostart directory {dir:?}: {e:?}");
        return Err(SET_ERROR_MESSAGE);
    }
    if let Err(e) = fs::write(&file, entry_contents()) {
        log::error!("failed to write autostart entry {file:?}: {e:?}");
        return Err(SET_ERROR_MESSAGE);
    }
    Ok(())
}

// Windows: the registry Run key keeps the same interface so the QML toggle
// needs no changes when it gets implemented (needs the `winreg` crate or
// direct registry calls; the value would be the executable plus --hidden and
// only makes the app launch itself exactly like the desktop entry does).
#[cfg(windows)]
pub fn is_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_enabled(_enable: bool) -> Result<(), &'static str> {
    Err("autostart on Windows is not implemented yet")
}
