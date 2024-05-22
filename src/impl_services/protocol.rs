use std::collections::HashMap;
use std::path::PathBuf;

use prost_types::Timestamp;
use tonic::{Request, Response, Status};

use crate::services::minknow_api::protocol::protocol_service_server::ProtocolService;
use crate::services::minknow_api::protocol::{
    protocol_info, protocol_info::TagValue, GetCurrentProtocolRunRequest, GetRunInfoRequest,
    GetVersionInfoResponse, ProtocolInfo, ProtocolRunInfo,
};

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
        let meta_info = Some(ProtocolInfo {
            identifier: String::from("insert_sequencing_kit_toml_name_here"),
            name: String::from("sequencing_toml_file_name"),
            tag_extraction_result: None,
            tags: HashMap::from([(
                String::from("barcoding"),
                TagValue {
                    tag_value: Some(protocol_info::tag_value::TagValue::BoolValue(true)),
                },
            ), (
                String::from("barcoding kits"),
                TagValue {
                    tag_value: Some(protocol_info::tag_value::TagValue::ArrayValue(String::from("[\"EXP-NBD114\",\"EXP-NBD103\",\"EXP-NBD104\",\"EXP-PBC01\",\"EXP-PBC096\"]")))
                }
            ), (
                String::from("kit"),
                TagValue {
                    tag_value: Some(protocol_info::tag_value::TagValue::StringValue(String::from("generic_kit_name")))
                }
            ), (
                String::from("available basecall models"),
                TagValue {
                    tag_value: None
                }
            )
            ]),
        });

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
            meta_info,
            associated_post_processing_analysis: Vec::new(),
            pqc_result: None,
            external_offload: None,
            software_versions: Some(GetVersionInfoResponse {
                minknow: None,
                distribution_status: 0,
                bream: "-1".to_string(),
                distribution_version: "unknown".to_string(),
                protocol_configuration: "0.0.0".to_string(),
                installation_type: 0,
                guppy_build_version: "7.3.9".to_string(),
                guppy_connected_version: "7.3.9".to_string(),
            }),
            stop_origin: None,
        }))
    }
}
