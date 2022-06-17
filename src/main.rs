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
mod impl_services;
/// The module pertaining the CLI code
pub mod cli;
#[macro_use]
extern crate log;

/// Import all our definied services
mod services;

use std::fs;

use clap::Parser;
use rand_distr::Gamma;
use serde::Deserialize;
use tonic::transport::Server;
use tokio_rustls::{
    rustls::{Certificate, PrivateKey, ServerConfig},
    TlsAcceptor,
};
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
    global_mean_read_length: Option<f64>
}

impl Config {
    /// Check if a global mean read length has been set
    pub fn has_global_length (&self) -> bool {
        let has_len = match self.global_mean_read_length{ 
            Some(_) =>  {
                true
            }
            None => {
                false
            } 
        };
        has_len
    }
    /// Get the usize version of the run duration so we can stop running if we exceed it. 
    /// If not set a default value of 4800 is returned
    pub fn get_experiment_duration_set (&self) -> usize {
        let duration = match self.parameters.experiment_duration_set{ 
            Some(duration) =>  {
                duration
            }
            None => {
                // wasn't set so default 4800
                4800
            } 
        };
        duration
    }

    pub fn check_fields (&self) {
        for sample in &self.sample {
            match sample.mean_read_length {
                Some(_) => {
                    continue
                }, 
                None => {
                    match self.global_mean_read_length {
                        Some(_) => {
                            continue
                        },
                        None => {
                            panic!("Sample {} does not have a mean read length and no global read length is set.", sample.input_genome.display());
                        }
                    }
                }
            }
        }
    }
}

#[derive(Deserialize, Debug, Clone)]
struct Parameters {
    sample_name: String,
    experiment_name: String,
    flowcell_name: String,
    experiment_duration_set: Option<usize>,
    device_id: String,
    position: String
}

#[derive(Deserialize, Debug, Clone)]
struct Sample {
    input_genome: std::path::PathBuf,
    mean_read_length: Option<f64>,
    weight: usize
}

impl Sample {
    pub fn get_read_len_dist(&self, global_read_len: Option<f64>) -> Gamma<f64> {
        match self.mean_read_length {
            Some(mrl) => {
                Gamma::new(mrl,1.0).unwrap()
            },
            None => {
                Gamma::new(global_read_len.unwrap(), 1.0).unwrap()
            }
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

    let addr_manager = "127.0.0.1:10000".parse().unwrap();
    let addr_position = "127.0.0.1:10001".parse().unwrap();
    let run_id = Uuid::new_v4().to_string().replace('-', "");

    let manager_init = Manager {
        positions: vec![FlowCellPosition {
            name: config.parameters.device_id,
            state: 1,
            rpc_ports: Some(RpcPorts {
                secure: 10001,
                insecure: 10001,
            }),
            location: Some(Location { x: 1, y: 1 }),
            error_info: "Unknown state, please help".to_string(),
            shared_hardware_group: Some(SharedHardwareGroup { group_id: 1 }),
            is_integrated: true,
            can_sequence_offline: true,
        }],
    };
    let certs = {
        let fd = std::fs::File::open("examples/data/tls/server.pem")?;
        let mut buf = std::io::BufReader::new(&fd);
        rustls_pemfile::certs(&mut buf)?
            .into_iter()
            .map(Certificate)
            .collect()
    };
    let key = {
        let fd = std::fs::File::open("examples/data/tls/server.key")?;
        let mut buf = std::io::BufReader::new(&fd);
        rustls_pemfile::pkcs8_private_keys(&mut buf)?
            .into_iter()
            .map(PrivateKey)
            .next()
            .unwrap()

        // let key = std::fs::read("examples/data/tls/server.key")?;
        // PrivateKey(key)
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
    // Create the position server for our one position.
    let log_svc = LogServiceServer::new(Log {});
    let insta_svc = InstanceServiceServer::new(Instance {});
    let anal_svc = AnalysisConfigurationServiceServer::new(Analysis {});
    let devi_svc = DeviceServiceServer::new(Device {});
    let acquisition_svc = AcquisitionServiceServer::new(Acquisition {});
    let protocol_svc = ProtocolServiceServer::new(Protocol {});
    let data_svc = DataServiceServer::new(DataServiceServicer::new(3000, run_id, args));

    Server::builder()
        .add_service(log_svc)
        .add_service(devi_svc)
        .add_service(insta_svc)
        .add_service(anal_svc)
        .add_service(acquisition_svc)
        .add_service(protocol_svc)
        .add_service(data_svc)
        .serve(addr_position)
        .await?;
    Ok(())
}
