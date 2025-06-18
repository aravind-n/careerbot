use std::error::Error;

use async_trait::async_trait;
use shared::job::Job;
use tracing::info;

use crate::channel::NotificationChannel;

pub(crate) struct EmailChannel;

#[async_trait]
impl NotificationChannel for EmailChannel {
    async fn send(&self, job: &Job) -> Result<(), Box<dyn Error>> {
        // TODO Implement sending out the email
        info!(job = %job, "New job received");

        Ok(())
    }

    fn name(&self) -> String {
        "email".into()
    }
}
