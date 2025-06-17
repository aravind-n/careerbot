mod collector;
mod ingestor_config;

use std::env;
use std::error::Error;
use std::sync::Arc;

use futures::{StreamExt, stream::FuturesUnordered};
use shared::db::{JobStore, create_pool};
use shared::postgres::PostgresStore;
use tokio::signal::unix::{SignalKind, signal};
use tokio::time::{Duration, sleep};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::collector::Collector;
use crate::ingestor_config::IngestorConfig;

/// Initializes `tracing_subscriber` configuration
///
/// This allows the package to output logs using
/// the `tracing` crate
fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::from_default_env())
        .with_current_span(true)
        .with_span_list(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .init();
}

/// Initialized the database configuration
///
/// # Returns
/// An `Arc` `JobStore` value that handles db communications
///
/// # Errors
/// * Missing DATABASE_URL environment variable
/// * Any errors that occur when creating the db pool
async fn init_db() -> Result<Arc<dyn JobStore>, Box<dyn Error>> {
    let db_url = env::var("DATABASE_URL")
        .inspect_err(|e| error!(error = %e, "Missing DATABASE_URL environment variable"))?;

    let pool = create_pool(&db_url)
        .await
        .inspect_err(|e| error!(error = %e, "Failed to create database pool"))?;

    let store = Arc::new(PostgresStore::new(pool));

    Ok(store)
}

/// Runs the main collector loop
///
/// This function contains the main loop that encapsulates
/// service behavior  
/// Each enabled collector is run concurrently and the loop pauses for
/// the delay duration
async fn run_collector_loop() -> Result<(), Box<dyn Error>> {
    let config = IngestorConfig::default();
    let factory_map = Collector::build_factory_map();
    let store = init_db().await?;

    loop {
        let mut tasks = FuturesUnordered::new();
        let enabled_collectors = Collector::load_collector_config(&config.collectors);

        for item in enabled_collectors.iter() {
            if let Some(factory) = factory_map.get(item) {
                let collector = factory();
                let store_clone = Arc::clone(&store);

                tasks.push(async move {
                    info!(collector = %collector.name(), "Starting collector");

                    match collector.collect(store_clone).await {
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

/// Interrupt handler for shutdown events
///
/// Currently handles SIGTERM and SIGINT events
async fn shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).unwrap();

    tokio::select! {
        _ = sigterm.recv() => {
            info!("INFO: Received SIGTERM");
        }
        _ = tokio::signal::ctrl_c() => {
            info!("INFO: Received SIGINT");
        }
    }
}

#[tokio::main]
async fn main() {
    init_tracing();

    info!("job-ingestor started");

    tokio::select! {
        _ = run_collector_loop() => {
            info!("INFO: Collector loop exited");
        }
        _ = shutdown_signal() =>  {
            info!("INFO: Shutdown signal received");
        }
    }
}
