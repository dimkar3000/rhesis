//! Agnostic platform surface for Rhesis: everything the UI, the service and
//! the LanguageTool worker need from the host OS, without pulling any Qt or
//! HTTP/bookkeeping dependencies into the consumers.
//!
//! The code is organized per OS: `windows/` and `linux/` carry parallel
//! implementations behind the same module set (`autostart`, `constants`,
//! `paths`, `process`, `resources`, `single_instance`, plus the Windows-only
//! `job_object`). The `linux` folder is the `not(windows)` branch, so it
//! compiles for every non-Windows target.
//!
//! One generic re-export keeps the consumer paths OS-neutral:
//! `rhesis_platform::autostart::is_enabled()` resolves through whichever
//! folder the target compiles. `job_object` stays crate-internal,
//! reachable only through `process::kill_child_on_exit`.
//! `single_instance` is cross-platform and lives at the crate root.

#[cfg(not(windows))]
mod linux;
#[cfg(windows)]
mod windows;

pub mod single_instance;

#[cfg(not(windows))]
pub use linux::*;
#[cfg(windows)]
pub use windows::*;
