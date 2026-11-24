//! Paperloom Cloud. Settings come from the environment; see
//! docs/sync-protocol.md.

use std::process::ExitCode;

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let config = match paperloom_server::config::Config::from_env() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("paperloom-server: {e}");
            return ExitCode::FAILURE;
        }
    };
    match paperloom_server::run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}
