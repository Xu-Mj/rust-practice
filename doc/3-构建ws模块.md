# ws模块

**创建crate引入相关依赖**

```SHELL
cargo new ws
# 引入依赖
cargo add abi -p ws
cargo add axum --features=ws -p ws
cargo add dashmap -p ws
cargo add futures -p ws
cargo add serde_json -p ws
cargo add tokio --features full -p ws 
cargo add tracing -p ws
cargo add tracing-subscriber -p ws
```

**创建相关文件**

```shell
touch ws/src/client.rs
touch ws/src/manager.rs
touch ws/src/ws_server.rs
```

修改main.rs文件，导出我们刚刚创建的模块

ws/src/main.rs

```rsut
mod client;
mod manager;
pub mod ws_server;
```

#### 实现WebSocket服务器的功能

### Client 结构体

ws/src/client.rs

```rust
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures::{stream::SplitSink, SinkExt};
use tokio::sync::RwLock;

type ClientSender = Arc<RwLock<SplitSink<WebSocket, Message>>>;
```

`ClientSender`是一个类型别名，它是`Arc`和`RwLock`的嵌套用法，`Arc`保证跨多个异步任务共享，并且`RwLock`使得同时只有一个任务能够写入数据到`WebSocket`， 注意RwLock是tokio::sync::RwLock。

`SplitSink`来自`futures`库，它是一个`WebSocket`的一个分离的发送半部分，用于处理发送消息的能力。这允许`WebSocket`流同时进行读和写操作，而不会相互阻塞。

ws/src/client

```rust
pub struct Client {
    pub sender: ClientSender,
    pub user_id: String,
    pub platform_id: String,
}
```

`Client`结构体包含以下主要字段：

- `sender`：客户端的发送通道，通过它发送消息。
- `user_id`：用户的唯一标识符。
- `platform_id`：用于识别用户的平台（设备）ID。

### impl Client

`Client`结构体实现了两个异步方法`send_text`和`send_binary`，用于向WebSocket连接发送文本和二进制数据。

ws/src/client.rs

```rust
impl Client {
    pub async fn send_text(&self, msg: String) -> Result<(), axum::Error> {
    	self.sender.write().await.send(Message::Text(msg)).await
	}

    #[allow(dead_code)]
	pub async fn send_binary(&self, msg: Vec<u8>) -> Result<(), axum::Error> {
    	self.sender.write().await.send(Message::Binary(msg)).await
	}
}
```

每个方法都调用`write`方法来获取到`sender`的可写锁，以便发送消息。这里使用`.await`是因为`write()`操作是异步的，它可能需要等待其他任务释放锁。之后通过`send`方法将`Message::Text`或`Message::Binary`发送出去。如果成功，这些方法将返回`Ok(())`，否则在遇到错误时返回`Err(axum::Error)`。由于目前我们暂时只用到了send_text方法，因此编译期会发出send_binary未使用的警告，我们使用#[allow(dead_code)]进行标记即可。

#### 管理客户端连接

ws/src/manager.rs

```rust
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
```

概念--类型别名：

>  在 Rust 中，我们可以使用 `type` 关键字来创建一个类型别名（Type Alias），这为原有的类型赋予了一个新的名字。例如，通过声明 `type UserID = String;`，我们定义了 `UserID` 作为 `String` 类型的别名。这样做不仅可以提高代码的可读性，让类型的用途更加明显，也有助于我们在复杂类型的场景下简化代码的编写。类型别名本质上不创建新的类型，它只是给现有的类型一个新的名字。`Manager` 结构体在这个 WebSocket 服务中起到一个核心的角色，它负责处理客户端的注册、注销和消息管理。`Manager` 使用 `DashMap` 数据结构，这是一个支持高并发的键值存储，非常适合在异步环境中管理客户端连接。

首先，来看一下 `Manager` 结构体中使用的 `DashMap` 类型的定义：

```rust
type Hub = Arc<DashMap<UserID, DashMap<PlatformID, Client>>>;
```

在这里，`Hub` 是一个原子引用计数（`Arc`）的 `DashMap`，其中存储了用户的 `UserID` 作为键，而值则是另一个 `DashMap`，映射 `PlatformID` 到 `Client` 对象的关系。`DashMap` 的选择是因为它提供了几个关键优势：

- **线程安全**：在多线程环境中不会有数据竞争的问题。
- **读写分离**：`DashMap` 内部实现了多个锁，读取操作通常不会阻塞其他读取，写入操作会阻塞少量相关的读写。
- **无需显式锁定**：与标准的 `Mutex` 或 `RwLock` 相比，`DashMap` 提供了一个简洁的 API，不需要编写复杂的锁定和解锁代码。

**new方法**

ws/src/manager.rs

```rust
pub fn new(tx: mpsc::Sender<Msg>) -> Self {
    Self {
        tx,
        hub: Arc::new(DashMap::new()),
    }
}
```

#### 注册和注销客户端

我们详细地看一下 `Manager` 结构体实现的几个关键方法：

**注册客户端 (`register` 方法)**：

当新的 WebSocket 连接建立时，需要创建一个新的 `Client` 并将其注册到 `Manager`：

ws/src/manager.rs

```rust
pub async fn register(&mut self, user_id: UserID, client: Client) {
    let entry = self.hub.entry(user_id.clone()).or_insert_with(DashMap::new);
    entry.insert(client.platform_id.clone(), client);
}
```

这段代码首先尝试获取用户的 `UserID` 对应的 `DashMap` 实体，如果不存在，将会初始化一个新的 `DashMap`。然后，它会在该 `DashMap` 中插入一个新的 `Client` 实例，关联至对应的 `PlatformID`。

**注销客户端 (`unregister` 方法)**：

一个客户端完成它的会话或断开连接时，它应该从 `Manager` 中移除：

ws/src/manager.rs

```rust
pub async fn unregister(&mut self, user_id: UserID, platform_id: PlatformID) {
    if let Some(user_clients) = self.hub.get_mut(&user_id) {
        user_clients.remove(&platform_id);
    }
}
```

此函数接收 `UserID` 和 `PlatformID` 作为参数，尝试从 `DashMap` 中取得对应的客户端，并从中移除。如果当前 `UserID` 下只有一个 `PlatformID` 关联的客户端，那么在移除之后整个用户的实体也会从 `hub` 中删除，整洁释放资源。

#### 管理消息

`Manager` 负责处理客户端的消息。它包含发送和接收消息的功能，如一对一聊天消息 (`send_single_msg`)。这些功能确保消息能被送达给特定的客户端或一组客户端。

**消息发送方法**

ws/src/manager.rs

```rust
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
```

**单发消息 (`send_single_msg` 方法)**：

向单个客户端发送消息只需找到对应的 `Client` 实例并发送消息：

ws/src/manager.rs

```rust
pub async fn send_single_msg(&self, user_id: &UserID, msg: &Msg) {
    if let Some(clients) = self.hub.get(user_id) {
        self.send_msg_to_clients(&clients, msg).await;
    }
}
```

**消息处理方法**

ws/src/manager.rs

```rust
pub async fn run(&mut self, mut receiver: mpsc::Receiver<Msg>) {
    info!("manager start");
    // 循环读取消息
    while let Some(message) = receiver.recv().await {
        // request the message rpc to get server_msg_id
        debug!("receive message: {:?}", message);
        self.send_single_msg(&message.receiver_id, &message).await;
    }
}
```

当websocket收到消息时将消息发送给目标用户，

**消息广播**

ws/src/manager.rs

```rust
pub async fn broadcast(&self, msg: Msg) -> Result<(), SendError<Msg>> {
    self.tx.send(msg).await
}
```

以上便是管理客户端连接的重点部分。在`Manager` 中我们使用了 Rust 的并发和异步特性来有效地处理客户端的注册、注销和消息管理。通过充分利用 `DashMap`，我们可以在高并发场景下维护高效率和代码的简洁性。

#### 构建应用状态和WebSocket服务器结构

定义`AppState`和`Manager`结构体，包含管理连接的必要组件：

ws/src/ws_server.rs

```rust
use std::{sync::Arc, time::Duration};

use abi::config::Config;
use axum::{extract::{ws::{Message, WebSocket}, Path, State, WebSocketUpgrade}, response::IntoResponse, routing::get, Router};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info};

use crate::{client::Client, manager::Manager};

/// 常量 心跳消息的发送间隔时间（秒）。
pub const HEART_BEAT_INTERVAL: u64 = 30;
#[derive(Clone)]
pub struct AppState {
    manager: Manager,
}
```

利用axum全局State我们可以在任何axum的http handler中访问共享状态，包括websocket handler。

这里，`AppState`持有`Manager`的克隆，后者负责管理客户端连接和消息。

接下来是`WsServer`结构和其方法。`WsServer`是实际WebSocket服务的主体，包含启动服务和处理连接的方法。

### websocket_handler 函数

`websocket_handler`函数是一个高阶函数，负责处理初始的WebSocket握手并将其升级为一个连接：

ws/src/ws_server.rs

```rust
pub async fn websocket_handler(
    Path((user_id, token, pointer_id)): Path<(String, String, String)>,
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    // validate token
    tracing::debug!("token is {}", token);
    ws.on_upgrade(move |socket| websocket(user_id, pointer_id, socket, state))
}
```

这里的参数包含了通过URL路径传递的`user_id`、`token`和`pointer_id`，这些是连接时需要的用户相关信息。`WebSocketUpgrade`类型的`ws`参数则处理HTTP升级WebSocket的操作。`AppState`是服务器状态的持有者，整个应用的状态会被传递到这里。

函数首先输出token用于验证日志，然后调用`on_upgrade`方法。这里的`move`关键字确保了函数内部使用的变量（user_id, pointer_id, socket, state）在异步块中仍然有效。当HTTP请求升级为WebSocket连接后，将调用`websocket`函数进行后续处理。

### websocket 函数

首先，了解到几个关键的异步编程元素：

1. **任务（Tasks）**：任务是异步程序中的基本执行单元。在`tokio`中，可以通过`tokio::spawn`来创建一个新的异步任务。
2. **通道（Channels）**：通道提供了任务之间的通讯方式。通常用来在生产者和消费者之间传递消息。
3. **共享状态（Arc、RwLock）**：在Rust中，`Arc<T>`（原子引用计数的智能指针）用来在多个任务之间共享并拥有一个值。`RwLock<T>`则提供了这个值的读写锁定功能。

`websocket`函数处理WebSocket连接的生命周期，包括消息接收、发送心跳和应答等：

ws/src/ws_server.rs

```rust
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
```

以下为此函数的关键组件和它们的作用：

- **初始化连接：** 创建新的`Client`，并注册到`Manager`中，为当前连接建立状态。
- **创建发送器（`shared_tx`）：** 为异步发送消息到客户端创建发送器，这是WebSocket连接的发送通道的包装器。
- **心跳任务（`ping_task`）：** 发送周期性的`Ping`消息以确保连接保持活跃。
- **接收任务（`rec_task`）：** 在一个循环中等待并处理来自客户端的消息。
- **消息处理：** 对于接收到的不同类型的`Message`，执行相应的操作，如反序列化文本消息、处理`Ping`和`Pong`消息，以及处理关闭(`Close`)消息。
- **退出逻辑：** 使用`tokio::select!`宏来同时运行`ping_task`和`rec_task`。如果其中一个任务完成，则终止另一个任务。
- **注销和清理：** 当连接关闭时，调用`unregister`方法将客户端从`Manager`中移除，并做相应的日志记录与资源清理。

上面所述就是这两个函数的具体行为和编程模式。它们处理从建立连接到注销客户端整个生命周期中的所有关键行为。通过这样的设计，我们能够有效地管理每个WebSocket连接，并维护服务的稳健性和可靠性。

### `start`方法

ws/src/ws_server.rs

```rust
pub async fn start(config: Config) {
    // 创建通道，用来从websocket连接中向manager发送消息。
    let (tx, rx) = mpsc::channel(1024);
    let hub = Manager::new(tx);
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
    axum::serve(listener, router).await.unwrap();
}
```

当调用`start`函数时，启动流程遵循以下步骤：

1. **初始化组件**：首先，函数根据提供的配置初始化必要的组件。这包括创建和配置消息传递通道、任务调度器、客户端管理器等。
2. **创建WebSocket和RPC服务**：接下来，根据配置创建WebSocket处理器和可能的RPC服务实例。每个WebSocket连接都会在其生命周期内维护与客户端的会话。
3. **配置Axum路由**：利用`axum`库来定义HTTP路由和处理WebSocket连接的逻辑。`axum`框架提供了简洁的API来实现这些功能。例如，使用`.route("/ws/:user_id/conn/:token/:pointer_id", get(...))`来处理特定的WebSocket连接请求。
4. **使用通道处理通讯**：创建一个`mpsc`（多生产者，单消费者）通道，用于处理从WebSocket连接到服务器的核心业务逻辑的消息传递。
5. **启动服务**：使用`tokio::net::TcpListener`来监听配置的地址和端口。在监听到连接请求时，将通过`axum::serve`函数将请求路由到相应的处理器。
6. **注册服务和启动RPC服务**：如果服务涉及到与其他微服务的通信，RPC服务客户端将在此步骤创建并注册。RPC服务通常用于处理诸如身份验证、数据库交云等跨服务的业务逻辑。
7. **并行运行WebSocket和RPC服务**：使用`tokio::select!`宏来实现WebSocket服务和RPC服务的并行执行。这使得服务能够同时处理WebSocket连接和RPC请求，而不会互相阻塞。

### 运行ws模块

ws/src/main.rs

```rust
#[tokio::main]
async fn main() {
    // 设置日志等级
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();
    let config = abi::config::Config::load("./abi/config.yaml");
    ws::ws_server::start(config).await;
}
```

到这一步，我们成功地完成了 WebSocket 模块的编写。现在，让我们运行这个模块，看看我们的努力成果！

```shell
cargo run --bin ws
```

### 测试

服务端已经编写完成，为了方便我们直接使用现有的测试工具进行测试，浏览器打开两个[WebSocket在线测试工具 (wstool.js.org)](https://wstool.js.org/)窗口，然后分别在`服务地址`中输入`ws://127.0.0.1:50000/ws/123/conn/321`和`ws://127.0.0.1:50000/ws/124/conn/321`，接着在id为123的窗口内`需要发送到服务端的内容`中粘贴以下数据，也就是我们项目中定义好的消息结构体:

```json
{
    "send_id": "123",
    "receiver_id": "124",
    "local_id": "",
    "server_id": "",
    "create_time": 0,
    "send_time": 0,
    "seq": 0,
    "msg_type": 0,
    "content_type": 0,
    "content": "hello world",
    "is_read": false
}
```

**注意：** 在这个步骤，我们仅指定发送者和接收者的 ID，其他全部设置为默认值。

现在点击"发送"按钮。在这个时候，如果你转到 ID 为 124 的窗口，你应该会看到我们刚才发送的消息。如果你能看到消息，那么恭喜你，你已经成功地完成了一个基本的 WebSocket 即时通信应用！接下来，我们将把这个基本的通信应用改造成分布式应用。

**总结：**这一章节我们基于axum中的websocket实现了一个最简单的聊天应用，其中涉及到了