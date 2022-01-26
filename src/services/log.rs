//! Implement the log service, to print any messages that we get sent. 
//! I have no diea if it works

pub mod log {
    tonic::include_proto!("minknow_api.log");
}
use log::log_service_server::LogService;

use tonic::{Request, Response, Status};
pub struct Log ();

#[tonic::async_trait]
impl LogService for Log {
    async fn send_user_message (
        &self,
        _request: Request<log::SendUserMessageRequest>
    ) -> Result <Response<log::SendUserMessageResponse>, Status> {
        println!("{:#?}", _request.into_inner().user_message);
        Ok(Response::new(log::SendUserMessageResponse {
            
        }))
    }
}
