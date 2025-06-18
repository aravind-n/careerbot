use std::{env, error::Error, sync::Arc};

use shared::messaging::{MessageConsumer, MessagingConfig};
use tracing::error;

use crate::channel::{NotificationChannel, email::EmailChannel};

pub(crate) struct NotifierConfig {
    message_consumer: Arc<dyn MessageConsumer>,
    pub(crate) channels: Vec<Arc<dyn NotificationChannel>>,
}

impl NotifierConfig {
    fn get_enabled_channels() -> Result<Vec<Arc<dyn NotificationChannel>>, Box<dyn Error>> {
        // TODO implement dynamic loading of channels
        Ok(vec![Arc::new(EmailChannel)])
    }

    pub(crate) async fn load_configuration() -> Result<Self, Box<dyn Error>> {
        let mqueue_endpoint = env::var("MQUEUE_URL")
            .inspect_err(|e| error!(error = %e, "Missing MQUEUE_URL environment variable"))?;

        let stream_key = env::var("STREAM_KEY_JOBS")
            .inspect_err(|e| error!(error = %e, "Missing STREAM_KEY_JOBS environment variable"))?;

        let message_consumer: Arc<dyn MessageConsumer> =
            MessagingConfig::init_consumer(&mqueue_endpoint, &stream_key)?;

        let channels = Self::get_enabled_channels()?;

        Ok(Self {
            message_consumer,
            channels,
        })
    }

    pub fn message_consumer(&self) -> Arc<dyn MessageConsumer> {
        Arc::clone(&self.message_consumer)
    }
}
