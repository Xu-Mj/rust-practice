use std::{sync::Arc, time::Duration};

use abi::config::Config;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};
use utils::typos::Registration;

use crate::{client::Client, manager::Manager, rpc::MsgRpcService};

/// 常量 心跳消息的发送间隔时间（秒）。
pub const HEART_BEAT_INTERVAL: u64 = 30;
#[derive(Clone)]
pub struct AppState {
    manager: Manager,
}
pub async fn start(config: Config) {
    // 创建通道，用来从websocket连接中向manager发送消息。
    let (tx, rx) = mpsc::channel(1024);
    let hub = Manager::new(tx, &config).await;
    let mut cloned_hub = hub.clone();
    tokio::spawn(async move {
        cloned_hub.run(rx).await;
    });
    let app_state = AppState {
        manager: hub.clone(),
    };
    // 定义一个处理WebSocket连接的路由。
    let router = Router::new()
        .route("/ws/:user_id/conn/:pointer_id", get(websocket_handler))
        .with_state(app_state);
    let addr = format!("{}:{}", config.websocket.host, config.websocket.port);

    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    info!("start websocket server on {}", addr);
    let mut ws = tokio::spawn(async move {
        println!("start websocket server on {}", addr);
        axum::serve(listener, router).await.unwrap();
    });

    // register websocket service to consul
    register_service(&config).await;

    let config = config.clone();
    let mut rpc = tokio::spawn(async move {
        // start rpc server
        MsgRpcService::start(hub, &config).await;
    });
    tokio::select! {
        _ = (&mut ws) => ws.abort(),
        _ = (&mut rpc) => rpc.abort(),
    }
}
async fn register_service(config: &Config) {
    // register service to service register center
    let center = utils::service_register_center(config);
    let registration = Registration {
        id: format!(
            "{}-{}",
            utils::get_host_name().unwrap(),
            &config.websocket.name
        ),
        name: config.websocket.name.clone(),
        address: config.websocket.host.clone(),
        port: config.websocket.port,
        tags: config.websocket.tags.clone(),
    };
    center.register(registration).await.unwrap();
}
pub async fn websocket_handler(
    Path((user_id, pointer_id)): Path<(String, String)>,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| websocket(user_id, pointer_id, socket, state))
}

pub async fn websocket(user_id: String, pointer_id: String, ws: WebSocket, app_state: AppState) {
    tracing::debug!(
        "client {} connected, user id : {}",
        user_id.clone(),
        pointer_id.clone()
    );

    let mut hub = app_state.manager.clone();

    // 将websocket连接切分成发送端和接收端
    let (ws_tx, mut ws_rx) = ws.split();
    // 将发送端进行包装，使得其能够在进程间安全的克隆
    let shared_tx = Arc::new(RwLock::new(ws_tx));
    let client = Client {
        user_id: user_id.clone(),
        platform_id: pointer_id.clone(),
        sender: shared_tx.clone(),
    };
    // 注册客户端
    hub.register(user_id.clone(), client).await;

    // send ping message to client
    let cloned_tx = shared_tx.clone();
    let mut ping_task = tokio::spawn(async move {
        loop {
            if let Err(e) = cloned_tx
                .write()
                .await
                .send(Message::Ping(Vec::new()))
                .await
            {
                error!("send ping error：{:?}", e);
                // break this task, it will end this conn
                break;
            }
            tokio::time::sleep(Duration::from_secs(HEART_BEAT_INTERVAL)).await;
        }
    });

    // spawn a new task to receive message
    let cloned_hub = hub.clone();
    let shared_tx = shared_tx.clone();
    // receive message from client
    let mut rec_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            // 处理消息
            match msg {
                Message::Text(text) => {
                    let result = serde_json::from_str(&text);
                    if result.is_err() {
                        error!("deserialize error: {:?}； source: {text}", result.err());
                        continue;
                    }

                    if cloned_hub.broadcast(result.unwrap()).await.is_err() {
                        // if broadcast not available, close the connection
                        break;
                    }
                }
                Message::Ping(_) => {
                    if let Err(e) = shared_tx
                        .write()
                        .await
                        .send(Message::Pong(Vec::new()))
                        .await
                    {
                        error!("reply ping error : {:?}", e);
                        break;
                    }
                }
                Message::Pong(_) => {
                    // tracing::debug!("received pong message");
                }
                Message::Close(info) => {
                    if let Some(info) = info {
                        tracing::warn!("client closed {}", info.reason);
                    }
                    break;
                }
                Message::Binary(_) => {}
            }
        }
    });

    tokio::select! {
        _ = (&mut ping_task) => rec_task.abort(),
        _ = (&mut rec_task) => ping_task.abort(),
    }

    // lost the connection, remove the client from hub
    hub.unregister(user_id, pointer_id).await;
    tracing::debug!("client thread exit {}", hub.hub.iter().count());
}
