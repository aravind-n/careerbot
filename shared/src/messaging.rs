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

#[async_trait]
pub trait MessagePublisherFactory: Send + Sync {
    async fn init(endpoint: &str, key: &str) -> Result<Arc<dyn MessagePublisher>, Box<dyn Error>>;
}

#[async_trait]
pub trait MessageConsumerFactory: Send + Sync {
    async fn init(
        endpoint: &str,
        key: &str,
        group: &str,
        consumer_name: &str,
    ) -> Result<Box<dyn MessageConsumer>, Box<dyn Error>>;
}
