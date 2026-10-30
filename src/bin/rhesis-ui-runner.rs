//! Qt Quick Test runner for the Rhesis QML suites (`tests/ui/*.qml`).
//!
//! A thin shim: the actual entry point lives in the library
//! (`rhesis::ui_tests::run_suites`). The C++ probes (QuickTest linkage,
//! harness.cpp) are compiled unconditionally by build.rs so that test
//! targets can link them without a feature gate.
//!
//! Run:
//! ```sh
//! cargo run --bin rhesis-ui-runner
//! # or, once built:
//! QT_QPA_PLATFORM=offscreen build/target/debug/rhesis-ui-runner
//! ```

fn main() {
    std::process::exit(rhesis::ui_tests::run_suites());
}
