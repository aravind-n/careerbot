pub mod redis;

use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait MessagePublisher: Send + Sync {
    async fn publish(&self, message: Value) -> Result<(), Box<dyn Error>>;
}

#[async_trait]
pub trait MessageConsumer: Send + Sync {
    async fn next(&self) -> Result<Option<Value>, Box<dyn Error>>;
}

pub struct MessagingConfig;

impl MessagingConfig {
    /// Initialize the message queue publisher configuration
    ///
    /// # Returns
    /// An `Arc` `MessagePublisher` value that handles data stream publishing
    ///
    /// # Errors
    /// * Missing MQUEUE_URL environment variable
    /// * Missing JOB_STREAM_KEY environment variable
    /// * Any errors that occur while opening a connection to the stream
    pub fn init_publisher(
        endpoint: &str,
        stream_key: &str,
    ) -> Result<Arc<dyn MessagePublisher>, Box<dyn Error>> {
        Ok(Arc::new(redis::RedisStreamPublisher::new(
            endpoint, stream_key,
        )?))
    }

    pub fn init_consumer(
        endpoint: &str,
        stream_key: &str,
    ) -> Result<Arc<dyn MessageConsumer>, Box<dyn Error>> {
        let stream_consumer =
            redis::RedisStreamConsumer::new(endpoint, stream_key, "mygroup", "notifier_worker")?;

        Ok(Arc::new(stream_consumer))
    }
}
