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
