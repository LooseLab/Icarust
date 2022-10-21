#![deny(missing_docs)]
#![deny(missing_doc_code_examples)]
#![recursion_limit = "1024"]
//! # Icarust
//!
//! A binary for running a mock, read-fish compatible grpc server to test live unblocking read-until experiments.
//!
//! The details for configuring the run can be found in Profile_tomls.
//!
//! Simply `cargo run --config <config.toml>` in the directory to start the server, which hosts the Manager Server on 127.0.0.1:10000
//!
//! Has one position, which is hosted on 127.0.0.1:10001
//!
/// The module pertaining the CLI code
pub mod cli;
mod impl_services;
#[macro_use]
extern crate log;

/// Import all our definied services
mod services;

use std::fs;
use std::net::SocketAddr;

use chrono::Duration;
use clap::Parser;
use rand_distr::Gamma;
use serde::Deserialize;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use uuid::Uuid;


use crate::impl_services::acquisition::Acquisition;
use crate::impl_services::analysis_configuration::Analysis;
use crate::impl_services::data::DataServiceServicer;
use crate::impl_services::device::Device;
use crate::impl_services::instance::Instance;
use crate::impl_services::log::Log;
use crate::impl_services::manager::Manager;
use crate::impl_services::protocol::Protocol;

use crate::services::minknow_api::acquisition::acquisition_service_server::AcquisitionServiceServer;
use crate::services::minknow_api::analysis_configuration::analysis_configuration_service_server::AnalysisConfigurationServiceServer;
use crate::services::minknow_api::data::data_service_server::DataServiceServer;
use crate::services::minknow_api::device::device_service_server::DeviceServiceServer;
use crate::services::minknow_api::instance::instance_service_server::InstanceServiceServer;
use crate::services::minknow_api::log::log_service_server::LogServiceServer;
use crate::services::minknow_api::manager::flow_cell_position::{
    Location, RpcPorts, SharedHardwareGroup,
};
use crate::services::minknow_api::manager::manager_service_server::ManagerServiceServer;
use crate::services::minknow_api::manager::FlowCellPosition;
use crate::services::minknow_api::protocol::protocol_service_server::ProtocolServiceServer;

#[derive(Deserialize, Debug, Clone)]
struct Config {
    parameters: Parameters,
    sample: Vec<Sample>,
    output_path: std::path::PathBuf,
    global_mean_read_length: Option<f64>,
    random_seed: Option<u64>,
}

impl Config {
    /// Check if a global mean read length has been set
    pub fn has_global_length(&self) -> bool {
        let has_len = match self.global_mean_read_length {
            Some(_) => true,
            None => false,
        };
        has_len
    }
    /// Get the usize version of the run duration so we can stop running if we exceed it.
    /// If not set a default value of 4800 is returned
    pub fn get_experiment_duration_set(&self) -> usize {
        let duration = match self.parameters.experiment_duration_set {
            Some(duration) => duration,
            None => {
                // wasn't set so default 4800
                4800
            }
        };
        duration
    }
    // Get the User set random seed. If not found provide one as a random usize
    pub fn get_rand_seed(&self) -> u64 {
        match self.random_seed {
            Some(seed) => seed,
            None => rand::random::<u64>(),
        }
    }

    // Check config fields and error out if there's a problem
    pub fn check_fields(&self) {
        for sample in &self.sample {
            match sample.mean_read_length {
                Some(_) => {}
                None => match self.global_mean_read_length {
                    Some(_) => {}
                    None => {
                        panic!("Sample {} does not have a mean read length and no global read length is set.", sample.input_genome.display());
                    }
                },
            }
            if sample.is_amplicon() {
                if sample.is_barcoded() {
                    if let Some(barcodes) = &sample.barcodes {
                        if let Some(read_files) = &sample.weights_files {
                            if barcodes.len() != read_files.len() {
                                panic!("If providing amplicon weights, it is necessary to provide as many as there are barcodes.")
                            }
                        }
                    }
                }
            }
        }
    }
    /// Check if any of the samples have a barcoded field
    pub fn is_barcoded(&self) -> bool {
        let mut bools = Vec::with_capacity(self.sample.len());
        for sample in &self.sample {
            match sample.barcodes {
                Some(_) => bools.push(true),
                None => bools.push(false),
            }
        }
        bools.iter().any(|&i| i)
    }
}

#[derive(Deserialize, Debug, Clone)]
struct Parameters {
    sample_name: String,
    experiment_name: String,
    flowcell_name: String,
    experiment_duration_set: Option<usize>,
    device_id: String,
    position: String,
}

#[derive(Deserialize, Debug, Clone)]
struct Sample {
    name: String,
    input_genome: std::path::PathBuf,
    mean_read_length: Option<f64>,
    weight: usize,
    weights_files: Option<Vec<std::path::PathBuf>>,
    amplicon: Option<bool>,
    barcodes: Option<Vec<String>>,
    barcode_weights: Option<Vec<usize>>,
    uneven: Option<bool>,
}

impl Sample {
    pub fn get_read_len_dist(&self, global_read_len: Option<f64>) -> Gamma<f64> {
        match self.mean_read_length {
            Some(mrl) => Gamma::new(mrl / 450.0 * 4000.0, 1.0).unwrap(),
            None => Gamma::new(global_read_len.unwrap() / 450.0 * 4000.0, 1.0).unwrap(),
        }
    }
    pub fn is_amplicon(&self) -> bool {
        match self.amplicon {
            Some(the_truth) => the_truth,
            None => false,
        }
    }
    pub fn is_barcoded(&self) -> bool {
        match self.barcodes {
            Some(_) => true,
            None => false,
        }
    }
}

/// Loads our config TOML to get the sample name, experiment name and flowcell name, which is returned as a Config struct.
fn _load_toml(file_path: &std::path::PathBuf) -> Config {
    let contents =
        fs::read_to_string(file_path).expect("Something went wrong with reading the config file -");
    let config: Config = toml::from_str(&contents).unwrap();
    config
}

/// Main function - Runs two asynchronous GRPC servers
/// The first server is the manager server, which here manages available sequencing positions and minknow version information.
/// Once a client connects to the manager it may then connect to the second GRPC server, which manages all the services relating to the
/// sequencing position.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = cli::Cli::parse();
    args.set_logging();
    args.check_config_exists();
    let config = _load_toml(&args.config);
    config.check_fields();
    info!("{:#?}", config);
    info!("HElol");

    let cert =  tokio::fs::read("tls/rpc-certs/localhost.crt").await?;
    let key = tokio::fs::read("tls/rpc-certs/localhost.key").await?;
    let server_identity = Identity::from_pem(cert, key);
    info!("HElol2");

    let client_ca_cert = tokio::fs::read("tls/rpc-certs/ca.crt").await?;
    let client_ca_cert = Certificate::from_pem(client_ca_cert);
    info!("HElol");
    
    let tls = ServerTlsConfig::new()
        .identity(server_identity);
    let tls_position = tls.clone();

    let addr_manager = "[::0]:10000".parse().unwrap();
    let addr_position: SocketAddr = "127.0.0.1:10001".parse().unwrap();
    let run_id = Uuid::new_v4().to_string().replace('-', "");

    let manager_init = Manager {
        positions: vec![FlowCellPosition {
            name: config.parameters.device_id,
            state: 1,
            rpc_ports: Some(RpcPorts {
                secure: 10001,
                secure_grpc_web: 420
            }),
            protocol_state: 1,
            error_info: "Unknown state, please help".to_string(),
            shared_hardware_group: Some(SharedHardwareGroup { group_id: 1 }),
            is_integrated: true,
            can_sequence_offline: true,
            location: None
        }],
    };

    // Create the manager server and add the ervice to it
    let svc = ManagerServiceServer::new(manager_init);
    // Spawn an Async thread and send it off somewhere
    tokio::spawn(async move {
        Server::builder()
            .tls_config(tls).unwrap()
            .concurrency_limit_per_connection(256)
            .add_service(svc)
            .serve(addr_manager)
            .await
            .unwrap();
    });
    // Create the position server for our one position.
    let log_svc = LogServiceServer::new(Log {});
    let insta_svc = InstanceServiceServer::new(Instance {});
    let anal_svc = AnalysisConfigurationServiceServer::new(Analysis {});
    let devi_svc = DeviceServiceServer::new(Device {});
    let acquisition_svc = AcquisitionServiceServer::new(Acquisition {
        run_id: run_id.clone(),
    });
    let protocol_svc = ProtocolServiceServer::new(Protocol {});
    // let data_svc = DataServiceServer::new(DataServiceServicer::new(run_id, args));

    Server::builder()
        .tls_config(tls_position).unwrap()
        .concurrency_limit_per_connection(256)
    //     .add_service(log_svc)
        .add_service(devi_svc)
        .add_service(insta_svc)
    //     .add_service(anal_svc)
    //     .add_service(acquisition_svc)
    //     .add_service(protocol_svc)
    //     .add_service(data_svc)
        .serve(addr_position)
        .await?;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(1000));
    }
    Ok(())
}
