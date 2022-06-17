//! Implementation of the CLI for icarust. Simple - just lets a user provide verbosity and a path to a config toml.

use clap::Parser;
use std::io::Write;

#[derive(Parser, Debug, Clone)]
#[clap(author, version, about, long_about = None)]
/// Struct to capture the command line interface options
pub struct Cli {
    /// Path to the config file to use
    #[clap(short, long, value_parser)]
    pub config: std::path::PathBuf,
    /// Determines verbosity of the processing, can be specified multiple times -vvv
    #[structopt(short, long, parse(from_occurrences))]
    pub verbosity: u8,
}

impl Cli {
    /// Sets logging level suplied by verbosity count.
    ///
    /// Levels possible are:
    ///
    /// - `Warn` (default)
    /// - `Info`
    /// - `Debug`
    /// - `Trace`
    pub fn set_logging(&self) {
        use log::LevelFilter::*;

        let log_level = match self.verbosity {
            level if level == 1 => Info,
            level if level == 2 => Debug,
            level if level > 2 => Trace,
            _ => Warn,
        };

        pretty_env_logger::formatted_builder()
            .format_module_path(true)
            .format(|buf, record| {
                let ts = buf.timestamp();
                writeln!(buf, "{}: {}: {}", ts, record.level(), record.args())
            })
            .filter_level(log_level)
            .init();
    }
    /// Check the config file is where specified
    pub fn check_config_exists (&self) {
        assert!(self.config.exists())
    }
}