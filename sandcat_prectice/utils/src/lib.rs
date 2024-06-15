mod consul;
mod tonic_service_discovery;
pub mod typos;
pub use tonic_service_discovery::*;

use crate::typos::{Registration, Service};
use abi::config::Config;
use abi::errors::Error;
use async_trait::async_trait;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::Arc;

pub type Services = HashMap<String, Service>;

/// the service register discovery center
#[async_trait]
pub trait ServiceRegister: Send + Sync + Debug {
    /// service register
    async fn register(&self, registration: Registration) -> Result<(), Error>;

    /// filter
    async fn filter_by_name(&self, name: &str) -> Result<Services, Error>;
}

pub fn service_register_center(config: &Config) -> Arc<dyn ServiceRegister> {
    Arc::new(consul::Consul::from_config(config))
}

pub fn get_host_name() -> Result<String, Error> {
    let hostname = hostname::get().map_err(|err| Error::InternalServer(err.to_string()))?;
    let hostname = hostname.into_string().map_err(|_| {
        Error::InternalServer(String::from(
            "get hostname error: OsString into String Failed",
        ))
    })?;
    Ok(hostname)
}
