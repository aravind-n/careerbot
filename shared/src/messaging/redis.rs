use std::{error::Error, sync::Arc};

use async_trait::async_trait;
use redis::{AsyncCommands, Client, aio::MultiplexedConnection};
use serde_json::Value;
use tracing::error;

use crate::messaging::{
    MessageConsumer, MessageConsumerFactory, MessagePublisher, MessagePublisherFactory,
};

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
    connection: MultiplexedConnection,
    stream_key: String,
    group: String,
    consumer_name: String,
}

impl RedisStreamConsumer {
    pub async fn new(
        endpoint: &str,
        stream_key: &str,
        group: &str,
        consumer_name: &str,
    ) -> Result<Self, Box<dyn Error>> {
        let client = Client::open(endpoint).map_err(|e| {
            error!(error = %e, "Error creating redis client");
            e
        })?;

        let mut connection = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| {
                error!(error = %e, "Error creating redis connection");
                e
            })?;

        // Ensure group exists
        redis::cmd("XGROUP")
            .arg("CREATE")
            .arg(stream_key)
            .arg(group)
            .arg("$")
            .arg("MKSTREAM")
            .query_async(&mut connection)
            .await
            .or_else(|e| {
                if e.to_string().contains("BUSYGROUP") {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;

        Ok(Self {
            connection,
            stream_key: stream_key.into(),
            group: group.into(),
            consumer_name: consumer_name.into(),
        })
    }
}

#[async_trait]
impl MessageConsumer for RedisStreamConsumer {
    async fn next(&self) -> Result<Option<Value>, Box<dyn Error>> {
        let mut conn = self.connection.clone();
        let stream_key = &self.stream_key;
        let group = &self.group;
        let consumer_name = &self.consumer_name;

        let result: Option<redis::streams::StreamReadReply> = redis::cmd("XREADGROUP")
            .arg("GROUP")
            .arg(group)
            .arg(consumer_name)
            .arg("COUNT")
            .arg(1)
            .arg("BLOCK")
            .arg(5000)
            .arg("STREAMS")
            .arg(stream_key)
            .arg(">")
            .query_async(&mut conn)
            .await?;

        if let Some(reply) = result {
            for stream in reply.keys {
                for entry in stream.ids {
                    if let Some(redis_value) = entry.map.get("data") {
                        let str_value: String = redis::from_redis_value(redis_value)?;
                        let json: Value = serde_json::from_str(&str_value)?;
                        return Ok(Some(json));
                    }
                }
            }
        }

        Ok(None)
    }
}

pub struct RedisStreamPublisherFactory;

#[async_trait]
impl MessagePublisherFactory for RedisStreamPublisherFactory {
    async fn init(endpoint: &str, key: &str) -> Result<Arc<dyn MessagePublisher>, Box<dyn Error>> {
        Ok(Arc::new(RedisStreamPublisher::new(endpoint, key)?))
    }
}

pub struct RedisStreamConsumerFactory;

#[async_trait]
impl MessageConsumerFactory for RedisStreamConsumerFactory {
    async fn init(
        endpoint: &str,
        key: &str,
        group: &str,
        consumer_name: &str,
    ) -> Result<Box<dyn MessageConsumer>, Box<dyn Error>> {
        let stream_consumer = RedisStreamConsumer::new(endpoint, key, group, consumer_name).await?;

        Ok(Box::new(stream_consumer))
    }
}
