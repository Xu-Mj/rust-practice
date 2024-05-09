use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    pub websocket: WsServerConfig,
}

impl Config {
    pub fn load(filename: impl AsRef<Path>) -> Self {
        let content = fs::read_to_string(filename).unwrap();
        serde_yaml::from_str(&content).unwrap()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WsServerConfig {
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub name: String,
    pub tags: Vec<String>,
}
