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
