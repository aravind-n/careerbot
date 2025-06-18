mod collector;
mod config;

use std::error::Error;
use std::sync::Arc;

use futures::{StreamExt, stream::FuturesUnordered};
use tokio::time::{Duration, sleep};
use tracing::{error, info};

use crate::config::IngestorConfig;

/// Runs the main collector loop
///
/// Each enabled collector is run concurrently and the loop pauses for
/// the delay duration
///
/// # Arguments
/// * `config` - A `Config` instance that stores configuration parameters
///
/// # Errors
/// * Any runtime errors encountered by spawned processes
async fn run_collector_loop(config: IngestorConfig) -> Result<(), Box<dyn Error>> {
    loop {
        let mut tasks = FuturesUnordered::new();

        for item in config.collectors.iter() {
            if let Some(factory) = config.factory_map.get(item) {
                let collector = factory();
                let store = Arc::clone(&config.store);
                let publisher = Arc::clone(&config.publisher);

                tasks.push(async move {
                    info!(collector = %collector.name(), "Starting collector");

                    match collector.collect(store, publisher).await {
                        Ok(_) => info!(collector = %collector.name(), "Collector finished"),
                        Err(e) => {
                            error!(collector = %collector.name(), error = %e, "Collector failed")
                        }
                    }
                });
            }
        }

        while tasks.next().await.is_some() {}

        sleep(Duration::from_secs(config.delay_duration)).await;
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    shared::log::init_tracing();

    info!("Starting ingestor");

    let config = IngestorConfig::load_configuration()
        .await
        .inspect_err(|e| error!(error = %e, "Unable to initialize config"))?;

    tokio::select! {
        _ = run_collector_loop(config) => {
            info!("Collector loop exited");
        }
        _ = shared::shutdown_signal() =>  {
            info!("Stopping ingestor");
        }
    }

    Ok(())
}
