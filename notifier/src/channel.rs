pub(crate) mod email;

use std::error::Error;

use async_trait::async_trait;
use shared::job::Job;

#[async_trait]
pub trait NotificationChannel: Send + Sync {
    async fn send(&self, job: &Job) -> Result<(), Box<dyn Error>>;
    fn name(&self) -> String;
}
