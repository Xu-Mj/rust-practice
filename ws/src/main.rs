use tracing::Level;

mod client;
mod manager;
mod rpc;
pub mod ws_server;
#[tokio::main]
async fn main() {
    // 设置日志等级
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();
    let config = abi::config::Config::load("./abi/config.yaml");
    ws_server::start(config).await;
}
