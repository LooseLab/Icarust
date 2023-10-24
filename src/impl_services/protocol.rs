use std::path::PathBuf;

use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::services::minknow_api::protocol::protocol_service_server::ProtocolService;
use crate::services::minknow_api::protocol::{GetCurrentProtocolRunRequest, GetRunInfoRequest, ProtocolRunInfo};

pub struct ProtocolServiceServicer {
    run_id: String,
    output_path: PathBuf,
}

impl ProtocolServiceServicer {
    pub fn new(run_id: String, output_path: PathBuf) -> ProtocolServiceServicer {
        ProtocolServiceServicer {
            run_id,
            output_path,
        }
    }
}

#[tonic::async_trait]
impl ProtocolService for ProtocolServiceServicer {
    async fn get_current_protocol_run(
        &self,
        _request: Request<GetCurrentProtocolRunRequest>,
    ) -> Result<Response<ProtocolRunInfo>, Status> {
        Ok(Response::new(ProtocolRunInfo {
            run_id: self.run_id.clone(),
            protocol_id: "IAMAPROTOCOL".to_string(),
            args: vec!["settings".to_string()],
            output_path: String::from(self.output_path.clone().to_str().unwrap()),
            state: 0,
            phase: 2,
            last_phase_change: None,
            can_pause: true,
            can_trigger_mux_scan: true,
            start_time: Some(Timestamp {
                seconds: 100,
                nanos: 2,
            }),
            script_end_time: None,
            end_time: None,
            acquisition_run_ids: vec![],
            user_info: None,
            epi2me_workflows: Vec::new(),
            device: None,
            flow_cell: None,
            meta_info: None,
            associated_post_processing_analysis: Vec::new(),
            pqc_result: None,
            external_offload: None,
            software_versions: None,
            stop_origin: None,
        }))
    }

    async fn get_run_info(
        &self,
        _request: Request<GetRunInfoRequest>,
    ) -> Result<Response<ProtocolRunInfo>, Status> {
        Ok(Response::new(ProtocolRunInfo {
            run_id: self.run_id.clone(),
            protocol_id: "IAMAPROTOCOL".to_string(),
            args: vec!["settings".to_string()],
            output_path: String::from(self.output_path.clone().to_str().unwrap()),
            state: 0,
            phase: 2,
            last_phase_change: None,
            can_pause: true,
            can_trigger_mux_scan: true,
            start_time: Some(Timestamp {
                seconds: 100,
                nanos: 2,
            }),
            script_end_time: None,
            end_time: None,
            acquisition_run_ids: vec![],
            user_info: None,
            epi2me_workflows: Vec::new(),
            device: None,
            flow_cell: None,
            meta_info: None,
            associated_post_processing_analysis: Vec::new(),
            pqc_result: None,
            external_offload: None,
            software_versions: None,
            stop_origin: None,
        }))
    }
}
