use std::{env, error::Error, sync::Arc};

use shared::{
    database::{Database, DatabaseFactory, postgres::PostgresDatabaseFactory},
    messaging::{MessageConsumer, MessageConsumerFactory, redis::RedisStreamConsumerFactory},
};
use tracing::error;

use crate::channel::{NotificationChannel, email::EmailChannel};

pub(crate) struct NotifierConfig {
    pub(crate) database: Arc<dyn Database>,
    pub(crate) message_consumer: Box<dyn MessageConsumer>,
    pub(crate) channels: Vec<Arc<dyn NotificationChannel>>,
}

impl NotifierConfig {
    fn get_enabled_channels() -> Result<Vec<Arc<dyn NotificationChannel>>, Box<dyn Error>> {
        // TODO implement dynamic loading of channels
        Ok(vec![Arc::new(EmailChannel)])
    }

    pub(crate) async fn load_configuration() -> Result<Self, Box<dyn Error>> {
        let db_endpoint = env::var("DATABASE_URL")
            .inspect_err(|e| error!(error = %e, "Missing DATABASE_URL environment variable"))?;

        let mqueue_endpoint = env::var("MQUEUE_URL")
            .inspect_err(|e| error!(error = %e, "Missing MQUEUE_URL environment variable"))?;

        let stream_key = env::var("STREAM_KEY_JOBS")
            .inspect_err(|e| error!(error = %e, "Missing STREAM_KEY_JOBS environment variable"))?;

        let database = PostgresDatabaseFactory::init(&db_endpoint).await?;

        let message_consumer: Box<dyn MessageConsumer> = RedisStreamConsumerFactory::init(
            &mqueue_endpoint,
            &stream_key,
            "notifier-group",
            "notifier-1",
        )
        .await?;

        let channels = Self::get_enabled_channels()?;

        Ok(Self {
            database,
            message_consumer,
            channels,
        })
    }
}
