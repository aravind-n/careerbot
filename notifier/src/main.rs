mod channel;
mod config;

use std::{error::Error, sync::Arc};

use futures::{FutureExt, StreamExt, stream::FuturesUnordered};
use shared::job::Job;
use tokio::time::{Duration, sleep};
use tracing::{debug, error, info};

use crate::config::NotifierConfig;

async fn run_notifier_loop(config: NotifierConfig) -> Result<(), Box<dyn Error>> {
    let consumer = config.message_consumer();
    let mut tasks = FuturesUnordered::new();

    loop {
        match consumer.next().await {
            Ok(Some(response)) => {
                // Deserialize job from response
                // Filter users who match job
                // Trigger notifs for users
                // TODO implement db search for users
                if let Ok(job) = serde_json::from_value::<Job>(response)
                    .inspect_err(|e| error!(error = %e, "Failed to deserialize message"))
                {
                    for channel in config.channels.iter() {
                        let job = job.clone();
                        let channel = Arc::clone(channel);

                        tasks.push(async move {
                            info!(job = %job, channel = %channel.name(), "Triggering notification channel");

                            match channel.send(&job).await {
                                Ok(_) => info!(job = %job, channel = %channel.name(), "Notification sent"),
                                Err(e) => error!(
                                    job = %job,
                                    channel = %channel.name(),
                                    error = %e,
                                    "Notification failed"
                                ),
                            }
                        });
                    }
                }
            }

            Ok(None) => {
                debug!("No message received");

                if tasks.is_empty() {
                    sleep(Duration::from_millis(100)).await;
                };
            }

            Err(e) => error!(error = %e, "Error occured while retrieving message"),
        }

        while let Some(Some(_)) = tasks.next().now_or_never() {}
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    dotenvy::dotenv().ok();
    shared::log::init_tracing();

    info!("Starting notifier");

    let config = NotifierConfig::load_configuration()
        .await
        .inspect_err(|e| error!(error = %e, "Unable to initialize config"))?;

    tokio::select! {
        _ = run_notifier_loop(config) => {
            info!("Notifier loop exited");
        }
        _ = shared::shutdown_signal() =>  {
            info!("Stopping notifier");
        }
    }

    Ok(())
}
