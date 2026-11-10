//! Linux constants: the app identity the desktop entry and the
//! single-instance socket name derive from, and the guard's wire payload.

/// Flatpak application id (reverse-DNS form): the desktop entry name and
/// the single-instance socket name derive from it, which is what makes
/// launched instances and autostart entries idempotent across package
/// flavors.
pub const APP_ID: &str = "io.github.dimkar3000.rhesis";

/// Reserved-only app id: doubles as the abstract socket name, which stays
/// well below the 107-byte socket address limit.
pub const SOCKET_NAME: &[u8] = APP_ID.as_bytes();

/// Payload a secondary instance sends to the primary one.
pub const SHOW_REQUEST: &[u8] = b"show\n";

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
