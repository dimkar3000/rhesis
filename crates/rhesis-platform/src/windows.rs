//! Windows implementation of the platform surface. Consumers never name
//! this folder directly: `lib.rs` re-exports the public modules
//! generically; `job_object` stays crate-internal, reachable only through
//! the `kill_child_on_exit` behavior of `process`.

pub mod autostart;
pub mod constants;
pub mod install;
pub mod paths;
pub mod process;
pub mod resources;
pub mod uninstall;

mod job_object;
