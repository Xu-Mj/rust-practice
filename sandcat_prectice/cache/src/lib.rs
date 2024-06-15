use std::fmt::Debug;
use std::sync::Arc;

use abi::errors::Error;
use async_trait::async_trait;

use abi::config::Config;

mod redis;

#[async_trait]
pub trait Cache: Sync + Send + Debug {
    /// query sequence by user id
    async fn get_seq(&self, user_id: &str) -> Result<i64, Error>;
    async fn increase_seq(&self, user_id: &str) -> Result<i64, Error>;
}

pub fn cache(config: &Config) -> Arc<dyn Cache> {
    Arc::new(redis::RedisCache::from_config(config))
}
