use std::error::Error;

use async_trait::async_trait;
use redis::{AsyncCommands, Client};
use serde_json::Value;
use tracing::error;

use crate::messaging::{MessageConsumer, MessagePublisher};

/// TODO add doc comments
pub struct RedisStreamPublisher {
    client: Client,
    stream_key: String,
}

/// TODO add doc comments
impl RedisStreamPublisher {
    pub fn new(endpoint: &str, stream_key: &str) -> Result<Self, Box<dyn Error>> {
        let client = Client::open(endpoint).map_err(|e| {
            error!(error = %e, "Error creating redis client");
            e
        })?;

        Ok(Self {
            client,
            stream_key: stream_key.into(),
        })
    }
}

/// TODO add doc comments
#[async_trait]
impl MessagePublisher for RedisStreamPublisher {
    async fn publish(&self, message: Value) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_string(&message)?;
        let mut conn = self.client.get_multiplexed_async_connection().await?;

        let _: String = conn.xadd(&self.stream_key, "*", &[("data", json)]).await?;

        Ok(())
    }
}

/// TODO add doc comments
pub struct RedisStreamConsumer {
    _client: Client,
    _stream: String,
    _group: String,
    _consumer_name: String,
}

impl RedisStreamConsumer {
    pub fn new(
        endpoint: &str,
        stream: &str,
        group: &str,
        consumer_name: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let client = Client::open(endpoint).map_err(|e| {
            error!(error = %e, "Error creating redis client");
            e
        })?;

        Ok(Self {
            _client: client,
            _stream: stream.to_string(),
            _group: group.to_string(),
            _consumer_name: consumer_name.to_string(),
        })
    }
}

#[async_trait]
impl MessageConsumer for RedisStreamConsumer {
    async fn next(&self) -> Result<Option<Value>, Box<dyn Error>> {
        Ok(None)
    }
}
