use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "rhesis-service",
    version,
    about = "Rhesis daemon: responsible to setup and manage the LanguageTool server."
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
    let mut options = rhesis_service::daemon::DaemonOptions::production();
    options.fake_fixture = cli.fake_lt;

    if let Err(e) = rhesis_service::daemon::run(&options) {
        log::error!("daemon failed: {e}");
        std::process::exit(1);
    }
    log::info!("daemon exited");
}
