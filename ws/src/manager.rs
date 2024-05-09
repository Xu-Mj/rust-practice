use std::sync::Arc;

use abi::{
    config::Config,
    message::{chat_service_client::ChatServiceClient, ContentType, Msg, MsgType, SendMsgRequest},
};
use dashmap::DashMap;
use tokio::sync::mpsc::{self, error::SendError};
use tracing::{debug, error, info};
use utils::LbWithServiceDiscovery;

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
    /// 用来接收websocket收到的消息
    tx: mpsc::Sender<Msg>,
    /// 存储用户与服务端的连接
    pub hub: Hub,
    /// 新增。chat服务的gRPC客户端
    pub chat_rpc: ChatServiceClient<LbWithServiceDiscovery>,
}
impl Manager {
    /// 修改new方法，增加chat_rpc客户端获取
    pub async fn new(tx: mpsc::Sender<Msg>, config: &Config) -> Self {
        let chat_rpc = Self::get_chat_rpc_client(config).await;
        Self {
            chat_rpc,
            tx,
            hub: Arc::new(DashMap::new()),
        }
    }

    /// 新增rpc客户端获取方法
    async fn get_chat_rpc_client(config: &Config) -> ChatServiceClient<LbWithServiceDiscovery> {
        // use service register center to get ws rpc url
        let channel = utils::get_channel_with_config(
            config,
            &config.rpc.chat.name,
            &config.rpc.chat.protocol,
        )
        .await
        .unwrap();
        ChatServiceClient::new(channel)
    }

    /// 修改run方法，将消息数据通过gRPC的方式发送到chat服务中
    pub async fn run(&mut self, mut receiver: mpsc::Receiver<Msg>) {
        info!("manager start");
        // 循环读取消息
        while let Some(mut message) = receiver.recv().await {
            // 请求chat服务，将消息发送到chat服务中，chat服务会返回一个server_id和send_time
            debug!("receive message: {:?}", message);
            match self
                .chat_rpc
                .send_msg(SendMsgRequest {
                    message: Some(message.clone()),
                })
                .await
            {
                Ok(res) => {
                    // 请求成功，我们就认为这条消息发送成功了，向发送者返回发送成功的消息
                    let response = res.into_inner();
                    if response.err.is_empty() {
                        debug!("send message success");
                    } else {
                        error!("send message error: {:?}", response.err);
                        message.content_type = ContentType::Error as i32;
                    }
                    message.msg_type = MsgType::MsgRecResp as i32;
                    message.server_id = response.server_id.clone();
                    message.content = response.err;
                }
                Err(err) => {
                    error!("send message error: {:?}", err);
                    message.content = err.to_string();
                }
            }

            // reply result to sender
            println!("reply message:{:?}", message);
            self.send_single_msg(&message.send_id, &message).await;
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

    pub async fn broadcast(&self, msg: Msg) -> Result<(), SendError<Msg>> {
        self.tx.send(msg).await
    }
}
