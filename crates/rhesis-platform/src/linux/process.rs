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

/// No-op needed for windows only
pub fn suppress_console(_command: &mut std::process::Command) {}

/// Pre-spawn custody for children: on Linux the spawned process gets
/// `PR_SET_PDEATHSIG`, so the JVM dies whenever the daemon dies (crash or
/// hard kill) instead of squatting on the LanguageTool port until the user
/// kills it by hand. The signal fires on parent *thread* death; the worker
/// thread terminates its child explicitly anyway, so the daemon-death case
/// is the one this covers. Registration cannot fail — errors surface at
/// `spawn()`.
///
/// `mod linux` is already gated on `#[cfg(not(windows))]` in `lib.rs`, so
/// this compiles only on Linux — no inner `#[cfg]` is needed. The Windows
/// no-op lives in `windows/process.rs`.
pub fn kill_child_on_parent_exit(command: &mut std::process::Command) {
    use std::os::unix::process::CommandExt as _;

    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

/// No-op needed for windows only
pub fn kill_child_on_exit(_child: &std::process::Child) -> Result<(), String> {
    Ok(())
}

pub fn spawn_detached(program: std::path::PathBuf, args: &[String]) -> Result<(), String> {
    let mut command = std::process::Command::new(&program);
    command
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        command.process_group(0);
    }
    #[cfg(not(unix))]
    {
        panic!("somehow we got here on a none unix platform")
    }

    match command.spawn() {
        Ok(_child) => {
            log::info!("spawned detached: {}", program.display());
            Ok(())
        }
        Err(e) => Err(format!("spawning {}: {e}", program.display())),
    }
}
