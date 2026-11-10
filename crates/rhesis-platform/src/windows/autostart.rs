//! Autostart at login: a value in
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` that launches the
//! executable with `--hidden`. Per-user, so no admin rights are needed;
//! Windows lets the user switch such entries off in Task Manager's Startup
//! tab, and that state is not read back here.
//!
//! Same interface as the XDG desktop entry (`is_enabled`/`set_enabled`/
//! `autostart_self_heal`), so the toggle and the heal are platform-neutral. The
//! "what did we write" memory lives in `install.conf` (`autostart-exec`);
//! the heal compares the registry value against that record. The value
//! launches into the tray like the Linux desktop entry does (see Root.qml's
//! hidden-launch gate).

use super::constants::{HIDDEN_FLAG, RUN_KEY, RUN_VALUE, SET_ERROR_MESSAGE};

pub fn is_enabled() -> bool {
    read_run_value().is_some()
}

pub fn set_enabled(enable: bool) -> Result<(), &'static str> {
    if enable {
        let command = run_command().ok_or("could not determine the executable path")?;
        write_run_value(&command)?;
        // The record is what the next launch's autostart_self_heal matches against.
        super::install::record_autostart(&command);
        Ok(())
    } else {
        delete_run_value()
    }
}

/// Re-point a Run value the record says we wrote at the running copy — its
/// fix for the one autostart target that can go stale: a portable zip
/// re-extracted elsewhere leaves the value pointing at a location that no
/// longer launches at login. Returns whether the value changed.
pub fn autostart_self_heal(record: Option<&str>) -> bool {
    let Some(record) = record else {
        return false;
    };
    let Some(current) = run_command() else {
        return false;
    };
    if current == record {
        return false; // the record still describes the running copy
    }
    let Some(value) = read_run_value() else {
        return false;
    };
    if value != record {
        return false; // not the value our record describes
    }
    match write_run_value(&current) {
        Ok(()) => {
            super::install::record_autostart(&current);
            log::info!("repointed the autostart Run value at the running copy");
            true
        }
        Err(e) => {
            log::error!("failed to repoint the stale autostart Run value: {e:?}");
            false
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// What the Run value should contain for the running copy.
fn run_command() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    Some(format!("\"{}\" {HIDDEN_FLAG}", exe.display()))
}

fn read_value(key_path: &str, name: &str) -> Option<String> {
    use std::ptr;
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ},
    };

    let key = wide(key_path);
    let name = wide(name);
    let mut bytes: u32 = 0;
    // SAFETY: the key and value names are NUL-terminated UTF-16; with a null
    // data pointer the call only reports the required size in `bytes`.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut bytes,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }

    let mut buffer = vec![0u16; (bytes as usize).div_ceil(2) + 1];
    let mut capacity = (buffer.len() * 2) as u32;
    // SAFETY: `buffer` is writable for `capacity` bytes.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut capacity,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

fn write_value(key_path: &str, name: &str, command: &str) -> Result<(), &'static str> {
    use windows_sys::Win32::{
        Foundation::ERROR_SUCCESS,
        System::Registry::{RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ},
    };

    let key = wide(key_path);
    let name = wide(name);
    let data = wide(command);
    // SAFETY: all strings are NUL-terminated UTF-16 and `data` is readable
    // for the byte length passed (terminator included, as REG_SZ requires).
    let status = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            REG_SZ,
            data.as_ptr().cast(),
            (data.len() * 2) as u32,
        )
    };
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        log::error!("RegSetKeyValueW failed with status {status}");
        Err(SET_ERROR_MESSAGE)
    }
}

fn delete_value(key_path: &str, name: &str) -> Result<(), &'static str> {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{RegDeleteKeyValueW, HKEY_CURRENT_USER},
    };

    let key = wide(key_path);
    let name = wide(name);
    // SAFETY: both names are NUL-terminated UTF-16.
    let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
    // Already gone is the state the caller asked for.
    if status == ERROR_SUCCESS || status == ERROR_FILE_NOT_FOUND {
        Ok(())
    } else {
        log::error!("RegDeleteKeyValueW failed with status {status}");
        Err(SET_ERROR_MESSAGE)
    }
}

fn read_run_value() -> Option<String> {
    read_value(RUN_KEY, RUN_VALUE)
}

fn write_run_value(command: &str) -> Result<(), &'static str> {
    write_value(RUN_KEY, RUN_VALUE, command)
}

fn delete_run_value() -> Result<(), &'static str> {
    delete_value(RUN_KEY, RUN_VALUE)
}

#[cfg(test)]
mod tests {
    use super::{delete_value, read_value, wide, write_value};

    #[test]
    fn registry_value_round_trips_on_a_scratch_key() {
        use windows_sys::Win32::System::Registry::{RegDeleteKeyW, HKEY_CURRENT_USER};

        // Never the real Run key: a throwaway key per test process, removed
        // again at the end.
        let parent = r"Software\rhesis-tests";
        let key = format!(r"{parent}\autostart-{}", std::process::id());
        let name = "Rhesis";

        assert_eq!(read_value(&key, name), None, "a fresh key has no value");

        let command = r#""C:\Program Files\Rhesis\rhesis.exe" --hidden"#;
        write_value(&key, name, command).expect("write creates the key and the value");
        assert_eq!(read_value(&key, name).as_deref(), Some(command));

        write_value(&key, name, "other").expect("write replaces the value");
        assert_eq!(read_value(&key, name).as_deref(), Some("other"));

        delete_value(&key, name).expect("delete removes the value");
        assert_eq!(read_value(&key, name), None);
        delete_value(&key, name).expect("deleting a missing value is not an error");

        // SAFETY: NUL-terminated key paths; the leaf must go before its parent.
        unsafe {
            RegDeleteKeyW(HKEY_CURRENT_USER, wide(&key).as_ptr());
            RegDeleteKeyW(HKEY_CURRENT_USER, wide(parent).as_ptr());
        }
    }
}
