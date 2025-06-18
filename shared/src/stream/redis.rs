use std::error::Error;

use async_trait::async_trait;
use redis::{AsyncCommands, Client};
use serde_json::Value;
use tracing::error;

use crate::stream::StreamPublisher;

pub struct RedisStreamPublisher {
    client: Client,
    stream: String,
}

impl RedisStreamPublisher {
    pub fn new(client: Client, stream: &str) -> Self {
        Self {
            client,
            stream: stream.into(),
        }
    }

    pub fn get_client(url: &str) -> Result<Client, Box<dyn Error>> {
        Client::open(url).map_err(|e| {
            error!(error = %e, "Error creating redis client");
            e.into()
        })
    }
}

#[async_trait]
impl StreamPublisher for RedisStreamPublisher {
    async fn publish(&self, message: Value) -> Result<(), Box<dyn Error>> {
        let json = serde_json::to_string(&message)?;
        let mut conn = self.client.get_multiplexed_async_connection().await?;

        let _: String = conn.xadd(&self.stream, "*", &[("data", json)]).await?;

        Ok(())
    }
}
