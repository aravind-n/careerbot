mod channel;
mod config;
mod orchestrator;

use std::error::Error;

use tracing::{error, info};

use crate::config::NotifierConfig;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    shared::log::init_tracing();

    info!("Starting notifier");

    let config = NotifierConfig::load_configuration()
        .await
        .inspect_err(|e| error!(error = %e, "Unable to initialize config"))?;

    tokio::select! {
        _ = orchestrator::run_notifier_loop(config) => {
            info!("Notifier loop exited");
        }
        _ = shared::shutdown_signal() =>  {
            info!("Stopping notifier");
        }
    }

    Ok(())
}
