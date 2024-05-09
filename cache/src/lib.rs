use std::fmt::Debug;
use std::sync::Arc;

use async_trait::async_trait;

use ::redis::RedisError;
use abi::config::Config;

mod redis;

#[async_trait]
pub trait Cache: Sync + Send + Debug {
    /// 获取seq
    async fn get_seq(&self, user_id: &str) -> Result<i64, RedisError>;
    /// 增加指定用户的sequence
    async fn increase_seq(&self, user_id: &str) -> Result<i64, RedisError>;
}

pub fn cache(config: &Config) -> Arc<dyn Cache> {
    Arc::new(redis::RedisCache::from_config(config))
}
