//! The instance service, required for minknow_api clients to connect.
//!
//! Implements 1 method
//!
//! 1. get_version_info

use crate::services::minknow_api::instance;
use crate::services::minknow_api::instance::get_version_info_response::MinknowVersion;
use crate::services::minknow_api::instance::instance_service_server::InstanceService;
use tonic::{Request, Response, Status};

#[derive(Debug)]
pub struct Instance();

#[tonic::async_trait]
impl InstanceService for Instance {
    /// Get the version info of this version of minknow, as listed on the position.
    async fn get_version_info(
        &self,
        _request: Request<instance::GetVersionInfoRequest>,
    ) -> Result<Response<instance::GetVersionInfoResponse>, Status> {
        Ok(Response::new(instance::GetVersionInfoResponse {
            minknow: Some(MinknowVersion {
                major: 6,
                minor: 1,
                patch: 4,
                full: "6.1.4".to_string(),
            }),
            distribution_version: "unknown".to_string(),
            distribution_status: 0,
            installation_type: 0,
            bream: String::from("Bream me up scotty"),
            protocol_configuration: String::from("0.0.0.0"),
            guppy_build_version: String::from("Build me guppy"),
            guppy_connected_version: String::from("6hopefully"),
        }))
    }
}
