//! Autostart at login: an XDG autostart desktop entry in
//! `~/.config/autostart/`. The `Exec` line passes ` --hidden` so the window
//! never flashes at login (see `appHideRootWindow` in helper.h), and the
//! "what did we write" memory lives in `install.conf` (`autostart-exec`):
//! both the composition of a fresh line and the stale-entry heal work
//! against that record instead of guessing entry shapes.

use std::fs;
use std::path::PathBuf;

use super::constants::{APP_ID, HIDDEN_FLAG, SET_ERROR_MESSAGE};

/// Re-point the autostart entry the record says we wrote at the Exec line
/// of the currently running install — the fix for the one Exec flavor that
/// can go stale: a moved or renamed AppImage (or a relocated tarball
/// payload) leaves the entry pointing somewhere that no longer launches at
/// login. Returns whether the file changed.
///
/// Launch-only: the login launch of a stale entry never gets far enough to
/// heal itself (systemd fails with 203/EXEC before our code runs), so the
/// entry refreshes on the next manual launch from the new location
/// instead.
pub fn autostart_self_heal(record: Option<&str>) -> bool {
    let Some(record) = record else {
        return false;
    };
    let current = exec_command();
    if current == record {
        return false; // the record still describes the running install
    }
    let Some(file) = autostart_file() else {
        return false;
    };
    let Ok(contents) = fs::read_to_string(&file) else {
        return false; // absent or unreadable: nothing of ours to fix
    };
    // The entry we wrote for the recorded Exec line — same header, name and
    // exactly the recorded `Exec=`, no shape guessing: the record in
    // `install.conf` says which line is ours.
    let recorded = contents.starts_with("[Desktop Entry]")
        && contents.contains("\nName=Rhesis\n")
        && exec_line(&contents) == Some(record);
    if !recorded {
        return false; // not the entry our record describes
    }
    match fs::write(&file, entry_contents()) {
        Ok(()) => {
            log::info!("repointed the autostart entry at the running install");
            true
        }
        Err(e) => {
            log::error!("failed to repoint the stale autostart entry: {e:?}");
            false
        }
    }
}

/// Remove the autostart entry as part of an uninstall: an entry going
/// through `flatpak run` belongs to the flatpak install, which flatpak
/// itself manages, so it survives; every other form this module could have
/// written now points at a payload the uninstall is deleting, and the login
/// launch would die with systemd 203/EXEC. Returns whether the entry was
/// removed.
pub fn uninstall_entry() -> bool {
    let Some(file) = autostart_file() else {
        return false;
    };
    let Ok(contents) = fs::read_to_string(&file) else {
        return false; // absent: nothing to remove
    };
    let Some(exec) = exec_line(&contents) else {
        return false;
    };
    if exec.trim_start().starts_with("flatpak run") {
        log::info!("the autostart entry belongs to the flatpak install; leaving it");
        return false;
    }
    match fs::remove_file(&file) {
        Ok(()) => {
            log::info!("removed the autostart entry as part of the uninstall");
            true
        }
        Err(e) => {
            log::error!("failed to remove the autostart entry {file:?}: {e:?}");
            false
        }
    }
}

/// Only entries written by this module count as enabled: the header, the
/// display name, and an `Exec` line matching what the current install
/// flavor writes. The Exec check is the cross-flavor guard: an entry left
/// by another flavor (e.g. a native launcher path that the uninstall then
/// removed) would otherwise read as ON while systemd fails it at every
/// login with 203/EXEC — with the check the toggle shows off, and turning
/// it on rewrites the line for this flavor.
pub fn is_enabled() -> bool {
    let Some(file) = autostart_file() else {
        return false;
    };
    let Ok(contents) = fs::read_to_string(file) else {
        return false;
    };
    contents.starts_with("[Desktop Entry]")
        && contents.contains("\nName=Rhesis\n")
        && contents.contains(&format!("\nExec={}", exec_command()))
}

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
    let line = exec_command();
    if let Err(e) = fs::write(&file, entry_contents()) {
        log::error!("failed to write autostart entry {file:?}: {e:?}");
        return Err(SET_ERROR_MESSAGE);
    }
    // The record is what the next launch's autostart_self_heal matches against.
    super::install::record_autostart(&line);
    Ok(())
}

/// The file this install writes: `$XDG_CONFIG_HOME/autostart` when set,
/// otherwise `$HOME/.config/autostart`.
fn autostart_file() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(
                PathBuf::from(dir)
                    .join("autostart")
                    .join(format!("{APP_ID}.desktop")),
            );
        }
    }

    std::env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".config"))
        .map(|config| config.join("autostart"))
        .map(|dir| dir.join(format!("{APP_ID}.desktop")))
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

/// Compose the `Exec` line for the running install from the identities the
/// runtimes inject, in precedence order: `FLATPAK_ID` (the deep sandbox
/// must be re-entered through `flatpak run` at login whatever filesystem
/// the sandbox sees), `APPIMAGE` (the original image path, whose extracted
/// mount point from `current_exe()` is wiped at exit), `RHESIS_LAUNCHER`
/// (the launcher script path exported by rhesis-installer.sh so autostart entries
/// point at the launcher, not the raw binary — this lets uninstall clean up
/// correctly), otherwise the running binary itself — the packaged tarball
/// payload, which resolves all of its payload locations from `install.conf`.
/// With no identity at all, the plain app id is the dev fallback.
fn exec_command() -> String {
    if let Some(id) = std::env::var("FLATPAK_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .as_deref()
    {
        return format!("flatpak run {id} {HIDDEN_FLAG}");
    }
    if let Some(path) = std::env::var("APPIMAGE")
        .ok()
        .filter(|p| !p.is_empty())
        .as_deref()
    {
        return exec_escape(path);
    }
    if let Some(launcher) = std::env::var("RHESIS_LAUNCHER")
        .ok()
        .filter(|p| !p.is_empty())
        .as_deref()
    {
        return exec_escape(launcher);
    }
    match std::env::current_exe().ok().as_deref() {
        Some(exe) if !exe.to_string_lossy().is_empty() => exec_escape(&exe.to_string_lossy()),
        _ => format!("{APP_ID} {HIDDEN_FLAG}"),
    }
}

fn appimage_icon_for_entry() -> String {
    if let Some(_appimage) = std::env::var("APPIMAGE").ok().filter(|p| !p.is_empty()) {
        // Extract icon from AppImage to user icon theme.
        // current_exe() is /tmp/.mount_XXXXXX/app/bin/rhesis, so mount root is 3 levels up.
        if let Ok(exe) = std::env::current_exe() {
            if let Some(mount_root) = exe
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
            {
                // Try the symlink at the mount root first (linuxdeploy puts it there).
                let icon_src = mount_root.join("io.github.dimkar3000.rhesis.png");
                // Fallback: search common icon locations in the mounted AppImage.
                let fallback_paths = [
                    "usr/share/icons/hicolor/512x512/apps/io.github.dimkar3000.rhesis.png",
                    "usr/share/icons/hicolor/256x256/apps/io.github.dimkar3000.rhesis.png",
                    "usr/share/icons/hicolor/128x128/apps/io.github.dimkar3000.rhesis.png",
                ];
                let icon_src = if icon_src.exists() {
                    icon_src
                } else {
                    fallback_paths
                        .iter()
                        .map(|p| mount_root.join(p))
                        .find(|p| p.exists())
                        .unwrap_or(icon_src)
                };
                if icon_src.exists() {
                    if let Some(home) = std::env::var_os("HOME") {
                        let icon_dir =
                            PathBuf::from(home).join(".local/share/icons/hicolor/256x256/apps");
                        let icon_dst = icon_dir.join("io.github.dimkar3000.rhesis.png");
                        let _ = std::fs::create_dir_all(&icon_dir);
                        let _ = std::fs::copy(&icon_src, &icon_dst);
                    }
                }
            }
        }
    }
    APP_ID.to_string()
}

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
         Icon={}\n\
         Terminal=false\n\
         StartupNotify=false\n\
         X-KDE-StartupNotify=false\n",
        exec_command(),
        appimage_icon_for_entry()
    )
}

/// The single `Exec=` line of a desktop entry, without its field prefix.
fn exec_line(contents: &str) -> Option<&str> {
    contents.lines().find_map(|line| line.strip_prefix("Exec="))
}

// Tests share one process, so every test that pins the autostart lookup
// or the flavor identity vars takes this lock; the Sandbox restores the
// previous values when dropped. Cross-module too — the uninstall tests
// sandbox the same variables — hence pub(crate).
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{OsStr, OsString};
    use std::path::Path;
    use std::sync::{MutexGuard, PoisonError};

    struct Sandbox {
        _lock: MutexGuard<'static, ()>,
        xdg: Option<OsString>,
        flatpak: Option<OsString>,
        appimage: Option<OsString>,
    }

    impl Sandbox {
        /// Pin the autostart dir under `root/autostart` and clear the
        /// flatpak/AppImage identities: Exec composes from the running exe.
        fn bare(root: &Path) -> Sandbox {
            let lock = ENV_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
            let xdg = std::env::var_os("XDG_CONFIG_HOME");
            let flatpak = std::env::var_os("FLATPAK_ID");
            let appimage = std::env::var_os("APPIMAGE");
            std::env::set_var("XDG_CONFIG_HOME", root);
            std::env::remove_var("FLATPAK_ID");
            std::env::remove_var("APPIMAGE");
            Sandbox {
                _lock: lock,
                xdg,
                flatpak,
                appimage,
            }
        }
    }

    impl Drop for Sandbox {
        fn drop(&mut self) {
            restore("XDG_CONFIG_HOME", &self.xdg);
            restore("FLATPAK_ID", &self.flatpak);
            restore("APPIMAGE", &self.appimage);
        }
    }

    fn restore(variable: &str, value: &Option<OsString>) {
        match value {
            Some(value) => std::env::set_var(variable, value),
            None => std::env::remove_var(variable),
        }
    }

    /// A fresh per-test root whose `autostart` subdir exists, so tests can
    /// write an entry and read it back through the sandboxed lookup.
    fn sandbox_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("autostart")).expect("scratch autostart dir creatable");
        root
    }

    fn sandbox_entry(root: &Path) -> PathBuf {
        root.join("autostart").join(format!("{APP_ID}.desktop"))
    }

    #[test]
    fn entry_lands_in_the_autostart_subdirectory() {
        // The misplaced-entry regression: the filename was joined directly
        // onto the config dir, so the session tray never saw the entry.
        // The sandbox pins the lookup through XDG_CONFIG_HOME, so the path
        // is fully determined.
        let root = sandbox_root("rhesis-autostart-path-test");
        let file = Sandbox::bare(&root);
        assert_eq!(
            sandbox_entry(&root).parent().and_then(|p| p.file_name()),
            Some(OsStr::new("autostart")),
            "XDG autostart entries live below the config dir, not in it"
        );
        assert_eq!(
            sandbox_entry(&root).file_name().and_then(|n| n.to_str()),
            Some(format!("{APP_ID}.desktop").as_str())
        );
        drop(file);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_running_install_shapes_the_exec_line() {
        // Flatpak: re-entering the sandbox through `flatpak run` is the
        // only way a host login reaches the app.
        let root = sandbox_root("rhesis-autostart-compose-test");
        let sandbox = Sandbox::bare(&root);

        std::env::set_var("FLATPAK_ID", "io.github.dimkar3000.rhesis");
        assert_eq!(
            exec_command(),
            format!("flatpak run io.github.dimkar3000.rhesis {HIDDEN_FLAG}")
        );

        // AppImage: the image path the runtime exports (spaced folder —
        // desktop Exec needs the quoted form).
        std::env::remove_var("FLATPAK_ID");
        std::env::set_var("APPIMAGE", "/apps/My Images/Rhesis-0.6.0-x86_64.AppImage");
        assert_eq!(
            exec_command(),
            format!("\"/apps/My Images/Rhesis-0.6.0-x86_64.AppImage\" {HIDDEN_FLAG}")
        );

        // Tarball install / dev run: the running binary is the entry point
        // (the payload resolves everything else from install.conf). The
        // exe-lookup-failure fallback is not reachable in-process.
        std::env::remove_var("APPIMAGE");
        let exe = std::env::current_exe().expect("test binary has an exe path");
        assert_eq!(exec_command(), exec_escape(&exe.to_string_lossy()));

        drop(sandbox);
        let _ = fs::remove_dir_all(&root);
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
        let root = sandbox_root("rhesis-autostart-enable-test");
        let sandbox = Sandbox::bare(&root);
        let file = sandbox_entry(&root);
        fs::write(&file, entry_contents()).expect("scratch entry writable");
        assert!(is_enabled());
        drop(sandbox);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn entries_from_another_flavor_count_as_off() {
        // Cross-flavor regression: a stale entry written by one flavor (e.g.
        // a native launcher path that the uninstall then removed) must not
        // read as ON in another flavor — systemd fails it at login with
        // 203/EXEC. With markers matching but Exec different, the toggle
        // reads off so enabling it rewrites the line for this flavor.
        let root = sandbox_root("rhesis-autostart-foreign-test");
        let sandbox = Sandbox::bare(&root);
        let file = sandbox_entry(&root);

        let stale_native = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\n\
             Exec=/gone/.local/bin/rhesis {HIDDEN_FLAG}\nIcon={APP_ID}\n"
        );
        fs::write(&file, &stale_native).expect("scratch entry writable");
        assert!(!is_enabled());

        fs::write(&file, "[Desktop Entry]\nName=Rhesis\nExec=rhesis\n")
            .expect("scratch entry writable");
        assert!(!is_enabled());

        drop(sandbox);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn heal_repoints_the_recorded_entry() {
        let root = sandbox_root("rhesis-autostart-heal-test");
        let sandbox = Sandbox::bare(&root);
        let file = sandbox_entry(&root);

        // What a previous launch of this install wrote before the payload
        // moved (the image path for an AppImage, the payload binary for a
        // tarball — the mechanism is record-based, not flavor-based).
        let stale = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\n\
             Exec=/old/Downloads/rhesis-0.4.0-x86_64.AppImage {HIDDEN_FLAG}\n\
             Icon={APP_ID}\n"
        );
        fs::write(&file, &stale).expect("scratch entry writable");

        let record = format!("/old/Downloads/rhesis-0.4.0-x86_64.AppImage {HIDDEN_FLAG}");
        assert!(
            autostart_self_heal(Some(&record)),
            "an entry matching our recorded line is a heal target"
        );

        // The heal rewrites the whole entry in the current form, so the
        // toggle reads it as ours again.
        let healed = fs::read_to_string(&file).expect("healed entry readable");
        assert_eq!(healed, entry_contents());
        assert!(is_enabled(), "healed entry must read as enabled");

        drop(sandbox);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn heal_only_touches_the_entry_the_record_describes() {
        let root = sandbox_root("rhesis-autostart-heal-negative-test");
        let sandbox = Sandbox::bare(&root);
        let file = sandbox_entry(&root);

        let record = "/still/there/rhesis-x86_64.AppImage --hidden";

        // The entry already matches the running flavor while the record
        // describes something older: no heal, nothing rewritten.
        let current = exec_escape(&std::env::current_exe().unwrap().to_string_lossy());
        let ours_shaped = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\nExec={current}\nIcon={APP_ID}\n"
        );
        fs::write(&file, &ours_shaped).expect("scratch entry writable");
        assert!(!autostart_self_heal(Some(record)));
        assert_eq!(
            fs::read_to_string(&file).expect("entry readable"),
            ours_shaped
        );

        // An entry left by another flavor or copy (Exec != record) must
        // survive: cross-flavor honesty belongs to the toggle, and a
        // foreign entry is never rewritten by the heal.
        let foreign = format!(
            "[Desktop Entry]\nType=Application\nName=Rhesis\n\
             Exec=/gone/.local/bin/rhesis {HIDDEN_FLAG}\n\
             Icon={APP_ID}\n"
        );
        fs::write(&file, &foreign).expect("scratch entry writable");
        assert!(!autostart_self_heal(Some(record)));
        assert_eq!(fs::read_to_string(&file).expect("entry readable"), foreign);

        // Missing file / missing Exec line are silent no-ops, not errors.
        let no_exec = "[Desktop Entry]\nType=Application\nName=Rhesis\n".to_string();
        fs::write(&file, &no_exec).expect("scratch entry writable");
        assert!(!autostart_self_heal(Some(record)));
        assert_eq!(fs::read_to_string(&file).expect("entry readable"), no_exec);
        fs::remove_file(&file).expect("scratch entry removable");
        assert!(!autostart_self_heal(Some(record)));

        drop(sandbox);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_heal_never_races_the_toggle() {
        // The heal and the toggle use the same Exec comparison: an entry
        // that reads as enabled is exactly the one whose Exec equals the
        // composed line, which autostart_self_heal answers no-op for.
        let root = sandbox_root("rhesis-autostart-heal-consistency-test");
        let sandbox = Sandbox::bare(&root);
        let file = sandbox_entry(&root);
        let ours = entry_contents();
        fs::write(&file, &ours).expect("scratch entry writable");
        assert!(is_enabled());
        let record = exec_line(&ours).expect("our entry has an Exec line");
        assert!(
            !autostart_self_heal(Some(record)),
            "an enabled entry must never be rewritten by the heal"
        );
        assert_eq!(fs::read_to_string(&file).expect("entry readable"), ours);

        drop(sandbox);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_heal_ignores_an_absent_record() {
        assert!(!super::autostart_self_heal(None));
    }

    #[test]
    fn the_uninstall_keeps_the_flatpak_entry_and_removes_ours() {
        let root = sandbox_root("rhesis-autostart-uninstall-test");
        let sandbox = Sandbox::bare(&root);
        let file = sandbox_entry(&root);

        // A flatpak entry survives a tarball/AppImage uninstall: the
        // flatpak install still exists and owns it.
        fs::write(
            &file,
            format!("[Desktop Entry]\nName=Rhesis\nExec=flatpak run {APP_ID}\n"),
        )
        .expect("scratch entry writable");
        assert!(!uninstall_entry());
        assert!(file.exists(), "the flatpak entry must survive");

        // The forms this module wrote (whatever flavor) die with the
        // uninstall — their login launch would be 203/EXEC anyway.
        fs::write(&file, entry_contents()).unwrap();
        assert!(uninstall_entry());
        assert!(!file.exists());

        // Legacy forms from earlier releases (launcher paths, bare names)
        // were written by this module too — removed as well.
        fs::write(
            &file,
            format!("[Desktop Entry]\nName=Rhesis\nExec=rhesis {HIDDEN_FLAG}\n"),
        )
        .unwrap();
        assert!(uninstall_entry());

        // Absent / Exec-less files are silent no-ops.
        assert!(!uninstall_entry());
        fs::write(&file, "[Desktop Entry]\nName=Rhesis\n").unwrap();
        assert!(!uninstall_entry());
        assert!(file.exists());

        drop(sandbox);
        let _ = fs::remove_dir_all(&root);
    }
}
