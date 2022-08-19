use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::services::minknow_api::protocol::protocol_service_server::ProtocolService;
use crate::services::minknow_api::protocol::{GetCurrentProtocolRunRequest, ProtocolRunInfo};

pub struct Protocol;

#[tonic::async_trait]
impl ProtocolService for Protocol {
    async fn get_current_protocol_run(
        &self,
        _request: Request<GetCurrentProtocolRunRequest>,
    ) -> Result<Response<ProtocolRunInfo>, Status> {
        Ok(Response::new(ProtocolRunInfo {
            run_id: "Hi".to_string(),
            protocol_id: "IAMAPROTOCOL".to_string(),
            args: vec!["settings".to_string()],
            output_path: "HI".to_string(),
            state: 2,
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
        }))
    }
}
