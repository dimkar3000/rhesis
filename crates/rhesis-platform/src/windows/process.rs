//! Windows branch of the process helpers: console suppression for spawned
//! children, and their lifetime tied to Rhesis through the job object.

use std::path::PathBuf;

/// Directory containing the running executable (the install layout root for
/// the Windows flavors, which keep everything next to the executable).
pub fn exe_dir() -> Option<PathBuf> {
    match std::env::current_exe() {
        Ok(exe) => exe.parent().map(std::path::Path::to_path_buf),
        Err(e) => {
            log::warn!("cannot determine the executable location: {e:?}");
            None
        }
    }
}

/// A GUI app has no console: without CREATE_NO_WINDOW Windows opens a
/// console window for every child we spawn (most visibly the JVM).
pub fn suppress_console(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Prevent spawned children from outliving Rhesis (a crash would otherwise
/// keep the LanguageTool port taken until the user kills it by hand): the
/// kill-on-close job object in `job_object.rs`.
pub fn kill_child_on_exit(child: &std::process::Child) -> Result<(), String> {
    super::job_object::kill_child_on_exit(child)
}

/// Pre-spawn custody: no-op here — the job object needs the spawned handle,
/// so Windows ties the child in post-spawn via [`kill_child_on_exit`].
pub fn kill_child_on_parent_exit(_command: &mut std::process::Command) {}

/// Spawn the given program (with `args`) detached from this process's
/// fate: it survives the UI exiting (the daemon must outlive its client);
/// console suppression applies and stdio is discarded — the daemon logs
/// are not the UI's.
///
/// The caller does not track the returned child: dropping the handle (= the
/// return value here) neither kills nor waits on it.
pub fn spawn_detached(program: std::path::PathBuf, args: &[String]) -> Result<(), String> {
    let mut command = std::process::Command::new(&program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    suppress_console(&mut command);
    match command.spawn() {
        Ok(_child) => {
            log::info!("spawned detached: {}", program.display());
            Ok(())
        }
        Err(e) => Err(format!("spawning {}: {e}", program.display())),
    }
}
