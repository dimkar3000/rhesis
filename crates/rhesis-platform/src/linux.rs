//! Unix implementation of the platform surface (`not(windows)`, so it
//! compiles for any non-Windows target, not just the shipped Linux
//! flavors). Consumers never name this folder directly: `lib.rs`
//! re-exports the public modules generically.

pub mod autostart;
pub mod constants;
pub mod install;
pub mod paths;
pub mod process;
pub mod resources;
pub mod uninstall;
