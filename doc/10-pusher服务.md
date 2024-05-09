## Pusher服务

pusher服务用来将消息推送到ws服务中，实现消息发送的闭环

### 创建模块以及引入依赖

```shell
cargo new pusher

cargo add abi -p pusher 
cargo add utils -p pusher
cargo add async-trait -p pusher
cargo add tonic -p pusher --features=gzip
cargo add tokio -p pusher --features=full
cargo add tracing -p pusher  
cargo add tracing_subscriber -p pusher
cargo add dashmap -p pusher
cargo add tower -p pusher
```

### 增加protobuf定义

abi/protos/messages.proto

```protobuf
// SendMsgRequest SendMsgResponse前面我们已经定义了 
service PushService {
  rpc PushSingleMsg(SendMsgRequest) returns (SendMsgResponse);
}
```

### 实现服务注册、服务发现、以及启动方法

定义pusher服务对象

pusher/src/main.rs

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;
use tonic::transport::{Channel, Endpoint, Server};
use tonic::{async_trait, Request, Response, Status};
use tower::discover::Change;
use tracing::{debug, error, info, Level};

use abi::config::Config;
use abi::message::msg_service_client::MsgServiceClient;
use abi::message::push_service_server::{PushService, PushServiceServer};
use abi::message::{SendMsgRequest, SendMsgResponse};
use utils::typos::Registration;
use utils::DynamicServiceDiscovery;

pub struct PusherRpcService {
    ws_rpc_list: Arc<DashMap<SocketAddr, MsgServiceClient<Channel>>>,
}
```

**说明：** 这里只需要维护一个ws服务的客户端列表即可，因为需要在运行期间动态的调整这个列表，所以通过`Arc`（原子引用计数）封装，就可以在多个异步任务之间共享和修改WebSocket服务的客户端列表

### 实现相关方法

**new方法**

pusher/src/main.rs

```rust
pub async fn new(config: &Config) -> Self {
    let register = utils::service_register_center(config);
    let ws_rpc_list = Arc::new(DashMap::new());
    let cloned_list = ws_rpc_list.clone();
    let (tx, mut rx) = mpsc::channel::<Change<SocketAddr, Endpoint>>(100);

    // read the service from the worker
    tokio::spawn(async move {
        while let Some(change) = rx.recv().await {
            match change {
                Change::Insert(service_id, client) => {
                    match MsgServiceClient::connect(client).await {
                        Ok(client) => {
                            cloned_list.insert(service_id, client);
                        }
                        Err(err) => {
                            error!("connect to ws service error: {:?}", err);
                        }
                    };
                }
                Change::Remove(service_id) => {
                    cloned_list.remove(&service_id);
                }
            }
        }
    });

    let worker = DynamicServiceDiscovery::new(
        register,
        config.rpc.ws.name.clone(),
        tokio::time::Duration::from_secs(10),
        tx,
        config.rpc.ws.protocol.clone(),
    );

    // start the worker
    tokio::spawn(worker.run());

    Self { ws_rpc_list }
}
```

**说明：**在这个分布式的微服务架构中，为了处理负载和故障恢复，我们可能会有多个WebSocket服务实例（即ws服务）运行。随着时间的推移，系统可能需要根据流量和性能的需求动态地增加或删除ws服务实例。要实现这种动态性，我们不能依赖于静态的服务列表，而需要一个机制来不断监测服务的状态，并及时对服务列表进行更新。

我们的`new`方法就应对了这个需求。首先，它通过`utils::service_register_center()`方法连接到了服务注册中心，并创建了`ws_rpc_list`，这是一个映射，用于跟踪活动的WebSocket服务连接。

为了动态管理这些服务连接，我们设立了一个异步任务，这个任务不断监听来自另一个异步任务的通知，这个通知任务会每隔一段时间（比如这里是10秒钟）向注册中心查询最新状态，并通过一个通道传递服务实例的变化信息。每当服务实例出现或消失时，这个通道就会传递一个`Change`消息给监听任务。

当监听到`Change::Insert`事件时，代码会尝试与新的服务实例建立连接。如果连接成功，则把这个新的连接添加到`ws_rpc_list`映射中，这样消息就可以推送给这个新服务实例了。若连接失败，系统会记录这个错误，但不会停止尝试连接新服务实例的过程。

相反地，当监听到`Change::Remove`事件时，代表某个服务实例不再可用或被从服务列表中移除。对应的，我们也会从`ws_rpc_list`映射中移除不再需要的连接。

启动动态服务发现的异步工作流（worker）是通过创建一个`DynamicServiceDiscovery`实例实现的，这个实例负责定期与服务注册中心通信，并将服务实例变更信息发送到之前提到的通道。

最后，`new`方法完成了服务启动准备，并返回了一个新的`PusherRpcService`实例。这个实例已经准备好响应系统内部的各种变化，确保服务列表始终是最新的，以便可靠地将消息推送到所有活跃的WebSocket服务实例上。

**start、register_service方法**

跟我们前面的服务相同，将自身服务信息注册到注册中心，然后启动rpc服务

pusher/src/main.rs

```rust
impl PusherRpcService { 
    pub async fn start(config: &Config) {
        // register service
        Self::register_service(config).await;
        info!("<pusher> rpc service register to service register center");

        let pusher_rpc = Self::new(config).await;
        let service = PushServiceServer::new(pusher_rpc);
        info!(
            "<pusher> rpc service started at {}",
            config.rpc.pusher.rpc_server_url()
        );

        Server::builder()
            .add_service(service)
            .serve(config.rpc.pusher.rpc_server_url().parse().unwrap())
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
                &config.rpc.pusher.name
            ),
            name: config.rpc.pusher.name.clone(),
            address: config.rpc.pusher.host.clone(),
            port: config.rpc.pusher.port,
            tags: config.rpc.pusher.tags.clone(),
        };
        center.register(registration).await.unwrap();
    }

}
```

### 实现gRPC接口

pusher/src/main.rs

```rust

#[async_trait]
impl PushService for PusherRpcService {
    async fn push_single_msg(
        &self,
        request: Request<SendMsgRequest>,
    ) -> Result<Response<SendMsgResponse>, Status> {
        debug!("push msg request: {:?}", request);
        // extract request
        let request = request.into_inner();

        let ws_rpc = self.ws_rpc_list.clone();
        let (tx, mut rx) = mpsc::channel(ws_rpc.len());

        // send message to ws with asynchronous way
        for v in ws_rpc.iter() {
            let tx = tx.clone();
            let service_id = *v.key();
            let mut v = v.clone();
            let request = request.clone();
            tokio::spawn(async move {
                if let Err(err) = v.send_msg_to_user(request).await {
                    tx.send((service_id, err)).await.unwrap();
                };
            });
        }

        // close tx
        drop(tx);

        while let Some((service_id, err)) = rx.recv().await {
            ws_rpc.remove(&service_id);
            error!("push msg to {} failed: {}", service_id, err);
        }
        Ok(Response::new(SendMsgResponse {}))
    }
}
```

`push_single_msg`方法，服务实现gRPC接口，允许客户端通过RPC调用该方法来请求消息的推送。由于需要向所有WebSocket服务实例推送消息，我们创建一个新的异步任务来处理每个WebSocket服务实例的消息发送。`ws_rpc_list`是当前所有活跃的WebSocket服务实例的连接列表。

对于`ws_rpc_list`中的每一个服务实例，函数都会克隆`tx`发送端，创建一个新的异步任务来异步发送消息。如果在发送过程中遇到错误，就通过`tx`发送一个包含服务实例地址和错误信息的元组。

在发送消息给所有服务后，主动调用`drop(tx)`来关闭`tx`发送端。这样，当所有的消息发送任务完成时，接收端`rx`将不再接收到新的消息，从而退出监听循环。

在接收端的循环中，我们会接收异步任务发送来的错误报告。每当有错误发生，意味着某个WebSocket服务实例可能出现了问题。此时，我们将该服务实例从`ws_rpc_list`中移除，并且记录错误日志

### 修改main方法，启动pusher服务

pusher/src/main.rs

```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();
    let config = abi::config::Config::load("./abi/config.yaml");
    PusherRpcService::start(&config).await;
}
```

**总结：** 在开发pusher服务的过程中，我们熟悉了Rust的异步编程范式，特别是多任务间的通信和服务的发现机制。实现了高效地进行并发RPC调用，并通过通道（channel）来有效地收集和处理这些调用的执行结果。