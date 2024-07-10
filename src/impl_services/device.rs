//! Implement the device service. hsa two methods
//!
//! 1. get_calibration
//!
//!     Returns the cliabration of the device, needed for basecalling
//!
//! 2. get_flow_cell_info
//!     
//!     Returns a tonne of information about the flowcell
//!
//! 3. Returns the sample rate, which is used in dorado >= 7.3.9
//!
use crate::services::minknow_api::device;
use crate::services::minknow_api::device::device_service_server::DeviceService;
use crate::services::minknow_api::device::get_flow_cell_info_response::TemperatureOffsetNullable;
use tonic::{Request, Response, Status};

#[derive(Debug)]
pub struct Device {
    channel_size: usize,
    sample_rate: u32,
    offset: f32,
    range: f32,
    digitisation: u32,
}

impl Device {
    pub fn new(
        channel_size: usize,
        sample_rate: u32,
        offset: f32,
        range: f32,
        digitisation: u32,
    ) -> Device {
        Device {
            channel_size,
            sample_rate,
            offset,
            range,
            digitisation,
        }
    }
}
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
        let offsets = vec![self.offset; n_us];
        let pa_ranges = vec![self.range; n_us];
        return Ok(Response::new(device::GetCalibrationResponse {
            digitisation: self.digitisation,
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
            channel_count: self.channel_size as u32,
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
            insertion_script_status: 0,
        }))
    }

    /// Get the sample rate for a givenexperiment
    async fn get_sample_rate(
        &self,
        _request: Request<device::GetSampleRateRequest>,
    ) -> Result<Response<device::GetSampleRateResponse>, Status> {
        Ok(Response::new(device::GetSampleRateResponse {
            sample_rate: self.sample_rate,
        }))
    }
}
