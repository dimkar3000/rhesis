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
use std::path::{Path, PathBuf};

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
    // XDG autostart entries are read from the "autostart" folder one level
    // below the config dir — not the config dir itself.
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("autostart"));
        }
    }
    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config"))
        .map(|config| config.join("autostart"))
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

/// Derive the native install launcher path from the payload executable.
/// The launcher sits two levels above the rhesis folder:
/// /usr/share/rhesis/bin/rhesis → /usr/bin/rhesis and
/// ~/.local/share/rhesis/bin/rhesis → ~/.local/bin/rhesis. The launcher
/// prepends the install bin/ to PATH so the embedded server can find the
/// bundled java/fasttext — a payload execed directly never gets that PATH.
/// Only returned when the launcher actually exists (dev runs fall through).
#[cfg(not(windows))]
fn launcher_for(exe: &Path) -> Option<String> {
    let rhesis_dir = exe.parent()?.parent()?; // <base>/rhesis
    let base = rhesis_dir.parent()?; // e.g. /usr/share or ~/.local/share
    let launcher = base.parent()?.join("bin").join("rhesis"); // e.g. /usr/bin/rhesis
    launcher
        .is_file()
        .then(|| launcher.to_string_lossy().to_string())
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
            // Prefer the sibling launcher on native installs — see
            // launcher_for for why the payload alone is not self-sufficient.
            let path = launcher_for(&exe).unwrap_or_else(|| exe.to_string_lossy().to_string());
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
/// display name, and an `Exec` line matching what the current install
/// flavor writes. The Exec check is the cross-flavor guard: an entry left
/// by another flavor (e.g. a native launcher path that the uninstall then
/// removed) would otherwise read as ON while systemd fails it at every
/// login with 203/EXEC — with the check the toggle shows off, and turning
/// it on rewrites the line for this flavor.
#[cfg(not(windows))]
fn is_our_entry(contents: &str) -> bool {
    contents.starts_with("[Desktop Entry]")
        && contents.contains("\nName=Rhesis\n")
        && contents.contains(&format!("\nExec={}", exec_command()))
}

/// The single `Exec=` line of a desktop entry, without its field prefix.
#[cfg(not(windows))]
fn exec_line(contents: &str) -> Option<&str> {
    contents.lines().find_map(|line| line.strip_prefix("Exec="))
}

/// An Exec line that autostarts some Rhesis AppImage under `--hidden`: an
/// autostart intent written by a previous AppImage launch. Only the path
/// being stale makes it wrong — cases: the file moved, Gear Lever relocated
/// it, or a version-bumping rename. Case-insensitive because nothing
/// guarantees the filename casing; arbitrarily renamed images (no "rhesis"
/// in the name at all) cannot be recognized, in which case the entry stays
/// stale-but-honest (the toggle reads off and re-enabling rewrites it).
#[cfg(not(windows))]
fn is_appimage_exec(exec: &str) -> bool {
    let Some(path) = exec.strip_suffix(&format!(" {HIDDEN_FLAG}")) else {
        return false;
    };
    let path = path.trim_matches('"');
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".appimage") && lower.contains("rhesis")
}

/// Re-point the autostart entry at the AppImage currently running — its fix
/// for the one Exec flavor that can go stale under us, because the filename
/// carries the version and the file moves freely (unlike a package- or
/// AppImage-runtime-managed install). Returns whether the file changed.
///
/// AppImage-only and launch-only: a package launch (`flatpak run ...`,
/// launcher) never stales; the login launch of a stale entry never gets far
/// enough to heal itself (systemd fails with 203/EXEC before our code
/// runs), so the entry refreshes on the next *manual* launch from the new
/// location instead.
#[cfg(not(windows))]
pub fn self_heal() -> bool {
    // Only the actual $APPIMAGE path is a heal target. Falling through to
    // current_exe() would bake the ephemeral FUSE mount path
    // (/tmp/.mount_*/...) into the entry — an instance-ephemeral path that
    // would only set up the next 203/EXEC.
    let Some(path) = std::env::var("APPIMAGE").ok().filter(|p| !p.is_empty()) else {
        return false;
    };
    let Some(file) = autostart_file() else {
        return false;
    };
    heal_appimage_entry(&file, &exec_escape(&path))
}

#[cfg(not(windows))]
fn heal_appimage_entry(file: &Path, current_exec: &str) -> bool {
    let Ok(contents) = fs::read_to_string(file) else {
        return false;
    };
    if is_our_entry(&contents) {
        return false; // already correct — nothing to heal
    }
    let Some(exec) = exec_line(&contents) else {
        return false; // not a form we wrote
    };
    if !is_appimage_exec(exec) || exec == current_exec {
        return false;
    }
    match fs::write(file, entry_contents()) {
        Ok(()) => {
            log::info!("repointed a stale AppImage autostart Exec to the running image");
            true
        }
        Err(e) => {
            log::error!("failed to repoint the stale AppImage autostart entry: {e:?}");
            false
        }
    }
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

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn entry_lands_in_the_autostart_subdirectory() {
        // The misplaced-entry regression: the filename was joined directly
        // onto the config dir, so the session tray never saw the entry.
        let file = autostart_file();
        if let Some(file) = file {
            assert_eq!(
                file.parent().and_then(|p| p.file_name()),
                Some(OsStr::new("autostart")),
                "XDG autostart entries live below the config dir, not in it"
            );
            assert_eq!(
                file.file_name().and_then(|n| n.to_str()),
                Some(format!("{APP_ID}.desktop").as_str())
            );
        }
        // None on platforms without HOME — nothing further to assert.
    }

    #[test]
    fn native_flavors_hand_over_to_the_sibling_launcher() {
        let scratch = std::env::temp_dir().join("rhesis-autostart-launcher-test");
        let _ = fs::remove_dir_all(&scratch);

        // Local layout: payload at .local/share/rhesis/bin/rhesis →
        // launcher at .local/bin/rhesis.
        let exe = scratch.join("home/dimkar3000/.local/share/rhesis/bin/rhesis");
        let launcher = scratch.join("home/dimkar3000/.local/bin/rhesis");
        fs::create_dir_all(launcher.parent().expect("launcher has a parent"))
            .expect("scratch dirs are creatable");
        fs::write(&launcher, b"#!/bin/sh\n").expect("scratch launcher writable");
        assert_eq!(
            launcher_for(Path::new(&exe)),
            Some(launcher.to_string_lossy().to_string())
        );

        // Global layout: /usr/share/rhesis/bin/rhesis → /usr/bin/rhesis.
        let global_exe = scratch.join("usr/share/rhesis/bin/rhesis");
        let global_launcher = scratch.join("usr/bin/rhesis");
        fs::create_dir_all(global_launcher.parent().expect("launcher has a parent"))
            .expect("scratch dirs are creatable");
        fs::write(&global_launcher, b"#!/bin/sh\n").expect("scratch launcher writable");
        assert_eq!(
            launcher_for(Path::new(&global_exe)),
            Some(global_launcher.to_string_lossy().to_string())
        );

        // Dev run (no launcher in the derived location) falls through.
        assert!(launcher_for(Path::new(&scratch.join("build/target/debug/rhesis"))).is_none());

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn exec_escape_quotes_paths_with_spaces() {
        assert_eq!(
            exec_escape("/opt/No Spaces/rhesis"),
            format!("\"/opt/No Spaces/rhesis\" {HIDDEN_FLAG}")
        );
        assert_eq!(
            exec_escape("/opt/rhesis"),
            format!("/opt/rhesis {HIDDEN_FLAG}")
        );
    }

    #[test]
    fn current_flavor_entries_count_as_ours() {
        assert!(is_our_entry(&entry_contents()));
    }

    #[test]
    fn entries_from_another_flavor_count_as_off() {
        // Cross-flavor regression: a stale entry written by one flavor (e.g.
        // a native launcher path that the uninstall then removed) must not
        // read as ON in another flavor — systemd fails it at login with
        // 203/EXEC. With markers matching but Exec different, the toggle
        // reads off so enabling it rewrites the line for this flavor.
        let stale_native = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\n\
             Exec=/gone/.local/bin/rhesis {HIDDEN_FLAG}\nIcon={APP_ID}\n"
        );
        assert!(!is_our_entry(&stale_native));

        // Not-ours entries without the hidden flag stay rejected too.
        assert!(!is_our_entry("[Desktop Entry]\nName=Rhesis\nExec=rhesis\n"));
    }

    #[test]
    fn appimage_exec_lines_are_recognized() {
        // Both quoting forms our writer produces.
        assert!(is_appimage_exec(&format!(
            "/apps/rhesis-0.5.0-x86_64.AppImage {HIDDEN_FLAG}"
        )));
        assert!(is_appimage_exec(&format!(
            "\"/apps/My Images/rhesis-x86_64.AppImage\" {HIDDEN_FLAG}"
        )));

        // Case-insensitivity: the file was moved with different casing.
        assert!(is_appimage_exec(&format!(
            "/apps/Rhesis.AppImage {HIDDEN_FLAG}"
        )));

        // Not an AppImage (a native launcher or flatpak exec): the heal
        // must never rewrite entries of other flavors — those go through
        // the honest-toggle path instead.
        assert!(!is_appimage_exec(&format!(
            "/gone/.local/bin/rhesis {HIDDEN_FLAG}"
        )));
        assert!(!is_appimage_exec(&format!(
            "flatpak run {APP_ID} {HIDDEN_FLAG}"
        )));

        // Autostart intent missing — not a form we wrote (no hidden flag).
        assert!(!is_appimage_exec("/apps/rhesis.AppImage"));

        // Arbitrary rename with no rhesis string left in the filename is
        // unrecognized by design (stays stale-but-honest).
        assert!(!is_appimage_exec(&format!(
            "/apps/myapp.AppImage {HIDDEN_FLAG}"
        )));
    }

    #[test]
    fn heal_repoints_a_stale_appimage_entry() {
        let scratch = std::env::temp_dir().join("rhesis-autostart-heal-test");
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).expect("scratch dir creatable");
        let file = scratch.join("stale.desktop");

        // What a previous AppImage launch wrote before the file moved.
        let stale = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\n\
             Exec=/old/Downloads/rhesis-0.4.0-x86_64.AppImage {HIDDEN_FLAG}\n\
             Icon={APP_ID}\n"
        );
        fs::write(&file, &stale).expect("scratch entry writable");

        let current = "/new/Applications/rhesis-0.5.0-x86_64.AppImage".to_string();
        assert!(
            heal_appimage_entry(&file, &exec_escape(&current)),
            "a stale AppImage entry is a heal target"
        );

        // The heal rewrites the whole entry in the current form, so the
        // toggle reads it as ours again.
        let healed = fs::read_to_string(&file).expect("healed entry readable");
        assert!(is_our_entry(&healed), "healed entry must read as enabled");
        assert_eq!(
            exec_line(&healed),
            Some(exec_line(&entry_contents()).expect("our entry has an Exec line"))
        );

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn heal_leaves_everything_that_is_not_stale_appimage() {
        let scratch = std::env::temp_dir().join("rhesis-autostart-heal-negative-test");
        let _ = fs::remove_dir_all(&scratch);
        fs::create_dir_all(&scratch).expect("scratch dir creatable");

        let current = "/new/Applications/rhesis-0.5.0-x86_64.AppImage".to_string();
        let current_exec = exec_escape(&current);

        // Already pointing at the running image — no rewrite.
        let file = scratch.join("current.desktop");
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\n\
             Exec={current_exec}\nIcon={APP_ID}\n"
        );
        fs::write(&file, &contents).expect("scratch entry writable");
        assert!(!heal_appimage_entry(&file, &current_exec));
        assert_eq!(fs::read_to_string(&file).expect("entry readable"), contents);

        // A stale native entry (different flavor, exe gone) must survive:
        // cross-flavor honesty belongs to the toggle, not the heal.
        let file = scratch.join("native.desktop");
        let contents = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\n\
             Exec=/gone/.local/bin/rhesis {HIDDEN_FLAG}\nIcon={APP_ID}\n"
        );
        fs::write(&file, &contents).expect("scratch entry writable");
        assert!(!heal_appimage_entry(&file, &current_exec));
        assert_eq!(fs::read_to_string(&file).expect("entry readable"), contents);

        // Missing file / missing Exec line are silent no-ops, not errors.
        assert!(!heal_appimage_entry(
            &scratch.join("absent.desktop"),
            &current_exec
        ));
        let file = scratch.join("no-exec.desktop");
        fs::write(&file, "[Desktop Entry]\nType=Application\nName=Rhesis\n").unwrap();
        assert!(!heal_appimage_entry(&file, &current_exec));

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn annotations_of_heal_target_consistency() {
        // The heal decision never races the toggle: any entry that reads as
        // enabled must be a no-op heal target, because both gate on the
        // same is_our_entry.
        let ours = entry_contents();
        let file = std::env::temp_dir().join("rhesis-autostart-heal-consistency.desktop");
        fs::write(&file, &ours).expect("scratch entry writable");
        assert!(is_our_entry(&ours));
        assert!(
            !heal_appimage_entry(&file, "anything --hidden"),
            "an enabled entry must never be rewritten by the heal"
        );
        assert_eq!(fs::read_to_string(&file).expect("entry readable"), ours);
        let _ = fs::remove_file(&file);
    }
}
