use std::sync::Arc;

use abi::message::Msg;
use dashmap::DashMap;
use tokio::sync::mpsc::{self, error::SendError};
use tracing::{debug, error, info};

use crate::client::Client;

/// 类型别名
/// 用户id
type UserID = String;
/// 平台id
type PlatformID = String;
/// 客户端连接池
type Hub = Arc<DashMap<UserID, DashMap<PlatformID, Client>>>;

/// manage the client
#[derive(Clone)]
pub struct Manager {
    /// 用来接收websocket收到的消息; tokio::sync::mpsc; Msg是我们基础设置中生成的Msg结构体
    tx: mpsc::Sender<Msg>,
    /// 存储用户与服务端的连接
    pub hub: Hub,
}

impl Manager {
    pub fn new(tx: mpsc::Sender<Msg>) -> Self {
        Self {
            tx,
            hub: Arc::new(DashMap::new()),
        }
    }
    pub async fn register(&mut self, user_id: UserID, client: Client) {
        let entry = self.hub.entry(user_id.clone()).or_insert_with(DashMap::new);
        entry.insert(client.platform_id.clone(), client);
    }
    pub async fn unregister(&mut self, user_id: UserID, platform_id: PlatformID) {
        if let Some(user_clients) = self.hub.get_mut(&user_id) {
            user_clients.remove(&platform_id);
        }
    }

    async fn send_msg_to_clients(&self, clients: &DashMap<PlatformID, Client>, msg: &Msg) {
        for client in clients.iter() {
            let content = match serde_json::to_string(msg) {
                Ok(res) => res,
                Err(_) => {
                    error!("msg serialize error");
                    return;
                }
            };
            if let Err(e) = client.value().send_text(content).await {
                error!("msg send error: {:?}", e);
            }
        }
    }
    pub async fn send_single_msg(&self, user_id: &UserID, msg: &Msg) {
        if let Some(clients) = self.hub.get(user_id) {
            self.send_msg_to_clients(&clients, msg).await;
        }
    }

    pub async fn run(&mut self, mut receiver: mpsc::Receiver<Msg>) {
        info!("manager start");
        // 循环读取消息
        while let Some(message) = receiver.recv().await {
            // request the message rpc to get server_msg_id
            debug!("receive message: {:?}", message);
            self.send_single_msg(&message.receiver_id, &message).await;
        }
    }

    pub async fn broadcast(&self, msg: Msg) -> Result<(), SendError<Msg>> {
        self.tx.send(msg).await
    }
}