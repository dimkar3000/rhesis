//! The Qt client: CXX-Qt bridge and QML UI, the connection loop against the
//! daemon, the boot sequence and the QML test machinery.
//!
//! Every Qt-side linkage lives in this crate's build script; the binary
//! entry points in the root package's `src/` only call into it.

pub mod connection;
pub mod constants;
pub mod fake_daemon;
pub mod interop;
pub mod ui_tests;

mod run;

pub use run::run;
