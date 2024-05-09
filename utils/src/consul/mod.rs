use std::collections::HashMap;

use abi::config::Config;
use abi::errors::Error;
use async_trait::async_trait;

use crate::typos::Registration;
use crate::{ServiceRegister, Services};

/// consul options
#[derive(Debug, Clone)]
pub struct ConsulOptions {
    pub host: String,
    pub port: u16,
    pub protocol: String,
    pub timeout: u64,
}

impl ConsulOptions {
    pub fn from_config(config: &Config) -> Self {
        Self {
            host: config.service_center.host.clone(),
            port: config.service_center.port,
            timeout: config.service_center.timeout,
            protocol: config.service_center.protocol.clone(),
        }
    }
}


#[derive(Debug, Clone)]
pub struct Consul {
    pub options: ConsulOptions,
    pub client: reqwest::Client,
}

impl Consul {
    pub fn from_config(config: &Config) -> Self {
        let options = ConsulOptions::from_config(config);
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_millis(options.timeout))
            .no_proxy()
            .build()
            .unwrap();
        Self { options, client }
    }

    pub fn api_url(&self, name: &str) -> String {
        self.url("agent", name)
    }

    fn url(&self, type_: &str, name: &str) -> String {
        format!(
            "{}://{}:{}/v1/{}/{}",
            self.options.protocol, self.options.host, self.options.port, type_, name
        )
    }
}

#[async_trait]
impl ServiceRegister for Consul {
    async fn register(&self, registration: Registration) -> Result<(), Error> {
        let url = self.api_url("service/register");
        let response = self.client.put(&url).json(&registration).send().await?;
        if !response.status().is_success() {
            return Err(Error::InternalServer(
                response.text().await.unwrap_or_default(),
            ));
        }
        Ok(())
    }

    async fn filter_by_name(&self, name: &str) -> Result<Services, Error> {
        let url = self.api_url("services");
        let mut map = HashMap::new();
        map.insert("filter", format!("Service == {}", name));

        let services = self
            .client
            .get(url)
            .query(&map)
            .send()
            .await?
            .json::<Services>()
            .await?;
        Ok(services)
    }
}