//! Rhesis daemon entry point: binds the service socket, serves the UI
//! client, and owns the LanguageTool server for its whole lifetime.
//!
//! No arguments: everything is settings and environment (the UI decides
//! when to spawn and shutdown the daemon). Logging goes to stderr through
//! `RUST_LOG`, same as the UI process.

fn main() {
    env_logger::init();

    let options = rhesis_service::DaemonOptions::production();
    if let Err(e) = rhesis_service::run(&options) {
        log::error!("daemon failed: {e}");
        std::process::exit(1);
    }
    log::info!("daemon exited");
}
