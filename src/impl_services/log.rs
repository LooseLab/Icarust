//! Implement the log service, to print any messages that we get sent.
//! I have no idea if it works

use crate::services::minknow_api::log::log_service_server::LogService;
use crate::services::minknow_api::log::{SendUserMessageRequest, SendUserMessageResponse};

use tonic::{Request, Response, Status};

#[derive(Debug)]
pub struct Log();

#[tonic::async_trait]
impl LogService for Log {
    async fn send_user_message(
        &self,
        _request: Request<SendUserMessageRequest>,
    ) -> Result<Response<SendUserMessageResponse>, Status> {
        info!("{:#?}", _request.into_inner().user_message);
        Ok(Response::new(SendUserMessageResponse {}))
    }
}
