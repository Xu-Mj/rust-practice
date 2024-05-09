use crate::Cache;
use abi::config::Config;
use async_trait::async_trait;
use redis::{AsyncCommands, RedisError};

#[derive(Debug)]
pub struct RedisCache {
    client: redis::Client,
}

impl RedisCache {
    #[allow(dead_code)]
    pub fn new(client: redis::Client) -> Self {
        Self { client }
    }
    pub fn from_config(config: &Config) -> Self {
        // Intentionally use unwrap to ensure Redis connection at startup.
        // Program should panic if unable to connect to Redis, as it's critical for operation.
        let client = redis::Client::open(config.redis.url()).unwrap();
        RedisCache { client }
    }
}

#[async_trait]
impl Cache for RedisCache {
    async fn get_seq(&self, user_id: &str) -> Result<i64, RedisError> {
        // generate key
        let key = format!("seq:{}", user_id);

        let mut conn = self.client.get_multiplexed_async_connection().await?;

        // increase seq
        let seq: i64 = conn.get(&key).await.unwrap_or_default();
        Ok(seq)
    }

    async fn increase_seq(&self, user_id: &str) -> Result<i64, RedisError> {
        // generate key
        let key = format!("seq:{}", user_id);

        let mut conn = self.client.get_multiplexed_async_connection().await?;

        // increase seq
        let seq: i64 = conn.incr(&key, 1).await?;
        Ok(seq)
    }
}
