//! The Qt-tier client's constants, grouped by their owning module.
//!
//! Connection timing lives here once so the reconnect loop, the boot probe
//! and the tier docs all name the same budget; the boot resources are
//! consumed by the binary (`main.rs`), the fixture-daemon stub and the
//! Quick Test runner ids by their modules.

use std::time::Duration;

// ---- connection.rs: reconnect backoff, spawn pacing, boot-probe timing ---

/// First reconnect wait; doubles on every failed attempt.
pub(crate) const RETRY_CHUNK: Duration = Duration::from_millis(50);

/// Upper bound of the reconnect backoff: while the daemon is absent, the
/// handshake retries at this cadence at most (§5: "handshake retry
/// 50 ms → 2 s cap").
pub(crate) const RETRY_MAX: Duration = Duration::from_millis(2000);

/// Minimum interval between two detached spawn attempts. A broken daemon
/// that dies at bind would otherwise turn the reconnect loop into an exec
/// loop; this keeps the worst case to one spawn per interval.
pub(crate) const SPAWN_INTERVAL: Duration = Duration::from_millis(2000);

/// Total boot-probe budget: connect-or-spawn must finish well before the
/// user notices anything; first-run daemon bind + a settings store load
/// take milliseconds, so `None` past the budget means real trouble.
pub(crate) const BOOT_BUDGET: Duration = Duration::from_secs(2);

/// Pace of both the boot-probe retries (while waiting for a spawned
/// daemon to bind) and the disconnect-wait alarm tick (see [`RETRY_CHUNK`]).
pub(crate) const PROBE_SLICE: Duration = RETRY_CHUNK;

/// The daemon argument this client re-emits from its own command line
/// (`--fake-lt <fixture>`, see `connection.rs :: forwarded_daemon_args`):
/// the e2e tiers launch the client with the flag, the spawned daemon
/// resolves the recorded fixture provider through its own clap parse —
/// the decision travels only through arguments.
pub(crate) const FAKE_FIXTURE_FLAG: &str = "--fake-lt";

/// How long a boot replay read may block before the probe bails. The
/// replay is deterministic (Settings + Status right after the greeting),
/// so hitting this means a wedged peer — boot on defaults instead.
pub(crate) const BOOT_REPLAY_CAP: Duration = Duration::from_secs(1);

// ---- interop: client-local status codes ----------------------------------

/// Status code the client reports locally when no daemon session exists
/// (QML's `server_status`). Codes 0-5 arrive from the daemon on the wire
/// (`DaemonMsg::Status`); 6 never does — it exists only between the
/// connection loop and the UI.
pub(crate) const CLIENT_STATUS_DISCONNECTED: i32 = 6;

// ---- main.rs: boot resources (used by the binary target, hence `pub`) ----

/// QML root loaded at boot: Windows ships its own Kirigami-free QML
/// (see build.rs).
#[cfg(windows)]
pub const ROOT_QML_SOURCE: &str = "src/interop/qml/windows/Root.qml";

#[cfg(not(windows))]
pub const ROOT_QML_SOURCE: &str = "src/interop/qml/Root.qml";

/// Quick Controls style when `QT_QUICK_CONTROLS_STYLE` leaves the choice
/// unset: the native look per platform.
#[cfg(windows)]
pub const DEFAULT_QUICK_STYLE: &str = "FluentWinUI3";

#[cfg(not(windows))]
pub const DEFAULT_QUICK_STYLE: &str = "org.kde.desktop";

// ---- fake_daemon.rs: the in-process fixture daemon --------------------

/// Poll slice of the fake daemon's accept loop: same shape as the real
/// daemon's `ACCEPT_SLICE` (`crates/rhesis-service`) — a connect is
/// answered within one slice.
pub(crate) const FAKE_ACCEPT_SLICE: Duration = Duration::from_millis(50);

/// The checker fixture: the same three recommendations recorded in
/// `tests/fixtures/lt/checker.json` and asserted by the daemon-session
/// tier (HE_LIKE suggestion pair at [4, 8), MORFOLOGIK at [38, 44)).
pub(crate) const FIXTURE_TEXT: &str = "She like football and play the guitar quicly.";

/// `HE_LIKE`'s two recorded replacement forms, in fixture order.
pub(crate) const FIXTURE_LIKE_FIRST: &str = "is like";
pub(crate) const FIXTURE_LIKE_SECOND: &str = "likes";

/// The MORFOLOGIK spelling fix recorded for `FIXTURE_TEXT`.
pub(crate) const FIXTURE_QUICKLY: &str = "quickly";

/// The recorded grammar tooltip of the `HE_LIKE` pair.
pub(crate) const FIXTURE_GRAMMAR_TOOLTIP: &str =
    "The pronoun 'She' requires a third-person verb or a past tense. Or did you forget to add a verb?";

/// The recorded spelling tooltip of the MORFOLOGIK match.
pub(crate) const FIXTURE_SPELL_TOOLTIP: &str = "Possible spelling mistake found.";

// ---- ui_tests.rs: the Quick Test runner --------------------------------

/// Runner process name and Quick Test suite id, reported in the output
/// banner (`********* Start testing of rhesis-ui-tests *********`).
pub(crate) const TEST_ID: &str = "rhesis-ui-tests";
