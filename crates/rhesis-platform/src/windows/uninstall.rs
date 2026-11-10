//! In-binary uninstall (invoked with `rhesis.exe --uninstall`) for the
//! Windows flavors: drop the autostart Run value, remove the configuration
//! folder (`install.conf` and the daemon's settings store), and schedule
//! the deletion of the payload folder for after this process exits — the
//! folder holds the running executable, which Windows does not allow to
//! delete from inside itself. The daemon the UI spawned dies with the UI
//! process anyway (job object), so the scheduled command fires ~2s later
//! into a quiescent folder.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use super::{autostart, constants::APP_ID, install, paths, process};

/// Remove this copy (or fail with a user-facing message; development
/// builds have nothing installed).
pub fn uninstall() -> Result<String, String> {
    let Some(payload) = install::payload_root() else {
        return Err("development build; nothing is installed to remove".to_string());
    };

    // The Run value disappears for good; the install.conf record goes with
    // the configuration folder below.
    if let Err(e) = autostart::set_enabled(false) {
        log::warn!("could not clear the autostart Run value: {e:?}");
    }

    let mut removed: Vec<String> = Vec::new();
    if let Some(config) = paths::config_dir() {
        match fs::remove_dir_all(&config) {
            Ok(()) => removed.push("the configuration".to_string()),
            Err(e) => log::warn!("could not remove the configuration folder {config:?}: {e:?}"),
        }
    }

    // Remove Start Menu shortcuts the installer may have written.
    let mut desktop_removed = false;
    for path in apps_desktop_files(&install::Flavor::Windows) {
        match fs::remove_file(&path) {
            Ok(()) => desktop_removed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("could not remove the application-menu entry {path:?}: {e:?}"),
        }
    }
    if desktop_removed {
        removed.push("the application-menu entry".to_string());
    }

    // Best effort immediate deletion of what is not locked; the scheduled
    // command removes the rest — including this executable — for sure.
    let _ = fs::remove_dir_all(&payload);
    schedule_payload_removal(&payload)?;

    let summary = if removed.is_empty() {
        String::from("nothing else was removable")
    } else {
        format!("removed {}", removed.join(", "))
    };
    Ok(format!(
        "{summary}; the program folder {} is scheduled for deletion",
        payload.display()
    ))
}

/// Schedule the payload's deletion via a detached `cmd`: it pings ~2s
/// (this process exits) and then `rd /s /q` removes the folder. A plain
/// child, deliberately NOT tied to the kill-on-close job object — it must
/// outlive the uninstalling process.
fn schedule_payload_removal(root: &Path) -> Result<(), String> {
    let mut command = Command::new("cmd");
    command
        .arg("/c")
        .arg("ping")
        .arg("-n")
        .arg("3")
        .arg("127.0.0.1")
        .arg(">")
        .arg("nul")
        .arg("&")
        .arg("rd")
        .arg("/s")
        .arg("/q")
        .arg(format!("\"{}\"", root.display()));
    process::suppress_console(&mut command);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match command.spawn() {
        Ok(_child) => Ok(()),
        Err(e) => Err(format!(
            "could not schedule the removal of {}: {e}; delete the folder manually",
            root.display()
        )),
    }
}

/// The Start Menu shortcut locations the installer can have written,
/// based on the install flavor.
fn apps_desktop_files(flavor: &install::Flavor) -> Vec<PathBuf> {
    let entry = format!("{APP_ID}.desktop");
    let mut paths = Vec::new();
    match flavor {
        install::Flavor::Windows => {
            if let Some(appdata) = env::var_os("APPDATA") {
                paths.push(
                    PathBuf::from(appdata)
                        .join("Microsoft")
                        .join("Windows")
                        .join("Start Menu")
                        .join("Programs")
                        .join(&entry),
                );
            }
            if let Some(all_users) = env::var_os("ALLUSERSPROFILE") {
                paths.push(
                    PathBuf::from(all_users)
                        .join("Microsoft")
                        .join("Windows")
                        .join("Start Menu")
                        .join("Programs")
                        .join(&entry),
                );
            }
        }
    }
    paths
}
