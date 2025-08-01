use std::{error::Error, sync::Arc};

use futures::{StreamExt, stream::FuturesUnordered};
use shared::job::Job;
use tokio::{
    task::JoinHandle,
    time::{Duration, sleep},
};
use tracing::{debug, error, info};

use crate::config::NotifierConfig;

pub(crate) async fn run_notifier_loop(config: NotifierConfig) -> Result<(), Box<dyn Error>> {
    let consumer = &config.message_consumer;
    let mut tasks = FuturesUnordered::new();

    loop {
        tokio::select! {
            msg = consumer.next() => {
                match msg {
                    Ok(Some(response)) => {
                        if let Some(job) = parse_json(response) {
                            spawn_notification_tasks(&config, &job, &mut tasks).await;
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
            }

            Some(task_result) = tasks.next() => {
                if let Err(e) = task_result {
                    error!(error = %e, "Notification tasks panicked");
                }
            }
        }
    }
}

fn parse_json(response: serde_json::Value) -> Option<Job> {
    serde_json::from_value::<Job>(response)
        .inspect_err(|e| error!(error = %e, "Failed to deserialize message"))
        .ok()
}

async fn spawn_notification_tasks(
    config: &NotifierConfig,
    job: &Job,
    tasks: &mut FuturesUnordered<JoinHandle<()>>,
) {
    match config.database.get_interested_users_for_job(job).await {
        Ok(users) => {
            for user in &users {
                for channel in config.channels.iter() {
                    let user = user.clone();
                    let job = job.clone();
                    let channel = Arc::clone(channel);

                    let task = tokio::spawn(async move {
                        info!(user = %user, job = %job, channel = %channel.name(), "Triggering notification channel");

                        if let Err(e) = channel.send(&user, &job).await {
                            error!(job = %job, channel = %channel.name(), error = %e, "Notification failed")
                        }
                    });

                    tasks.push(task);
                }
            }
        }
        Err(e) => error!(error = %e, job = %job, "Failed to retrieve matching users"),
    }
}
