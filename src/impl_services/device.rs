//! Implement the device service. hsa two methods
//!
//! 1. get_calibration
//!
//!     Returns the cliabration of the device, needed for basecalling
//!
//! 2. get_flow_cell_info
//!     
//!     Returns a tonne of information about the flowclel
//!
use crate::services::minknow_api::device;
use crate::services::minknow_api::device::device_service_server::DeviceService;
use crate::services::minknow_api::device::get_flow_cell_info_response::TemperatureOffsetNullable;
use crate::services::setup_conf::get_channel_size;
use tonic::{Request, Response, Status};

#[derive(Debug)]
pub struct Device();
#[tonic::async_trait]
impl DeviceService for Device {
    /// Get the information about the flowcell if any present.
    async fn get_calibration(
        &self,
        _request: Request<device::GetCalibrationRequest>,
    ) -> Result<Response<device::GetCalibrationResponse>, Status> {
        let request_values = _request.into_inner();
        let channels: u32 = request_values.last_channel - request_values.first_channel + 1;
        // explicitly convert to usize from u32
        let n_us = usize::try_from(channels).unwrap();
        let mut offsets = vec![0.0];
        // resize in place
        offsets.resize(n_us, 0.0);
        let mut pa_ranges = vec![1.0];
        // resize in place
        pa_ranges.resize(n_us, 0.0);
        return Ok(Response::new(device::GetCalibrationResponse {
            digitisation: 1,
            offsets,
            pa_ranges,
            has_calibration: true,
        }));
    }

    /// Get the information about the flow cell if present
    async fn get_flow_cell_info(
        &self,
        _request: Request<device::GetFlowCellInfoRequest>,
    ) -> Result<Response<device::GetFlowCellInfoResponse>, Status> {
        Ok(Response::new(device::GetFlowCellInfoResponse {
            has_flow_cell: true,
            channel_count: get_channel_size() as u32,
            wells_per_channel: 4,
            flow_cell_id: "slow_poke".to_string(),
            asic_id_str: "help_me".to_string(),
            product_code: "please".to_string(),
            user_specified_flow_cell_id: "".to_string(),
            user_specified_product_code: "".to_string(),
            has_adapter: false,
            adapter_id: "".to_string(),
            asic_version: "".to_string(),
            temperature_offset_nullable: Some(TemperatureOffsetNullable::TemperatureOffset(0.0)),
        }))
    }
}
