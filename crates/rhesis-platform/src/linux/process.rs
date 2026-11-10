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
