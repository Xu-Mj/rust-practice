use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub websocket: WsServerConfig,
    pub kafka: KafkaConfig,
    pub rpc: RpcConfig,
    pub redis: RedisConfig,
    pub service_center: ServiceCenterConfig,
    pub db: PostgresConfig,
}

impl Config {
    pub fn load(filename: impl AsRef<Path>) -> Self {
        let content = fs::read_to_string(filename).unwrap();
        serde_yaml::from_str(&content).unwrap()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PostgresConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
    #[serde(default = "default_conn")]
    pub max_connections: u32,
}

fn default_conn() -> u32 {
    5
}
impl PostgresConfig {
    pub fn server_url(&self) -> String {
        if self.password.is_empty() {
            return format!("postgres://{}@{}:{}", self.user, self.host, self.port);
        }
        format!(
            "postgres://{}:{}@{}:{}",
            self.user, self.password, self.host, self.port
        )
    }
    pub fn url(&self) -> String {
        format!("{}/{}", self.server_url(), self.database)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServiceCenterConfig {
    pub host: String,
    pub port: u16,
    pub protocol: String,
    pub timeout: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WsServerConfig {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub name: String,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KafkaConfig {
    pub hosts: Vec<String>,
    pub topic: String,
    pub group: String,
    pub producer: KafkaProducer,
    pub consumer: KafkaConsumer,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KafkaProducer {
    pub timeout: u16,
    pub acks: String,
    pub max_retry: u8,
    pub retry_interval: u16,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KafkaConsumer {
    pub session_timeout: u16,
    pub auto_offset_reset: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcConfig {
    pub ws: RpcServerConfig,
    pub chat: RpcServerConfig,
    pub db: RpcServerConfig,
    pub pusher: RpcServerConfig,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpcServerConfig {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub name: String,
    pub tags: Vec<String>,
    // pub grpc_health_check: GrpcHealthCheck,
}

impl RpcServerConfig {
    #[inline]
    pub fn rpc_server_url(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    #[inline]
    pub fn url(&self, https: bool) -> String {
        url(https, &self.host, self.port)
    }
}

fn url(https: bool, host: &str, port: u16) -> String {
    if https {
        format!("https://{}:{}", host, port)
    } else {
        format!("http://{}:{}", host, port)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RedisConfig {
    pub host: String,
    pub port: u16,
}

impl RedisConfig {
    pub fn url(&self) -> String {
        format!("redis://{}:{}", self.host, self.port)
    }
}
