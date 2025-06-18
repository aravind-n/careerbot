mod channel;
mod config;

use std::error::Error;

use futures::{StreamExt, stream::FuturesUnordered};
use shared::job::Job;
use tracing::{debug, error, info};

use crate::config::NotifierConfig;

async fn run_notifier_loop(config: NotifierConfig) -> Result<(), Box<dyn Error>> {
    let mut tasks = FuturesUnordered::new();

    loop {
        let consumer = config.message_consumer();

        match consumer.next().await {
            Ok(Some(value)) => {
                // Deserialize job from response
                // Filter users who match job
                // Trigger notifs for users
                // TODO implement db search for users
                let job: Job = serde_json::from_value(value).unwrap();

                for channel in config.channels.iter() {
                    let job = job.clone();

                    tasks.push(async move {
                        info!(job = %job, channel = %channel.name(), "Triggering notification channel");

                        match channel.send(&job).await {
                            Ok(_) => info!(job = %job, channel = %channel.name(), "Notification sent"),
                            Err(e) => error!(channel = %channel.name(), error = %e, "Notification failed"),
                        }
                    });
                }

                while tasks.next().await.is_some() {}
            }

            Ok(None) => debug!("No message received"),
            Err(e) => {
                error!(error = %e, "Error occured while retrieving message")
            }
        }
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
