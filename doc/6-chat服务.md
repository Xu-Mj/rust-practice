## 创建chat服务

### 创建项目并导入依赖

```shell
cargo new chat 

cargo add abi -p chat 
cargo add utils -p chat 
# 异步trait
cargo add async-trait -p chat
# 时间
cargo add chrono -p chat --features=serde
# 序列化
cargo add serde -p chat --features=derive
# 序列化
cargo add serde-json -p chat
# gRPC
cargo add tonic -p chat --features=gzip
# 异步运行时
cargo add tokio -p chat --features=full
# 日志
cargo add tracing  -p chat  
cargo add tracing-subscriber  -p chat --features=env-filter
# kafka，如果你是Linux平台那么--features=cmake-build
cargo add rdkafka  -p chat --features=dynamic-linking 
# 唯一id
cargo add nanoid -p chat  
```

**rdkafka依赖librdkafka，需要安装librdkafka。**安装方式：[Development>install librdkafka](https://github.com/Xu-Mj/sandcat-backend?tab=readme-ov-file#development)

### 定义gRPC接口

定义request、response结构体以及chat服务中的rpc接口

abi/protos/messages.proto中新增如下代码

```protobuf
message SendMsgRequest {
  Msg message = 1;
}

message MsgResponse {
  string local_id = 1;
  string server_id = 2;
  int64  send_time = 3;
  string err = 4;
}

/// chat服务rpc接口
service ChatService {
  rpc SendMsg(SendMsgRequest) returns (MsgResponse);
}
```

保存之后如果cargo没有重新编译，那么手动执行`cargo check`命令，cargo会运行build.rs文件重新将proto数据转换为rust代码。

### chat实现

1. **定义chat结构体**，包含两个字段一个是kafka的发布者FutureProducer一个是主题名称topic。

   chat/src/main.rs

```rust
use std::time::Duration;

use async_trait::async_trait;
use nanoid::nanoid;
use rdkafka::admin::{AdminClient, AdminOptions, NewTopic, TopicReplication};
use rdkafka::client::DefaultClientContext;
use rdkafka::error::KafkaError;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::ClientConfig;
use tonic::transport::Server;
use tracing::{error, info, Level};

use abi::config::Config;
use abi::message::chat_service_server::{ChatService, ChatServiceServer};
use abi::message::{MsgResponse, SendMsgRequest};
use utils::typos::Registration;

pub struct ChatRpcService {
    pub kafka: FutureProducer,
    pub topic: String,
}
```

2. **实现相关方法**

   chat/src/main.rs

```rust
impl ChatRpcService {
    pub fn new(kafka: FutureProducer, topic: String) -> Self {
        Self { kafka, topic }
    }
    pub async fn start(config: &Config) {
        let broker = config.kafka.hosts.join(",");
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &broker)
            .set(
                "message.timeout.ms",
                config.kafka.producer.timeout.to_string(),
            )
            .set("acks", config.kafka.producer.acks.clone())
            // make sure the message is sent exactly once
            .set("enable.idempotence", "true")
            .set("retries", config.kafka.producer.max_retry.to_string())
            .set(
                "retry.backoff.ms",
                config.kafka.producer.retry_interval.to_string(),
            )
            .create()
            .expect("Producer creation error");

        Self::ensure_topic_exists(&config.kafka.topic, &broker)
            .await
            .expect("Topic creation error");

        // register service
        Self::register_service(config).await;
        info!("<chat> rpc service register to service register center");

        let chat_rpc = Self::new(producer, config.kafka.topic.clone());
        let service = ChatServiceServer::new(chat_rpc);
        info!(
            "<chat> rpc service started at {}",
            config.rpc.chat.rpc_server_url()
        );

        Server::builder()
            .add_service(service)
            .serve(config.rpc.chat.rpc_server_url().parse().unwrap())
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
                &config.rpc.chat.name
            ),
            name: config.rpc.chat.name.clone(),
            address: config.rpc.chat.host.clone(),
            port: config.rpc.chat.port,
            tags: config.rpc.chat.tags.clone(),
        };
        center.register(registration).await.unwrap();
    }

    async fn ensure_topic_exists(topic_name: &str, brokers: &str) -> Result<(), KafkaError> {
        // Create Kafka AdminClient
        let admin_client: AdminClient<DefaultClientContext> = ClientConfig::new()
            .set("bootstrap.servers", brokers)
            .create()?;

        // create topic
        let new_topics = [NewTopic {
            name: topic_name,
            num_partitions: 1,
            replication: TopicReplication::Fixed(1),
            config: vec![],
        }];
        let options = AdminOptions::new();
        admin_client.create_topics(&new_topics, &options).await?;
        match admin_client.create_topics(&new_topics, &options).await {
            Ok(_) => {
                info!("Topic not exist; create '{}' ", topic_name);
                Ok(())
            }
            Err(KafkaError::AdminOpCreation(_)) => {
                println!("Topic '{}' already exists.", topic_name);
                Ok(())
            }
            Err(err) => Err(err),
        }
    }
}
```

**说明：**在ChatRpcService的实现中，我们定义了三个关键的方法：`new()`，`start()`，以及两个辅助的异步方法`register_service()`和`ensure_topic_exists()`。

**new 方法**

```rust
pub fn new(kafka: FutureProducer, topic: String) -> Self {
    Self { kafka, topic }
}
```

- **作用**: `new` 方法是一个构造函数，用于创建一个新的 `ChatRpcService` 实例。
- **参数:**
  - `kafka`: `FutureProducer` 类型，代表Kafka的异步生产者客户端。
  - `topic`: `String` 类型，代表将要用于发送消息的Kafka主题名。
- **返回值**: 返回一个 `ChatRpcService` 实例。

**start 方法**

```rust
pub async fn start(config: &Config) {
    ...
}
```

- **功能**: `start` 方法用于启动聊天服务。它负责创建Kafka生产者(`FutureProducer`), 确保Kafka主题存在，注册服务到服务注册中心，以及启动gRPC服务。
- **执行流程:**
  1. **创建Kafka生产者**: 根据配置(`Config`)建立连接到Kafka的生产者，用于将聊天消息发送到Kafka主题中。
  2. **确保Kafka主题存在**: 在发送消息前，通过调用 `ensure_topic_exists` 方法确保相应的Kafka主题已创建。
  3. **注册服务**: 调用 `register_service` 将聊天服务注册到服务注册中心。
  4. **启动gRPC服务**: 最后通过`tonic`框架的gRPC服务器将`chat`服务暴露出去，等待客户端连接并提供服务。

**register_service 方法**

```rust
async fn register_service(config: &Config) {
    ...
}
```

- **目的**: 此方法用于将`chat`服务注册到服务注册中心，使得客户端或其他服务能够发现并调用聊天服务。
- **主要步骤**: 构造服务注册信息`Registration`，并调用服务注册中心的`register`方法进行注册。

**ensure_topic_exists 方法**

```rust
async fn ensure_topic_exists(topic_name: &str, brokers: &str) -> Result<(), KafkaError> {
    ...
}
```

- **作用**: 确保Kafka主题已经存在，如果不存在，则创建它。
- **执行流程:**
  1. **创建Kafka管理员客户端**: 使用Kafka的管理员API创建一个新的`AdminClient`实例。
  2. **检查并创建主题**: 通过`create_topics`方法尝试创建主题。如果主题已存在，则忽略创建错误(`KafkaError::AdminOpCreation`)。

   3. **实现gRPC接口**

      chat/src/main.rs

```rust
#[async_trait]
impl ChatService for ChatRpcService {
    /// send message to mq
    /// generate msg id and send time
    async fn send_msg(
        &self,
        request: tonic::Request<SendMsgRequest>,
    ) -> Result<tonic::Response<MsgResponse>, tonic::Status> {
        let inner = request.into_inner().message;
        if inner.is_none() {
            return Err(tonic::Status::invalid_argument("message is empty"));
        }

        let mut msg = inner.unwrap();

        debug!("send msg: {:?}", msg);
        
        // generate msg id
        msg.server_id = nanoid!();
        msg.send_time = chrono::Local::now()
            .naive_local()
            .and_utc()
            .timestamp_millis();

        // send msg to kafka
        let payload = serde_json::to_string(&msg).unwrap();
        // let kafka generate key, then we need set FutureRecord<String, type>
        let record: FutureRecord<String, String> = FutureRecord::to(&self.topic).payload(&payload);
        let err = match self.kafka.send(record, Duration::from_secs(0)).await {
            Ok(_) => String::new(),
            Err((err, msg)) => {
                error!(
                    "send msg to kafka error: {:?}; owned message: {:?}",
                    err, msg
                );
                err.to_string()
            }
        };

        return Ok(tonic::Response::new(MsgResponse {
            local_id: msg.local_id,
            server_id: msg.server_id,
            send_time: msg.send_time,
            err,
        }));
    }
}
```

在这段代码中，我们实现了 `ChatService` gRPC接口的 `send_msg` 方法。这个方法的目标是接收客户端通过gRPC发送的消息，生成一个消息ID和时间戳，然后将消息发送到Kafka:

 **接口实现指定**

```rust
#[async_trait]
impl ChatService for ChatRpcService {
    ...
}
```

- 使用 `#[async_trait]` 宏来支持异步方法在trait中的实现。
- `impl ChatService for ChatRpcService` 表示为 `ChatRpcService` 结构体实现 `ChatService` trait。这是gRPC服务定义的一部分，让我们能够处理gRPC请求。这个ChatService就是我们在protobuf中定义的那个gRPC接口。

**校验非空消息**

```rust
let inner = request.into_inner().message;
if inner.is_none() {
    return Err(tonic::Status::invalid_argument("message is empty"));
}
```

- 通过调用 `request.into_inner().message` 获取实际的消息内容。
- 如果消息为空（`None`），则返回一个错误状态，提示 "message is empty"。

**生成消息ID和时间戳**

```rust
let mut msg = inner.unwrap();
msg.server_id = nanoid!();
msg.send_time = chrono::Local::now()
    .naive_local()
    .and_utc()
    .timestamp_millis();
```

- 解包消息内容。
- 使用 `nanoid!()` 宏生成一个唯一的 `server_id` 作为消息ID。
- 使用 `chrono` 库获取当前的时间戳（以毫秒为单位），作为 `send_time`。

**发送消息到Kafka**

```rust
let payload = serde_json::to_string(&msg).unwrap();
let record: FutureRecord<String, String> = FutureRecord::to(&self.topic).payload(&payload);
let err = match self.kafka.send(record, Duration::from_secs(0)).await {
    Ok(_) => String::new(),
    Err((err, msg)) => {
        error!(
            "send msg to kafka error: {:?}; owned message: {:?}",
            err, msg
        );
        err.to_string()
    }
};
```

- 将消息体序列化为JSON字符串作为 `payload`。
- 使用 `FutureRecord` 创建一个将要发送的Kafka消息，指定目标主题和Payload。
- 调用 `self.kafka.send()` 将消息发送到Kafka。结果可能是成功或者失败。
- 如果发送失败，记录错误信息，并把错误描述作为返回消息的 `err` 字段。

**返回响应**

```rust
return Ok(tonic::Response::new(MsgResponse {
    local_id: msg.local_id,
    server_id: msg.server_id,
    send_time: msg.send_time,
    err,
}));
```

- 创建一个 `MsgResponse` 实例，填入消息ID、服务端ID、发送时间和错误信息（如果有的话）。
- 通过 `tonic::Response::new` 包裹响应体，并返回成功状态。

通过这个方法，我们处理了从客户端来的发消息请求，完成了消息的验证、ID生成、时间戳设置，并将消息成功发送到Kafka

### 修改main方法启动chat服务

chat/src/main.rs

```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();
    let config = Config::load("./abi/config.yaml");
    ChatRpcService::start(&config).await;
}
```

**总结：**这是我们迈向微服务使用gRPC的第一步，所以说的比较详细，后面对于gRPC接口的实现可能只讲一下核心逻辑部分。