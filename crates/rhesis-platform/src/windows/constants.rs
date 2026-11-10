//! Windows constants: the OS-specific persistence names plus the shared
//! app surface (`APP_ID`, `HIDDEN_FLAG`) that platform-neutral consumers
//! read through `lib.rs`'s generic re-export.

/// App id (reverse-DNS form).
pub const APP_ID: &str = "io.github.dimkar3000.rhesis";

/// CLI flag launching into the tray without flashing the window; ignored
/// when background hosting is disabled (helper.h keeps the window visible
/// so the app can never strand itself without a tray icon).
pub const HIDDEN_FLAG: &str = "--hidden";

/// CLI flag removing this install instead of starting the UI: handled by
/// the client before any UI comes up, flavor-guarded (see
/// `rhesis_platform::uninstall`).
pub const UNINSTALL_FLAG: &str = "--uninstall";

/// Descriptive error for the QML toggle; details are logged where they occur.
pub(crate) const SET_ERROR_MESSAGE: &str = "could not write the autostart entry";

/// Autostart persistence: a per-user `Run` key value named after the
/// product (see `autostart.rs`).
pub(crate) const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

pub(crate) const RUN_VALUE: &str = "Rhesis";
