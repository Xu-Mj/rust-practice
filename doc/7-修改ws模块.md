## 修改ws模块，增加gRPC相关实现

### 增加依赖以及创建文件

```shell
# 用来将ws rpc服务注册到服务中心，供后面的pusher服务调用
cargo add utils -p ws 
# 实现gRPC接口
cargo add tonic -p ws 

# 创建rpc文件
touch ws/src/rpc.rs
```

将rpc模块加入到crate中， 修改ws/src/main.rs

```rust
// 增加
mod rpc;
```

### 增加protobuf定义

```protobuf
// SendMsgRequest我们在前面已经定义了，这里直接复用即可
message SendMsgResponse {}

service MsgService {
  // send message through rpc
  rpc SendMessage(SendMsgRequest) returns (SendMsgResponse);
  // send single message to user by websocket
  rpc SendMsgToUser(SendMsgRequest) returns (SendMsgResponse);
}
```

### 修改manager，增加chat服务gRPC客户端

manager是管理客户端连接和消息传输的核心，我们需要在其中添加gRPC客户端的支持，以便可以通过gRPC发送消息

ws/src/manager.rs

```rust
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
```

### 修改manager相关方法

ws/src/manager.rs

```rust
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
```

### 增加启动方法和服务注册方法

ws/src/rpc.rs

```rust
use std::result::Result;

use abi::config::Config;
use abi::message::msg_service_server::MsgServiceServer;
use abi::message::{msg_service_server::MsgService, SendMsgRequest, SendMsgResponse};
use tonic::transport::Server;
use tonic::{async_trait, Request, Response, Status};
use tracing::{debug, info};
use utils::typos::Registration;

use crate::manager::Manager;

pub struct MsgRpcService {
    manager: Manager,
}

impl MsgRpcService {
    pub fn new(manager: Manager) -> Self {
        Self { manager }
    }

    pub async fn start(manager: Manager, config: &Config) {
        // register service to service register center
        Self::register_service(config).await;
        info!("<ws> rpc service register to service register center");

        let service = Self::new(manager);
        let svc = MsgServiceServer::new(service);
        info!(
            "<ws> rpc service started at {}",
            config.rpc.ws.rpc_server_url()
        );

        Server::builder()
            .add_service(svc)
            .serve(config.rpc.ws.rpc_server_url().parse().unwrap())
            .await
            .unwrap();
    }

    async fn register_service(config: &Config) {
        // register service to service register center
        let center = utils::service_register_center(config);

        let registration = Registration {
            id: format!(
                "{}-{}",
                utils::get_host_name().unwrap(),
                &config.rpc.ws.name
            ),
            name: config.rpc.ws.name.clone(),
            address: config.rpc.ws.host.clone(),
            port: config.rpc.ws.port,
            tags: config.rpc.ws.tags.clone(),
        };
        center.register(registration).await.unwrap();
    }
}
```

### 实现gRPC接口

ws/src/rpc.rs

```rust
#[async_trait]
impl MsgService for MsgRpcService {
    async fn send_message(
        &self,
        request: Request<SendMsgRequest>,
    ) -> Result<Response<SendMsgResponse>, Status> {
        debug!("Got a request: {:?}", request);
        let msg = request
            .into_inner()
            .message
            .ok_or_else(|| Status::invalid_argument("message is empty"))?;
        self.manager
            .broadcast(msg)
            .await
            .map_err(|err| Status::internal(err.to_string()))?;
        let response = Response::new(SendMsgResponse {});
        Ok(response)
    }

    /// Send message to user
    /// pusher will procedure this to send message to user
    async fn send_msg_to_user(
        &self,
        request: Request<SendMsgRequest>,
    ) -> Result<Response<SendMsgResponse>, Status> {
        let msg = request
            .into_inner()
            .message
            .ok_or_else(|| Status::invalid_argument("message is empty"))?;
        debug!("send message to user: {:?}", msg);
        self.manager.send_single_msg(&msg.receiver_id, &msg).await;
        let response = Response::new(SendMsgResponse {});
        Ok(response)
    }
}

```

### 增加websocket服务的服务注册方法

ws/src/ws_server.rs

```rust
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
```

### 修改start方法，实现websocket服务注册以及运行gRPC服务

ws/src/ws_server.rs

```rust
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
```

**总结：**

通过上述步骤，我们成功地在WebSocket模块中集成了gRPC的支持，允许WebSocket服务与其他gRPC服务进行交互，从而实现了将用户发送过来的消息通过chat服务发布到kafka中。