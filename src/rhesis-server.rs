use rhesis_constants::DEFAULT_PORT;
use rhesis_languagetool::client::LanguageToolClient;
use rhesis_service::daemon::DaemonOptions;

fn main() {
    env_logger::init();

    // The live client's construction address is a placeholder: the first
    // settings-derived event re-points it at the worker's active address
    // before any request runs.
    let provider = LanguageToolClient::new(format!("http://localhost:{DEFAULT_PORT}"));

    if let Err(e) = rhesis_service::daemon::deamon_run(&DaemonOptions::production(), provider) {
        log::error!("daemon failed: {e}");
        std::process::exit(1);
    }
    log::info!("daemon exited");
}
