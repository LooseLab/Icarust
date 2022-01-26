#![deny(missing_docs)]
#![deny(missing_doc_code_examples)]

//! # Icarust
//!
//! A binary for running a mock, read-fish compatible grpc server to test live unblocking read-until experiments.
//!  
/// Import all our definied services
mod services;

use crate::services::device::device::device_service_server::DeviceServiceServer;
use crate::services::device::Device;

use crate::services::instance::instance::instance_service_server::InstanceServiceServer;
use crate::services::instance::Instance;

use crate::services::analysis::analysis::analysis_configuration_service_server::AnalysisConfigurationServiceServer;
use crate::services::analysis::Analysis;

use crate::services::log::Log;
use crate::services::log::log::log_service_server::LogServiceServer;

use crate::services::manager::Manager;
use crate::services::manager::manager::manager_service_server::ManagerServiceServer;

use crate::services::manager::manager::FlowCellPosition;
use crate::services::manager::manager::flow_cell_position::{RpcPorts, SharedHardwareGroup, Location};

use tonic::transport::Server;


/// Main function - Runs two asynchronous GRPC servers
/// The first server is the manager server, which here manages available sequencing positions and minknow version information.
/// Once a client connects to the manager it may then connect to the second GRPC server, which manages all the services relating to the 
/// sequencing position. 
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr_manager = "127.0.0.1:10000".parse().unwrap();
    let addr_position = "127.0.0.1:10001".parse().unwrap();

    let manager_init = Manager {
        positions : [FlowCellPosition {
            name: String::from("Bantersaurus"),
            state: 1,
            rpc_ports: Some(RpcPorts{secure:8000, insecure:10001, secure_grpc_web:0, insecure_grpc_web:0}),
            location: Some(Location{x:1, y:1}),
            error_info: "Unknown state, please help".to_string(),
            shared_hardware_group: Some(SharedHardwareGroup{group_id: 1}),
            is_integrated: true,
            can_sequence_offline: true
        }]
    };
    // Create the manager server and add the ervice to it
    let svc = ManagerServiceServer::new(manager_init);
    // Spawn an Async thread and send it off somewhere
    tokio::spawn(async move {
        Server::builder()
            .add_service(svc)
            .serve(addr_manager)
            .await
            .unwrap();
    });
    println!("Hello!");
    // Create the position server for our one position.
    let log_svc = LogServiceServer::new(Log {});
    let insta_svc = InstanceServiceServer::new(Instance{});
    let anal_svc = AnalysisConfigurationServiceServer::new(Analysis{});
    let devi_svc = DeviceServiceServer::new(Device{});
    
    Server::builder().add_service(log_svc).add_service(devi_svc).add_service(insta_svc).add_service(anal_svc).serve(addr_position).await?;
    Ok(())
} 