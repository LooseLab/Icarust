//! Implement the acquistion service. Provides three `Acquisition` methods
//!
//! 1. watch_current_acquisition_run
//!     Streams AcquisitionRunInfo back to the server
//!
//! 2. current_status
//!     Gets the current status response (Unary)
//!
//! 3. get progress
//!     Returns a current progress response (Unary)
//!

use crate::services::minknow_api::acquisition::acquisition_service_server::AcquisitionService;
use crate::services::minknow_api::acquisition::get_progress_response::RawPerChannel;
use crate::services::minknow_api::acquisition::{
    AcquisitionRunInfo, CurrentStatusRequest, CurrentStatusResponse, GetProgressRequest,
    GetProgressResponse, WatchCurrentAcquisitionRunRequest,
};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

#[derive(Debug)]
pub struct Acquisition {}

#[tonic::async_trait]
impl AcquisitionService for Acquisition {
    type watch_current_acquisition_runStream = ReceiverStream<Result<AcquisitionRunInfo, Status>>;
    async fn watch_current_acquisition_run(
        &self,
        _request: Request<WatchCurrentAcquisitionRunRequest>,
    ) -> Result<Response<Self::watch_current_acquisition_runStream>, Status> {
        let (tx, rx) = mpsc::channel(4);
        let acquisition_run_info = AcquisitionRunInfo {
            run_id: "Wowee".to_string(),
            startup_state: 0,
            startup_state_estimated_end: None,
            startup_state_estimated_percent_complete: 0.0,
            state: 0,
            finishing_state: 0,
            stop_reason: 0,
            start_time: None,
            data_read_start_time: None,
            data_read_end_time: None,
            end_time: None,
            yield_summary: None,
            config_summary: None,
            writer_summary: None,
        };
        tokio::spawn(async move {
            tx.send(Ok(acquisition_run_info.clone())).await.unwrap();
        });
        Ok(Response::new(ReceiverStream::new(rx)))
    }

    async fn current_status(
        &self,
        _request: Request<CurrentStatusRequest>,
    ) -> Result<Response<CurrentStatusResponse>, Status> {
        Ok(Response::new(CurrentStatusResponse { status: 3 }))
    }

    async fn get_progress(
        &self,
        _request: Request<GetProgressRequest>,
    ) -> Result<Response<GetProgressResponse>, Status> {
        Ok(Response::new(GetProgressResponse {
            raw_per_channel: Some(RawPerChannel {
                acquired: 100,
                processed: 900,
            }),
        }))
    }
}
