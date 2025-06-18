pub mod redis;

use std::error::Error;

use async_trait::async_trait;
use serde_json::Value;

#[async_trait]
pub trait StreamPublisher: Send + Sync {
    async fn publish(&self, message: Value) -> Result<(), Box<dyn Error>>;
}
