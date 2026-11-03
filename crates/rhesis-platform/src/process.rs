//! Process-level helpers shared by every Rhesis binary.

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
#[cfg(windows)]
pub fn suppress_console(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

/// Console suppression is a Windows-only concern; other platforms spawn
/// children as-is.
#[cfg(not(windows))]
pub fn suppress_console(_command: &mut std::process::Command) {}

/// Prevent spawned children from outliving Rhesis (a crash would otherwise
/// keep the LanguageTool port taken until the user kills it by hand).
/// Windows enforces this with the kill-on-close job object in `job_object.rs`;
/// every other platform gets a no-op.
#[cfg(windows)]
pub fn kill_child_on_exit(child: &std::process::Child) -> Result<(), String> {
    crate::job_object::kill_child_on_exit(child)
}

#[cfg(not(windows))]
pub fn kill_child_on_exit(_child: &std::process::Child) -> Result<(), String> {
    Ok(())
}

/// Spawn the given program detached from this process's fate: it survives
/// the UI exiting (the daemon must outlive its client) and a closing
/// controlling terminal (`process_group(0)` removes the child from the
/// terminal's process group, so no SIGHUP arrives when the session ends).
/// Console suppression applies on Windows; stdio is discarded either way —
/// the daemon logs are not the UI's.
///
/// The caller does not track the returned child: dropping the handle (= the
/// return value here) neither kills nor waits on it.
pub fn spawn_detached(program: std::path::PathBuf) -> Result<(), String> {
    let mut command = std::process::Command::new(&program);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    suppress_console(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    match command.spawn() {
        Ok(_child) => {
            log::info!("spawned detached: {}", program.display());
            Ok(())
        }
        Err(e) => Err(format!("spawning {}: {e}", program.display())),
    }
}
