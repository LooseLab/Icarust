//! Implmentation of the analysis configuration service
//! 
//! Implements the get read classifcation method
//! 
pub mod analysis {
    tonic::include_proto!("minknow_api.analysis_configuration");
}
use analysis::analysis_configuration_service_server::AnalysisConfigurationService;
use tonic::{Request, Response, Status};
use std::collections::HashMap;

#[derive(Debug)]
pub struct Analysis();


#[tonic::async_trait]
impl AnalysisConfigurationService for Analysis {
    /// Get read classifications, as an int to string map.
    async fn get_read_classifications (
        &self,
        _request: Request<analysis::GetReadClassificationsRequest>
    ) -> Result <Response<analysis::GetReadClassificationsResponse>, Status> {
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
            (78, "unclassed".to_string())
        ]);
        Ok(Response::new(
            analysis::GetReadClassificationsResponse{
                read_classifications: class_map
            }
            )
        )
    }
}