//! Agnostic platform surface for Rhesis: everything the UI, the service and
//! the LanguageTool worker need from the host OS, without pulling any Qt or
//! HTTP/bookskeeping dependencies into the consumers.
//!
//! Per-OS internals stay behind the modules: the Linux single-instance guard
//! (abstract socket) sits next to the Windows one (mutex + event), and the
//! Windows job-object custody for child processes is a no-op everywhere else.

pub mod autostart;
pub mod constants;
pub mod paths;
pub mod process;
pub mod resources;
pub mod single_instance;

#[cfg(windows)]
mod job_object;
