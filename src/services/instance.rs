//! The instance service, required for minknow_api clients to connect.
//! 
//! Implements 1 method
//! 
//! 1. get_version_info

use instance::instance_service_server::InstanceService;
use instance::get_version_info_response::MinknowVersion;
use tonic::{Request, Response, Status};

pub mod instance {
    tonic::include_proto!("minknow_api.instance");
}

#[derive(Debug)]
pub struct Instance;

#[tonic::async_trait]
impl InstanceService for Instance {
    /// Get the version info of this version of minknow, as listed on the position.
    async fn get_version_info(
        &self,
        _request: Request<instance::GetVersionInfoRequest>
     ) -> Result <Response<instance::GetVersionInfoResponse>, Status> {
        Ok(Response::new(instance::GetVersionInfoResponse {
            minknow: Some(MinknowVersion {
                major:4, minor:0, patch:0, full:"4.0.0".to_string()
            }),
            protocols: "0.0.0.0".to_string(),
            distribution_version: "unknown".to_string(),
            distribution_status: 0,
            configuration: "0.0.0.0".to_string(),
            installation_type: 0
        }))
    }
}


