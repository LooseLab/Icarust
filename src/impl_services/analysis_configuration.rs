//! Implmentation of the analysis configuration service
//!
//! Implements the get read classifcation method
//!

use std::collections::HashMap;
use tonic::{Request, Response, Status};

use crate::services::minknow_api::analysis_configuration;
use crate::services::minknow_api::analysis_configuration::{
    analysis_configuration_service_server::AnalysisConfigurationService,
    GetAnalysisConfigurationRequest,
    GetReadClassificationsRequest,
    GetReadClassificationsResponse,
    AnalysisConfiguration,
    EventDetection,
    ReadDetectionParams
};
#[derive(Debug)]
pub struct Analysis();

#[tonic::async_trait]
impl AnalysisConfigurationService for Analysis {
    /// Get read classifications, as an int to string map.
    async fn get_read_classifications(
        &self,
        _request: Request<GetReadClassificationsRequest>,
    ) -> Result<Response<GetReadClassificationsResponse>, Status> {
        // Hard coded to minknows current values
        let class_map = HashMap::from([
            (83, "strand".to_string()),
            (67, "strand1".to_string()),
            (77, "multiple".to_string()),
            (90, "zero".to_string()),
            (65, "adapter".to_string()),
            (66, "mux_uncertain".to_string()),
            (70, "user2".to_string()),
            (68, "user1".to_string()),
            (69, "event".to_string()),
            (80, "pore".to_string()),
            (85, "unavailable".to_string()),
            (84, "transition".to_string()),
            (78, "unclassed".to_string()),
        ]);
        Ok(Response::new(
            analysis_configuration::GetReadClassificationsResponse {
                read_classifications: class_map,
            },
        ))
    }

    async fn get_analysis_configuration(
        &self,
        _request: Request<GetAnalysisConfigurationRequest>,
    ) -> Result<Response<AnalysisConfiguration>, Status> {
        Ok(Response::new(AnalysisConfiguration {
                event_detection: Some(EventDetection{
                    window_size: 0,
                }),
                read_detection: Some(ReadDetectionParams{
                    break_reads_after_seconds: Some(1.0),
                })
            }
        ))
    }

}
