

## Consumer服务

Consumer服务作为Kafka的消费者，扮演了核心的角色。Consumer服务不断地从Kafka中取出（消费）消息，然后为接收者增加消息序列号。同时，Consumer服务也把这些消息发送给数据库服务（DB Service）与推送服务（Pusher Service）；前者实现消息的持久化存储，后者将消息推送到ws服务中实时的将消息推送给在线的用户。

### 创建项目并添加依赖

```shell
cargo new consumer

# 依赖
cargo add abi -p consumer
cargo add utils -p consumer
# 缓存，增加用户消息序列号
cargo add cache -p consumer 
# kafka依赖，如果你是Linux平台那么使用--features=cmake-build
cargo add rdkafka  -p consumer --features=dynamic-linking
# 异步运行时
cargo add tokio -p consumer --features=full
# 日志
cargo add tracing -p consumer 
cargo add tracing-subscriber  -p consumer
# 序列化
cargo add serde-json -p consumer
cargo add serde  -p consumer
```

### Consumer数据结构定义

我们需要不停的消费kafka中的数据，因此需要一个kafka的消费者；另外因为我们需要将消息发送给db服务、pusher服务并且递增用户的消息序列号，因此consumer对象就需要包含这三个对象。其中两个rpc客户端通信方式使用的我们自定义的Channel--LbWithServiceDiscovery（拥有服务发现的能力）。

consumer/src/main.rs

```rust
use std::sync::Arc;

use abi::errors::Error;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::{ClientConfig, Message};
use tracing::{debug, error, info, Level};

use abi::config::Config;
use abi::message::db_service_client::DbServiceClient;
use abi::message::push_service_client::PushServiceClient;
use abi::message::{Msg, SaveMessageRequest, SendMsgRequest};
use cache::Cache;
use utils::LbWithServiceDiscovery;

pub struct ConsumerService {
    consumer: StreamConsumer,
    /// rpc client
    db_rpc: DbServiceClient<LbWithServiceDiscovery>,
    pusher: PushServiceClient<LbWithServiceDiscovery>,
    cache: Arc<dyn Cache>,
}
```



### 实现相关方法

```rust
impl ConsumerService {
    pub async fn new(config: &Config) -> Self {
        // init kafka consumer
        let consumer: StreamConsumer = ClientConfig::new()
            .set("group.id", &config.kafka.group)
            .set("bootstrap.servers", config.kafka.hosts.join(","))
            .set("enable.auto.commit", "false")
            .set(
                "session.timeout.ms",
                config.kafka.consumer.session_timeout.to_string(),
            )
            .set("enable.partition.eof", "false")
            .set(
                "auto.offset.reset",
                config.kafka.consumer.auto_offset_reset.clone(),
            )
            .create()
            .expect("Consumer creation failed");

        // subscribe to topic
        consumer
            .subscribe(&[&config.kafka.topic])
            .expect("Can't subscribe to specified topic");

        // init rpc client
        let db_rpc = Self::get_db_rpc_client(config).await.unwrap();

        let pusher = Self::get_pusher_rpc_client(config).await.unwrap();

        let cache = cache::cache(config);

        Self {
            consumer,
            db_rpc,
            pusher,
            cache,
        }
    }

    async fn get_db_rpc_client(
        config: &Config,
    ) -> Result<DbServiceClient<LbWithServiceDiscovery>, Error> {
        // use service register center to get ws rpc url
        let channel =
            utils::get_channel_with_config(config, &config.rpc.db.name, &config.rpc.db.protocol)
                .await?;
        let db_rpc = DbServiceClient::new(channel);
        Ok(db_rpc)
    }

    async fn get_pusher_rpc_client(
        config: &Config,
    ) -> Result<PushServiceClient<LbWithServiceDiscovery>, Error> {
        let channel = utils::get_channel_with_config(
            config,
            &config.rpc.pusher.name,
            &config.rpc.pusher.protocol,
        )
        .await?;
        let push_rpc = PushServiceClient::new(channel);
        Ok(push_rpc)
    }
}
```

`new`方法的功能是基于配置文件中指定的Kafka消费者配置，初始化一个Kafka消费者实例。此外，这一步骤还涉及到创建两个gRPC客户端以及一个缓存对象。特别值得一提的是，`get_db_rpc_client`和`get_pusher_rpc_client`方法通过我们之前定义的具备服务发现功能的`LbWithServiceDiscovery` Channel，来实现gRPC客户端的创建。不同于在Pusher服务中需主动处理服务发现传来的数据，这里的数据直接被发送至`tonic`，由`tonic`框架自行负责更新服务列表。

由于`tonic`提供了基础的负载均衡能力，因此无论我们的数据库服务（db）还是推送服务（pusher）实例有多少个，`tonic`都能自动地选择其中一个服务实例来发送请求。这样一来，即使服务的实例数量增加，`tonic`也能确保请求分发的平衡性，从而提升系统的扩展性和可靠性。这个机制对于我们的服务来说至关重要，因为它简化了负载均衡的处理流程，使得服务的扩展和维护变得更为直接和高效。

consumer/src/main.rs

```rust
impl ConsumerService {
    pub async fn consume(&mut self) -> Result<(), Error> {
        loop {
            match self.consumer.recv().await {
                Err(e) => error!("Kafka error: {}", e),
                Ok(m) => {
                    if let Some(Ok(payload)) = m.payload_view::<str>() {
                        if let Err(e) = self.handle_msg(payload).await {
                            error!("Failed to handle message: {:?}", e);
                            continue;
                        }
                        if let Err(e) = self.consumer.commit_message(&m, CommitMode::Async) {
                            error!("Failed to commit message: {:?}", e);
                        }
                    }
                }
            }
        }
    }

    /// 递增用户消息序列号
    async fn increase_seq(&self, user_id: &str) -> Result<i64, Error> {
        match self.cache.increase_seq(user_id).await {
            Ok(seq) => Ok(seq),
            Err(err) => {
                error!("failed to get seq, error: {:?}", err);
                Err(err)
            }
        }
    }

    /// 处理kafka中的消息
    async fn handle_msg(&self, payload: &str) -> Result<(), Error> {
        debug!("Received message: {:#?}", payload);

        // send to db rpc server
        let mut db_rpc = self.db_rpc.clone();

        let mut msg: Msg =
            serde_json::from_str(payload).map_err(|err| Error::InternalServer(err.to_string()))?;
        msg.seq = self.increase_seq(&msg.receiver_id).await?;

        // send to db
        let cloned_msg = msg.clone();
        let to_db = tokio::spawn(async move {
            if let Err(e) = Self::send_to_db(&mut db_rpc, cloned_msg).await {
                error!("failed to send message to db, error: {:?}", e);
            }
        });

        // send to pusher
        let mut pusher = self.pusher.clone();
        let to_pusher = tokio::spawn(async move {
            if let Err(e) = Self::send_single_to_pusher(&mut pusher, msg).await {
                error!("failed to send message to pusher, error: {:?}", e);
            }
        });

        if let Err(err) = tokio::try_join!(to_db, to_pusher) {
            error!("failed to consume message, error: {:?}", err);
            return Err(Error::InternalServer(err.to_string()));
        }
        Ok(())
    }

    async fn send_to_db(
        db_rpc: &mut DbServiceClient<LbWithServiceDiscovery>,
        msg: Msg,
    ) -> Result<(), Error> {
        let request = SaveMessageRequest { message: Some(msg) };
        db_rpc
            .save_message(request)
            .await
            .map_err(|e| Error::InternalServer(e.to_string()))?;

        Ok(())
    }

    async fn send_single_to_pusher(
        pusher: &mut PushServiceClient<LbWithServiceDiscovery>,
        msg: Msg,
    ) -> Result<(), Error> {
        pusher
            .push_single_msg(SendMsgRequest { message: Some(msg) })
            .await
            .map_err(|e| Error::InternalServer(e.to_string()))?;
        Ok(())
    }
}
```

`consume`方法负责从Kafka中获取并处理数据。这个方法会调用`StreamConsumer`的`recv`方法来获取数据，如果没有新的消息，那么这个方法就会阻塞。一旦获取到新的消息，就会启动消息处理流程。

在`handle_msg`方法中，我们首先对接收者的消息序列号进行递增，然后用两个并发的`tokio`任务，分别将消息通过RPC发送给DB服务和Pusher服务。这些任务会同时运行，如果在运行过程中出现任何错误，那么这个错误会被捕获并返回。

### 修改main方法运行consumer

consumer/src/main.rs

```rust
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    let config = Config::load("./abi/config.yaml");

    let mut consumer = ConsumerService::new(&config).await;
    info!("start consumer...");
    info!(
        "connect to kafka server: {:?}; topic: {}, group: {}",
        config.kafka.hosts, config.kafka.topic, config.kafka.group
    );
    info!("connect to rpc server: {}", config.rpc.db.rpc_server_url());
    if let Err(e) = consumer.consume().await {
        panic!("failed to consume message, error: {}", e);
    }
}

```

**总结：**利用`tonic`框架和`rdkafka`库，我们构建了一个稳健的Consumer服务。同时通过`tokio`异步运行时库，我们实现了非阻塞的消息消费和异步的消息处理，这样即便在高并发的环境中也能保持高性能。

结合服务发现和`tonic`的负载均衡功能，我们的Consumer服务不仅能够适应动态变化的服务实例和分布式环境，还能保持高效的消息处理流程。让我们的Consumer服务同时具备了健壮性和灵活性。