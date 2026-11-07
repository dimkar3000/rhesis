//! Rhesis daemon entry point: binds the service socket, serves the UI
//! client, and owns the LanguageTool server for its whole lifetime.
//!
//! The command line stays minimal (the UI decides when to spawn and
//! shutdown the daemon; logging goes to stderr through `RUST_LOG`, same
//! as the UI process) — the one argument is the recorded-fixture seam
//! that lets the test/dev harness run without a real LanguageTool server.

use clap::Parser;

/// Long-term this stays tiny: production launches the daemon through
/// `spawn_detached`, which argues nothing (`--fake-lt` arrives only when a
/// harness launched the client with the flag and the client forwarded it).
/// Everything downstream of [`DaemonOptions`] sees only the resolved
/// decision.
#[derive(Parser, Debug)]
#[command(
    name = "rhesis-service",
    version,
    about = "Rhesis daemon: owns the LanguageTool server, serves one UI client"
)]
struct Cli {
    /// Serve check requests from the recorded fixture
    /// `rhesis-languagetool/tests/fixtures/lt/<FIXTURE>.json` instead of
    /// a real LanguageTool server (a fixture that cannot load falls back
    /// to the live client). Test/dev harness seam: the client re-emits the
    /// flag on the daemon's command line when IT was launched with it.
    #[arg(long = "fake-lt", value_name = "FIXTURE")]
    fake_lt: Option<String>,
}

fn main() {
    env_logger::init();

    let cli = Cli::parse();
    let mut options = rhesis_service::DaemonOptions::production();
    options.fake_fixture = cli.fake_lt;

    if let Err(e) = rhesis_service::run(&options) {
        log::error!("daemon failed: {e}");
        std::process::exit(1);
    }
    log::info!("daemon exited");
}
