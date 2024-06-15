mod postgres;

use std::sync::Arc;

use abi::config::Config;
use async_trait::async_trait;

use abi::errors::Error;
use abi::message::Msg;

use self::postgres::PostgresMessage;

/// face to postgres db
#[async_trait]
pub trait MsgStoreRepo: Sync + Send {
    /// save message to db
    async fn save_message(&self, message: Msg) -> Result<(), Error>;
}

pub async fn msg_repo(config: &Config) -> Arc<dyn MsgStoreRepo> {
    Arc::new(PostgresMessage::from_config(config).await)
}
