mod database;

use std::sync::Arc;

use tonic::transport::Server;
use tonic::{Request, Response, Status};
use tracing::{debug, info, Level};

use abi::config::Config;
use abi::errors::Error;
use abi::message::db_service_server::{DbService, DbServiceServer};
use abi::message::{SaveMessageRequest, SaveMessageResponse};
use utils::typos::Registration;

use crate::database::MsgStoreRepo;
#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();
    let config = abi::config::Config::load("./abi/config.yaml");
    DbRpcService::start(&config).await;
}
/// DbRpcService contains the postgres trait
pub struct DbRpcService {
    db: Arc<dyn MsgStoreRepo>,
}

impl DbRpcService {
    pub async fn new(config: &Config) -> Self {
        Self {
            db: database::msg_repo(config).await,
        }
    }

    pub async fn start(config: &Config) {
        // register service
        Self::register_service(config).await.unwrap();
        info!("<db> rpc service health check started");

        let db_rpc = DbRpcService::new(config).await;
        let service = DbServiceServer::new(db_rpc);
        info!(
            "<db> rpc service started at {}",
            config.rpc.db.rpc_server_url()
        );

        Server::builder()
            .add_service(service)
            .serve(config.rpc.db.rpc_server_url().parse().unwrap())
            .await
            .unwrap();
    }

    /// 服务注册
    async fn register_service(config: &Config) -> Result<(), Error> {
        // register service to service register center
        let center = utils::service_register_center(config);

        let registration = Registration {
            id: format!("{}-{}", utils::get_host_name()?, &config.rpc.db.name),
            name: config.rpc.db.name.clone(),
            address: config.rpc.db.host.clone(),
            port: config.rpc.db.port,
            tags: config.rpc.db.tags.clone(),
        };
        center.register(registration).await?;
        Ok(())
    }
}
#[async_trait::async_trait]
impl DbService for DbRpcService {
    async fn save_message(
        &self,
        request: Request<SaveMessageRequest>,
    ) -> Result<Response<SaveMessageResponse>, Status> {
        let inner = request.into_inner();
        let message = inner
            .message
            .ok_or_else(|| Status::invalid_argument("message is empty"))?;
        debug!("save message: {:?}", message);
        // 调用自己的save_message方法将消息落盘
        self.db
            .save_message(message)
            .await
            .map_err(|err| Status::internal(format!("Save Message Failed: {:?}", err)))?;
        return Ok(Response::new(SaveMessageResponse {}));
    }
}
