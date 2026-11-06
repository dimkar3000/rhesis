//! Platform-surface constants: the app identity everything is derived from,
//! the single-instance wire words, and the OS-specific persistence names.

/// Flatpak application id (reverse-DNS form): the desktop entry name, the
/// single-instance socket name and the Windows named-object roots all
/// derive from it, which is what makes launched instances and autostart
/// entries idempotent across package flavors.
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

/// Descriptive error for the QML toggle; details are logged where they occur.
pub(crate) const SET_ERROR_MESSAGE: &str = "could not write the autostart entry";

/// Windows single-instance objects: a session-local mutex decides who is
/// primary and a named event carries the show request (see
/// `single_instance/windows.rs`). Spelled out rather than built from
/// [`APP_ID`]: `concat!` cannot expand consts.
#[cfg(windows)]
pub(crate) const MUTEX_NAME: &str = r"Local\io.github.dimkar3000.rhesis.instance";

#[cfg(windows)]
pub(crate) const EVENT_NAME: &str = r"Local\io.github.dimkar3000.rhesis.show";

/// Windows autostart persistence: a per-user `Run` key value named after
/// the product (see `autostart.rs`).
#[cfg(windows)]
pub(crate) const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

#[cfg(windows)]
pub(crate) const RUN_VALUE: &str = "Rhesis";
