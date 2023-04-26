#![deny(missing_docs)]
#![deny(missing_doc_code_examples)]
//! This module contains all the code to create a new DataServiceServicer, which spawns a data generation thread that acts an approximation of a sequencer.
//! It has a few issues, but should serve it's purpose. Basically the bread and butter of this server implementation, should be readfish compatibile.
//!
//! The thread shares data with the get_live_reads function through a ARC<Mutex<Vec>>>
//!
//!
//!
//!
//!
//!
//!
use futures::{Stream, StreamExt};
use std::cmp::{self, min};
use std::collections::HashMap;
use std::fmt;
use std::fs::{create_dir_all, File, DirEntry};
use std::mem;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::Instant;
use std::{thread, u8};

use byteorder::{ByteOrder, LittleEndian};
use chrono::prelude::*;
use fnv::FnvHashSet;
use frust5_api::*;
use memmap2::Mmap;
use ndarray::{s, Array1, ArrayBase, ArrayView1, Dim, ViewRepr};
use ndarray_npy::{read_npy, ReadNpyError, ViewNpyExt};
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand_distr::{Distribution, SkewNormal};
use serde::Deserialize;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::cli::Cli;
use crate::reacquisition_distribution::{ReacquisitionPoisson, SampleDist};
use crate::read_length_distribution::ReadLengthDist;
use crate::services::minknow_api::data::data_service_server::DataService;
use crate::services::minknow_api::data::get_data_types_response::DataType;
use crate::services::minknow_api::data::get_live_reads_request::action;
use crate::services::minknow_api::data::get_live_reads_response::ReadData;
use crate::services::minknow_api::data::{
    get_live_reads_request, get_live_reads_response, GetDataTypesRequest, GetDataTypesResponse,
    GetLiveReadsRequest, GetLiveReadsResponse,
};
use crate::{Config, Sample, _load_toml};

/// unused
#[derive(Debug)]
struct RunSetup {
    setup: bool,
    first: u32,
    last: u32,
    dtype: i32,
}

/// Stores the view and total length of a squiggle NPY file
struct FileInfo {
    contig_len: usize,
    view: ArrayBase<ndarray::OwnedRepr<i16>, Dim<[usize; 1]>>,
}

impl FileInfo {
    pub fn new(
        length: usize,
        view: ArrayBase<ndarray::OwnedRepr<i16>, Dim<[usize; 1]>>,
    ) -> FileInfo {
        FileInfo {
            contig_len: length,
            view,
        }
    }
}
impl fmt::Debug for FileInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{{\n
        Contig_length: {}
        Has View: {}
        }}",
            self.contig_len,
            self.view.is_empty()
        )
    }
}
/// Stores information about each sample listed in the config TOML.
struct SampleInfo {
    name: String,
    barcodes: Option<Vec<String>>,
    barcode_weights: Option<WeightedIndex<usize>>,
    uneven: Option<bool>,
    read_len_dist: ReadLengthDist,
    files: Vec<FileInfo>,
    is_amplicon: bool,
    is_barcoded: bool,
    file_weights: Vec<WeightedIndex<usize>>,
}
impl fmt::Debug for SampleInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{{\n
        Name: {}
        Barcodes: {:#?}
        Barcode Weights: {:#?}
        Uneven: {:#?}
        Files: {:#?}
        Is Amplicon: {}
        File weights: {:#?}
        }}",
            self.name,
            self.barcodes,
            self.barcode_weights,
            self.uneven,
            self.files,
            self.is_amplicon,
            self.file_weights

        )
    }
}

impl SampleInfo {
    pub fn new(
        name: String,
        barcodes: Option<Vec<String>>,
        uneven: Option<bool>,
        is_amplicon: bool,
        is_barcoded: bool,
        read_len_dist: ReadLengthDist,
    ) -> SampleInfo {
        SampleInfo {
            name,
            barcodes,
            barcode_weights: None,
            uneven,
            read_len_dist,
            files: vec![],
            is_amplicon,
            is_barcoded,
            file_weights: vec![],
        }
    }
}

impl RunSetup {
    pub fn new() -> RunSetup {
        RunSetup {
            setup: false,
            first: 0,
            last: 0,
            dtype: 0,
        }
    }
}

#[derive(Debug)]
pub struct DataServiceServicer {
    read_data: Arc<Mutex<Vec<ReadInfo>>>,
    // to be implemented
    action_responses: Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>>,
    setup: Arc<Mutex<RunSetup>>,
    break_chunks_ms: u64,
    channel_size: usize
}

#[derive(Debug, Deserialize)]
struct Weights {
    weights: Vec<usize>,
    names: Vec<String>,
}

/// #Internal to the data generation thread
#[derive(Clone)]
struct ReadInfo {
    read_id: String,
    read: Vec<i16>,
    channel: usize,
    stop_receiving: bool,
    read_number: u32,
    was_unblocked: bool,
    write_out: bool,
    start_time: u64,
    start_time_seconds: usize,
    start_time_utc: DateTime<Utc>,
    start_mux: u8,
    end_reason: u8,
    channel_number: String,
    prev_chunk_start: usize,
    duration: usize,
    time_accessed: DateTime<Utc>,
    time_unblocked: DateTime<Utc>,
    dead: bool,
    last_read_len: u64,
    pause: f64,
    // Which sample is this read from - so we can get the chance it kills the pore
    read_sample_name: String,
}

impl fmt::Debug for ReadInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{{\n
        Read Id: {}
        Stop receiving: {}
        Data len: {}
        Read number: {}
        Was Unblocked: {}
        Duration: {}
        Time Started: {}
        Time Accessed: {}
        Prev Chunk End: {}
        Dead: {}
        Read from: {}
        Pause: {}
        WriteOut: {}
        }}",
            self.read_id,
            self.stop_receiving,
            self.read.len(),
            self.read_number,
            self.was_unblocked,
            self.duration,
            self.start_time_utc,
            self.time_accessed,
            self.prev_chunk_start,
            self.dead,
            self.read_sample_name,
            self.pause,
            self.write_out
        )
    }
}

/// Convert our vec of i16 signal to a vec of bytes to be transferred to the read until API
fn convert_to_u8(raw_data: Vec<i16>) -> Vec<u8> {
    let mut dst: Vec<u8> = vec![0; raw_data.len() * 2];
    LittleEndian::write_i16_into(&raw_data, &mut dst);
    dst
}

/// Create the output dir to write fast5 too, if it doesn't already exist
fn create_ouput_dir(output_dir: &std::path::PathBuf) -> std::io::Result<()> {
    create_dir_all(output_dir)?;
    Ok(())
}

/// Create a HashMap of barcode name to a tuple of the I16 squiggle of the 1st and 2nd form of the barcode.
fn create_barcode_squig_hashmap(config: &Config) -> HashMap<String, (Vec<i16>, Vec<i16>)> {
    let mut barcodes: HashMap<String, (Vec<i16>, Vec<i16>)> = HashMap::new();
    for sample in config.sample.iter() {
        if let Some(barcode_vec) = &sample.barcodes {
            for barcode in barcode_vec.iter() {
                let (barcode_squig_1, barcode_squig_2) = get_barcode_squiggle(barcode).unwrap();
                barcodes.insert(barcode.clone(), (barcode_squig_1, barcode_squig_2));
            }
        }
    }
    barcodes
}

/// Read in the 1st and second squiggle for a given barcode.
fn get_barcode_squiggle(barcode: &String) -> Result<(Vec<i16>, Vec<i16>), ReadNpyError> {
    info!("Fetching barcode squiggle for barcode {}", barcode);
    let barcode_arr_1: Array1<i16> = read_npy(format!(
        "python/barcoding/squiggle/{}_1.squiggle.npy",
        barcode
    ))?;
    let barcode_arr_1: Vec<i16> = barcode_arr_1.to_vec();
    let barcode_arr_2: Array1<i16> = read_npy(format!(
        "python/barcoding/squiggle/{}_2.squiggle.npy",
        barcode
    ))?;
    let barcode_arr_2: Vec<i16> = barcode_arr_2.to_vec();
    Ok((barcode_arr_1, barcode_arr_2))
}

/// Start the thread that will handle writing out the FAST5 file,
fn start_write_out_thread(
    run_id: String,
    config: Cli,
    output_path: PathBuf,
) -> SyncSender<ReadInfo> {
    let (complete_read_tx, complete_read_rx) = sync_channel(8000);
    let x = config.clone();
    thread::spawn(move || {
        let mut read_infos: Vec<ReadInfo> = Vec::with_capacity(8000);
        let exp_start_time = Utc::now();
        let iso_time = exp_start_time.to_rfc3339_opts(SecondsFormat::Millis, false);
        let config = _load_toml(&x.simulation_profile);
        let experiment_duration = config.get_experiment_duration_set().to_string();
        // std::env::set_var("HDF5_PLUGIN_PATH", "./vbz_plugin".resolve().as_os_str());
        let context_tags = HashMap::from([
            ("barcoding_enabled", "0"),
            ("experiment_duration_set", experiment_duration.as_str()),
            ("experiment_type", "genomic_dna"),
            ("local_basecalling", "0"),
            ("package", "bream4"),
            ("package_version", "6.3.5"),
            ("sample_frequency", "4000"),
            ("sequencing_kit", "sqk-lsk109"),
        ]);
        let tracking_id = HashMap::from([
            ("asic_id", "817405089"),
            ("asic_id_eeprom", "5661715"),
            ("asic_temp", "29.357218"),
            ("asic_version", "IA02D"),
            ("auto_update", "0"),
            (
                "auto_update_source",
                "https,//mirror.oxfordnanoportal.com/software/MinKNOW/",
            ),
            ("bream_is_standard", "0"),
            ("configuration_version", "4.4.13"),
            ("device_id", "Bantersaurus"),
            ("device_type", config.parameters.position.as_str()),
            ("distribution_status", "stable"),
            ("distribution_version", "21.10.8"),
            (
                "exp_script_name",
                "sequencing/sequencing_MIN106_DNA,FLO-MIN106,SQK-LSK109",
            ),
            ("exp_script_purpose", "sequencing_run"),
            ("exp_start_time", iso_time.as_str()),
            ("flow_cell_id", config.parameters.flowcell_name.as_str()),
            ("flow_cell_product_code", "FLO-MIN106"),
            ("guppy_version", "5.0.17+99baa5b"),
            ("heatsink_temp", "34.066406"),
            ("host_product_code", "GRD-X5B003"),
            ("host_product_serial_number", "NOTFOUND"),
            ("hostname", "master"),
            ("installation_type", "nc"),
            ("local_firmware_file", "1"),
            ("operating_system", "ubuntu 16.04"),
            (
                "protocol_group_id",
                config.parameters.experiment_name.as_str(),
            ),
            ("protocol_run_id", "SYNTHETIC_RUN"),
            ("protocol_start_time", iso_time.as_str()),
            ("protocols_version", "6.3.5"),
            ("run_id", run_id.as_str()),
            ("sample_id", config.parameters.sample_name.as_str()),
            ("usb_config", "fx3_1.2.4#fpga_1.2.1#bulk#USB300"),
            ("version", "4.4.3"),
        ]);
        let mut read_numbers_seen: FnvHashSet<String> =
            FnvHashSet::with_capacity_and_hasher(4000, Default::default());
        let mut file_counter = 0;
        let output_dir = PathBuf::from(format!("{}/fast5_pass/", output_path.display()));
        if !output_dir.exists() {
            create_ouput_dir(&output_dir).unwrap();
        }
        // loop to collect reads and write out files
        loop {
            for finished_read_info in complete_read_rx.try_iter() {
                read_infos.push(finished_read_info);
                if read_infos.len() >= 4000 {
                    let fast5_file_name = format!(
                        "{}/{}_pass_{}_{}.fast5",
                        &output_dir.display(),
                        config.parameters.flowcell_name,
                        &run_id[0..6],
                        file_counter
                    );
                    info!("Writing out file to {}", fast5_file_name);
                    // drain 4000 reads and write them into a FAST5 file
                    let mut multi = MultiFast5File::new(fast5_file_name.clone(), OpenMode::Append);
                    for to_write_info in read_infos.drain(..4000) {
                        // skip this read if we are trying to write it out twice
                        if !read_numbers_seen.insert(to_write_info.read_id.clone()) {
                            continue;
                        }
                        let mut new_end = to_write_info.read.len();
                        if to_write_info.was_unblocked {
                            let unblock_time = to_write_info.time_unblocked;
                            let prev_time = to_write_info.start_time_utc;
                            let elapsed_time = unblock_time.time() - prev_time.time();
                            let stop = convert_milliseconds_to_samples(elapsed_time.num_milliseconds());
                            new_end = min(stop, to_write_info.read.len());
                        }
                        let signal = to_write_info.read[0..new_end].to_vec();
                        if signal.is_empty(){
                            continue
                        };
                        let raw_attrs = HashMap::from([
                            ("duration", RawAttrsOpts::Duration(signal.len() as u32)),
                            (
                                "end_reason",
                                RawAttrsOpts::EndReason(to_write_info.end_reason),
                            ),
                            ("median_before", RawAttrsOpts::MedianBefore(100.0)),
                            (
                                "read_id",
                                RawAttrsOpts::ReadId(to_write_info.read_id.as_str()),
                            ),
                            (
                                "read_number",
                                RawAttrsOpts::ReadNumber(to_write_info.read_number as i32),
                            ),
                            ("start_mux", RawAttrsOpts::StartMux(to_write_info.start_mux)),
                            (
                                "start_time",
                                RawAttrsOpts::StartTime(to_write_info.start_time),
                            ),
                        ]);
                        let channel_info = ChannelInfo::new(
                            8192_f64,
                            6.0,
                            1500.0,
                            4000.0,
                            to_write_info.channel_number.clone(),
                        );
                        multi
                            .create_empty_read(
                                to_write_info.read_id.clone(),
                                run_id.clone(),
                                &tracking_id,
                                &context_tags,
                                channel_info,
                                &raw_attrs,
                                signal,
                            )
                            .unwrap();
                    }
                    file_counter += 1;
                    read_numbers_seen.clear();
                }
            }
            thread::sleep(Duration::from_millis(1));
        }
    });
    complete_read_tx
}

fn start_unblock_thread(
    channel_read_info: Arc<Mutex<Vec<ReadInfo>>>,
    run_setup: Arc<Mutex<RunSetup>>,
) -> SyncSender<GetLiveReadsRequest> {
    let (tx, rx): (
        SyncSender<GetLiveReadsRequest>,
        Receiver<GetLiveReadsRequest>,
    ) = sync_channel(6000);
    thread::spawn(move || {
        // We have like some actions to adress before we do anything
        let mut read_numbers_actioned = [0; 3000];
        let mut total_unblocks = 0;
        let mut total_sr = 0;
        for get_live_req in rx.iter() {
            let request_type = get_live_req.request.unwrap();
            // match whether we have actions or a setup
            let (_setup_proc, unblock_proc, stop_rec_proc) = match request_type {
                // set up request
                get_live_reads_request::Request::Setup(_) => setup(request_type, run_setup.clone()),
                // list of actions, pas through to take actions
                get_live_reads_request::Request::Actions(_) => {
                    take_actions(request_type, &channel_read_info, &mut read_numbers_actioned)
                }
            };
            total_unblocks += unblock_proc;
            total_sr += stop_rec_proc;
            info!(
                "Unblocked: {}, Stop receiving: {}, Total unblocks {}, total sr {}",
                unblock_proc, stop_rec_proc, total_unblocks, total_sr
            );
        }
    });

    tx
}

/// Process a get_live_reads_request StreamSetup, setting all the fields on the Threads RunSetup struct. This actually has no
/// effect on the run itself, but could be implemented to do so in the future if required.
fn setup(
    setuppy: get_live_reads_request::Request,
    setup_arc: Arc<Mutex<RunSetup>>,
) -> (usize, usize, usize) {
    let mut setup = setup_arc.lock().unwrap();
    info!("Received stream setup, setting up.");
    if let get_live_reads_request::Request::Setup(_h) = setuppy {
        setup.first = _h.first_channel;
        setup.last = _h.last_channel;
        setup.dtype = _h.raw_data_type;
        setup.setup = true;
    }
    // return we have prcessed 1 action
    (1, 0, 0)
}

/// Iterate through a given set of received actions and match the type of action to take
/// Unblock a read by emptying the ReadChunk held in channel readinfo dict
/// Stop receving a read sets the stop_receiving field on a ReadInfo struct to True, so we don't send it back.
/// Action Responses are appendable to a Vec which can be shared between threads, so can be accessed by the GRPC, which drains the Vec and sends back all responses.
/// Returns the number of actions processed.
fn take_actions(
    action_request: get_live_reads_request::Request,
    channel_read_info: &Arc<Mutex<Vec<ReadInfo>>>,
    read_numbers_actioned: &mut [u32; 3000],
) -> (usize, usize, usize) {
    // check that we have an action type and not a setup, whihc should be impossible
    debug!("Processing non setup actions");
    let (unblocks_processed, stop_rec_processed) = match action_request {
        get_live_reads_request::Request::Actions(actions) => {
            // let mut add_response = response_carrier.lock().unwrap();
            let mut unblocks_processed: usize = 0;
            let mut stop_rec_processed: usize = 0;

            // iterate a vec of Action
            let mut read_infos = channel_read_info.lock().unwrap();
            for action in actions.actions {
                let action_type = action.action.unwrap();
                let zero_index_channel = action.channel as usize - 1;
                let (_action_response, unblock_count, stopped_count) = match action_type {
                    action::Action::Unblock(unblock) => unblock_reads(
                        unblock,
                        action.action_id,
                        zero_index_channel,
                        action.read.unwrap(),
                        read_numbers_actioned,
                        read_infos.get_mut(zero_index_channel).unwrap_or_else(|| {
                            panic!("failed to unblock on channel {}", action.channel)
                        }),
                    ),
                    action::Action::StopFurtherData(stop) => stop_sending_read(
                        stop,
                        action.action_id,
                        zero_index_channel,
                        read_infos.get_mut(zero_index_channel).unwrap_or_else(|| {
                            panic!("failed to stop receiving on channel {}", action.channel)
                        }),
                    ),
                };
                // add_response.push(action_response);
                unblocks_processed += unblock_count;
                stop_rec_processed += stopped_count;
            }
            (unblocks_processed, stop_rec_processed)
        }
        _ => panic!(),
    };
    (0, unblocks_processed, stop_rec_processed)
}

/// Unblocks reads by clearing the channels (Represented by the index in a Vec) read vec.
fn unblock_reads(
    _action: get_live_reads_request::UnblockAction,
    action_id: String,
    channel_number: usize,
    read_number: action::Read,
    channel_num_to_read_num: &mut [u32; 3000],
    channel_read_info: &mut ReadInfo,
) -> (
    Option<get_live_reads_response::ActionResponse>,
    usize,
    usize,
) {
    let value = channel_read_info;
    // destructure read number from action request
    if let action::Read::Number(read_num) = read_number {
        // check if the last read_num we performed an action on isn't this one, on this channel
        if channel_num_to_read_num[channel_number] == read_num {
            // Debug!("Ignoring second unblock! on read {}", read_num);
            return (None, 0, 0);
        }
        if read_num != value.read_number {
            // Debug!("Ignoring unblock for old read");
            return (None, 0, 0);
        }
        // if we are dealing with a new read, set the new read num as the last dealt with read num ath this channel number
        channel_num_to_read_num[channel_number] = read_num;
    };
    // set the was unblocked field for writing out
    value.was_unblocked = true;
    value.write_out = true;
    // set the time unblocked so we can work out the length of the read to serve
    value.time_unblocked = Utc::now();
    // end reason of unblock
    value.end_reason = 4;
    (
        Some(get_live_reads_response::ActionResponse {
            action_id,
            response: 0,
        }),
        1,
        0,
    )
}

/// Stop sending read data, sets Stop receiving to True.
fn stop_sending_read(
    _action: get_live_reads_request::StopFurtherData,
    action_id: String,
    _channel_number: usize,
    value: &mut ReadInfo,
) -> (
    Option<get_live_reads_response::ActionResponse>,
    usize,
    usize,
) {
    // need a way of picking out channel by channel number or read ID

    value.stop_receiving = true;
    (
        Some(get_live_reads_response::ActionResponse {
            action_id,
            response: 0,
        }),
        0,
        1,
    )
}

/// Read the config file and parse the sample fields. This then returns any necessary Sample infos, setup
/// according to the structure of the run. This structure changes based on whether the sample is barcoded, amplicons based and has provided weights.
fn process_samples_from_config(
    config: &Config,
) -> (HashMap<String, SampleInfo>, WeightedIndex<usize>) {
    // a hashamp keyed from sample into the information about this sample that we need. The Sampleinfo value is created in the function generate_amplicon_sampling_distribution
    // and is then mutated by accessing the hashmap.
    let mut views: HashMap<String, SampleInfo> = HashMap::new();
    // iterate all the samples listed in the config directory and fetch their relative weights
    let sample_weights = read_sample_distribution(&config);
    // Seeded rng for generated weighted dists
    let mut rng: rand::rngs::StdRng = rand::SeedableRng::seed_from_u64(config.get_rand_seed());

    // Now iterate all the samples and setup any required fields for the type of run wie have. Possible combos:
    //      Amplicon barcoded
    //      Amplicon unbarcoded
    //      Non Amplicon barcoded
    //      Non Amplicon Unbarcoded
    for sample in &config.sample {
        // if the given sample input genome is actually a directory
        if sample.input_genome.is_dir() {
            let mut t: Vec<DirEntry> = sample.input_genome.read_dir().expect("read_dir call failed").into_iter().map(|x| x.unwrap()).collect();
            t.sort_by(|a, b| a.path().cmp(&b.path()));
            for entry in t
            {
                // only read files that are .npy squiggle
                if entry
                    .path()
                    .extension()
                    .unwrap()
                    .to_str()
                    .unwrap()
                    == "npy"
                {
                    info!("Reading view for{:#?}", entry.path());
                    read_views_of_data(
                        &mut views,
                        &entry.path().clone(),
                        config.global_mean_read_length,
                        sample,
                    );
                }
            }
            // if the sample is an amplicon based sample we want to get the relative distributions
            let sample_info = views.get_mut(&sample.name).unwrap();

            let distributions: Vec<WeightedIndex<usize>> = match &sample.weights_files {
                Some(_) => read_sample_distribution_files(sample),
                // generate amplicon distributions for each barcode
                None => {
                    let num_files = sample_info.files.len();
                    let mut file_distributions = vec![];
                    // If we have barcodes
                    if let Some(barcodes) = &sample.barcodes {
                        for _barcode in barcodes.iter() {
                            let disty = generate_file_sampling_distribution(num_files, &mut rng);
                            file_distributions.push(disty);
                        }
                    } else {
                        let disty = generate_file_sampling_distribution(num_files, &mut rng);
                        file_distributions.push(disty)
                    }
                    file_distributions
                }
            };
            sample_info.file_weights = distributions;

            // Time for barcoding shenanigans
            if sample.is_barcoded() {
                let barcode_dists = Some(generate_barcode_weights(
                    sample.barcode_weights.as_ref(),
                    &mut rng,
                    sample.barcodes.as_ref().unwrap().len(),
                ));
                sample_info.barcode_weights = barcode_dists;
            }
        // only a path to a single file has been passed
        } else {
            read_views_of_data(
                &mut views,
                &sample.input_genome.clone(),
                config.global_mean_read_length,
                sample,
            );

            // Time for barcoding shenanigans
            let sample_info = views.get_mut(&sample.name).unwrap();
            // we will still "sample" randomly from files but will only add 1 - resulting in 0 always being sampled - as the possible sample to be drawn
            // so we only ever see this file when we generate a read
            sample_info.file_weights = vec![WeightedIndex::new(&vec![1]).unwrap()];
            let mut barcode_dists = None;
            if sample.is_barcoded() {
                barcode_dists = Some(generate_barcode_weights(
                    sample.barcode_weights.as_ref(),
                    &mut rng,
                    sample.barcodes.as_ref().unwrap().len(),
                ));
            }
            sample_info.barcode_weights = barcode_dists;
        }
    }
    info!("{:#?}", views);
    (views, sample_weights)
}

/// Read in the sample info from the config.toml, which gives us the odds of a species genome being chosen in a multi species sample.
///
/// This file can be manually created to alter library balances.
fn read_sample_distribution_files(sample: &Sample) -> Vec<WeightedIndex<usize>> {
    let files = sample.weights_files.as_ref().unwrap();
    let mut weights: Vec<WeightedIndex<usize>> = Vec::with_capacity(files.len());
    for file_path in files {
        let file =
            File::open(file_path).expect("Distribution JSON file not found, please see README.");
        let w: Weights =
            serde_json::from_reader(file).expect("Error whilst reading distribution file.");
        weights.push(WeightedIndex::new(&w.weights).unwrap());
    }
    weights
}

/// Generate a weighted index using a skewed normal distribution. This weigted index will be of len num_amplicons.
fn generate_file_sampling_distribution(
    num_amplicons: usize,
    randay: &mut rand::rngs::StdRng,
) -> WeightedIndex<usize> {
    let mut distribution: Vec<usize> = vec![];
    let skew_normal: SkewNormal<f64> = SkewNormal::new(7.0, 2.0, 1.0).unwrap();
    for _ in 0..num_amplicons {
        distribution.push(skew_normal.sample(randay).ceil() as usize);
    }
    WeightedIndex::new(&distribution).unwrap()
}

/// Generate the weighted index for all the barcodes on a sample
fn generate_barcode_weights(
    barcode_weights: Option<&Vec<usize>>,
    randay: &mut rand::rngs::StdRng,
    num_barcodes: usize,
) -> WeightedIndex<usize> {
    let barcode_weigths_dist = match barcode_weights {
        Some(weights_vec) => WeightedIndex::new(weights_vec).unwrap(),
        // No weights provided so we will generate some using the random seed provided
        None => {
            let mut weights: Vec<usize> = vec![];
            for _ in 0..num_barcodes {
                weights.push(randay.gen())
            }
            WeightedIndex::new(&weights).unwrap()
        }
    };
    barcode_weigths_dist
}

/// Iterate the samples in the config toml and load the weights in
fn read_sample_distribution(config: &Config) -> WeightedIndex<usize> {
    let mut weights: Vec<usize> = Vec::with_capacity(config.sample.len());
    for sample in config.sample.iter() {
        weights.push(sample.weight);
    }
    WeightedIndex::new(&weights).unwrap()
}

/// Iterate the samples given as the input genome path listed in the config. If it is a directory - insert a view for each squiggle file in the directory. N.B This is used for amplicon based genomes.
/// Wraps reads_views_of_data
// fn read_genome_dir_or_file (
//     config: Config
// ) -> HashMap<String, SampleInfo> {

// }

/// Creates Memory mapped views of the precalculated numpy arrays of squiggle for reference genomes, generated by make_squiggle.py
///
/// Returns a Hashmap, keyed to the genome name that is accessed to pull a "read" (A slice of this "squiggle" array)
/// The value is in a Tuple - in order it contains the length of the squiggle array, the memory mapped view and a Gamma distribution to draw
fn read_views_of_data(
    views: &mut HashMap<String, SampleInfo>,
    file_info: &std::path::PathBuf,
    global_mean_read_length: Option<f64>,
    sample_info: &Sample,
) {
    info!(
        "Reading information for {:#?} for sample {:#?}",
        file_info.file_name(),
        sample_info
    );
    let file = File::open(file_info).unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };
    let view: ArrayBase<ViewRepr<&i16>, Dim<[usize; 1]>> =
        ArrayView1::<i16>::view_npy(&mmap).unwrap();
    let size = view.shape()[0];
    let read_length_dist = sample_info.get_read_len_dist(global_mean_read_length);
    let file_info = FileInfo::new(size, view.to_owned());
    let sample = views
        .entry(sample_info.name.clone())
        .or_insert(SampleInfo::new(
            sample_info.name.clone(),
            sample_info.barcodes.clone(),
            sample_info.uneven,
            sample_info.is_amplicon(),
            sample_info.is_barcoded(),
            read_length_dist,
        ));
    sample.files.push(file_info)
}

/// Convert an elapased period of time in milliseconds tinto samples
///
///
fn convert_milliseconds_to_samples(milliseconds: i64) -> usize {
    (milliseconds * 4) as usize
}

///
/// Create and return a Vec that stores the internal data generate thread state to be shared bewteen the server and the threads.
///
/// The vec is the length of the set number of channels with each element representing a "channel". These are accessed by index, with channel 1 represented by element at index 0.
/// The created Vec is populated by ReadInfo structs, which are used to track the ongoing state of a channel during a run.
///
/// This vec already exists and is shared around, so is not returned by this function.
/// Returns the number of pores that are alive.
fn setup_channel_vec(
    size: usize,
    thread_safe: &Arc<Mutex<Vec<ReadInfo>>>,
    rng: &mut StdRng,
    wpp: usize,
) -> usize {
    // Create channel Mutexed vec - here we hold a Vec of chunks to be served each iteration below
    let thread_safe_chunks = Arc::clone(thread_safe);

    let mut num = thread_safe_chunks.lock().unwrap();
    let percent_pore = wpp as f64 / 100.0;
    let mut alive = 0;
    for channel_number in 1..size + 1 {
        let read_info = ReadInfo {
            read_id: Uuid::nil().to_string(),
            // potench use with capacity?
            read: vec![],
            channel: channel_number,
            stop_receiving: false,
            read_number: 0,
            was_unblocked: false,
            write_out: false,
            start_time: 0,
            start_time_seconds: 0,
            start_time_utc: Utc::now(),
            channel_number: channel_number.to_string(),
            end_reason: 0,
            start_mux: 1,
            prev_chunk_start: 0,
            duration: 0,
            time_accessed: Utc::now(),
            time_unblocked: Utc::now(),
            dead: !(rng.gen_bool(percent_pore)),
            last_read_len: 0,
            pause: 0.0,
            read_sample_name: String::from(""),
        };
        if !read_info.dead {
            alive += 1
        }
        num.push(read_info);
    }
    alive
}

/// Generate an inital read, which is stored as a ReadInfo in the channel_read_info vec. This is mutated in place.
fn generate_read(
    samples: &Vec<String>,
    value: &mut ReadInfo,
    dist: &WeightedIndex<usize>,
    views: &HashMap<String, SampleInfo>,
    rng: &mut StdRng,
    read_number: &mut u32,
    start_time: &u64,
    barcode_squig: &HashMap<String, (Vec<i16>, Vec<i16>)>,
) {
    // set stop receieivng to false so we don't accidentally not send the read
    value.stop_receiving = false;
    // update as this read hasn't yet been unblocked
    value.was_unblocked = false;
    // signal psoitive end_reason
    value.end_reason = 1;
    // we want to write this out at the end
    value.write_out = true;
    // read start time in samples (seconds since start of experiment * 4000)
    value.start_time = (Utc::now().timestamp() as u64 - start_time) * 4000_u64;
    value.start_time_seconds = (Utc::now().timestamp() as u64 - start_time) as usize;
    value.start_time_utc = Utc::now();
    value.read_number = *read_number;
    let sample_choice: &String = &samples[dist.sample(rng)];
    value.read_sample_name = sample_choice.clone();
    let sample_info = &views[sample_choice];
    // choose a barcode if we need to - else we always use the first distirbution in the vec
    let mut file_weight_choice: usize = 0;
    let mut barcode = None;
    // choose barcode
    if sample_info.is_barcoded {
        // this is analagous to the choice of the barcode as well - the file weights are in vec with one weight per barcode
        file_weight_choice = sample_info.barcode_weights.as_ref().unwrap().sample(rng);
        barcode = Some(
            sample_info
                .barcodes
                .as_ref()
                .unwrap()
                .get(file_weight_choice)
                .unwrap(),
        )
    }
    // need to choose a squiggle file at this point
    let file_info = sample_info
        .files
        .get(
            sample_info
                .file_weights
                .get(file_weight_choice)
                .unwrap()
                .sample(rng),
        )
        .unwrap();
    // earliest possible start point in file, match is for amplicons so we don't start halfway through
    let start: usize = match sample_info.is_amplicon {
        true => 0,
        false => rng.gen_range(0..file_info.contig_len - 1000) as usize,
    };
    // Get our distribution from either the Sample specified Gamma or the global read length
    let read_distribution = &sample_info.read_len_dist;
    let read_length: usize = read_distribution.sample(rng) as usize;
    // don;t over slice our read
    let end: usize = cmp::min(start + read_length, file_info.contig_len - 1);
    let mut base_squiggle = file_info.view.slice(s![start..end]).to_vec();
    // Barcode name has been provided for this sample
    if sample_info.is_barcoded {
        let (mut barcode_1_squig, mut barcode_2_squig) =
            barcode_squig.get(barcode.unwrap()).unwrap().clone();
        base_squiggle.append(&mut barcode_2_squig);
        barcode_1_squig.append(&mut base_squiggle);
        base_squiggle = barcode_1_squig;
    }
    // slice the view to get our full read
    value.read.append(&mut base_squiggle);
    // set estimated duration in seconds
    value.duration = value.read.len() / 4000;
    // set the read len for channel death chance
    value.last_read_len = value.read.len() as u64;
    let read_id = Uuid::new_v4().to_string();
    value.read_id = read_id;
    // reset these time based metrics
    value.time_accessed = Utc::now();
    // prev chunk start has to be zero as there are now no previous chunks on anew read
    value.prev_chunk_start = 0;
}

impl DataServiceServicer {
    pub fn new(run_id: String, cli_opts: Cli, output_path: PathBuf, channel_size: usize) -> DataServiceServicer {
        let now = Instant::now();
        let config = _load_toml(&cli_opts.simulation_profile);
        let working_pore_percent = config.get_working_pore_precent();
        let break_chunks_ms: u64 = config.parameters.get_chunk_size_ms();
        let start_time: u64 = Utc::now().timestamp() as u64;
        let barcode_squig = create_barcode_squig_hashmap(&config);
        info!("Barcodes available {:#?}", barcode_squig.keys());
        let safe: Arc<Mutex<Vec<ReadInfo>>> =
            Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let action_response_safe: Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>> =
            Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let _thread_safe_responses = Arc::clone(&action_response_safe);
        let thread_safe = Arc::clone(&safe);
        let run_setup = RunSetup::new();
        let is_setup = Arc::new(Mutex::new(run_setup));
        let is_safe_setup = Arc::clone(&is_setup);

        let (views, dist) = process_samples_from_config(&config);
        let files: Vec<String> = views.keys().map(|z| z.clone()).collect();
        let complete_read_tx = start_write_out_thread(run_id, cli_opts, output_path.clone());
        let mut rng: StdRng = rand::SeedableRng::seed_from_u64(1234567);

        let starting_functional_pore_count =
            setup_channel_vec(channel_size, &thread_safe, &mut rng, working_pore_percent);
        let death_chance = config.calculate_death_chance(starting_functional_pore_count);
        let mut time_logged_at: f64 = 0.0;
        info!("Death chances {:#?}", death_chance);
        // start the thread to generate data
        thread::spawn(move || {
            let r: ReacquisitionPoisson = ReacquisitionPoisson::new(1.0, 0.0, 0.0001, 0.05);

            // read number for adding to unblock
            let mut read_number: u32 = 0;
            let mut completed_reads: u32 = 0;

            // Infinte loop for data generation
            loop {
                let read_process = Instant::now();
                debug!("Sequencer mock loop start");
                let mut new_reads = 0;
                let mut dead_pores = 0;
                let mut empty_pores = 0;
                let mut awaiting_reacquisition = 0;
                let mut occupied = 0;
                // sleep the length of the milliseconds chunk size
                // Don't sleep the thread just reacquire reads
                thread::sleep(Duration::from_millis(10));

                // get some basic stats about what is going on at each channel
                let _channels_with_reads = 0;
                let mut num = thread_safe.lock().unwrap();

                for i in 0..channel_size {
                    let time_taken = read_process.elapsed().as_secs_f64();
                    let value = num.get_mut(i).unwrap();
                    if value.dead {
                        dead_pores += 1;
                        continue;
                    }
                    if value.read.is_empty() {
                        empty_pores += 1;
                        if value.pause > 0.0 {
                            value.pause -= time_taken;
                            awaiting_reacquisition += 1;
                            continue;
                        }
                    } else {
                        occupied += 1;
                    }
                    let read_estimated_finish_time =
                        value.start_time_seconds as usize + value.duration;
                    // experiment_time is the time the experimanet has started until now
                    let experiment_time = Utc::now().timestamp() as u64 - start_time;
                    // info!("exp time: {}, read_finish_time: {}, is exp greater {}", experiment_time, read_estimated_finish_time, experiment_time as usize > read_estimated_finish_time);
                    // We should deal with this read as if it had finished
                    if experiment_time as usize > read_estimated_finish_time || value.was_unblocked
                    {
                        if value.write_out {

                            completed_reads += 1;
                            complete_read_tx.send(value.clone()).unwrap();
                            value.pause = r.sample(&mut rng);
                            // The potnetial chance to die
                            let potential_yolo_death =
                                death_chance.get(&value.read_sample_name).unwrap();
                            // all our death chances are altered by yield, so we need to change the chance of death of a read was unblocked due to the lowered yield
                            let prev_chance_multiplier = match value.was_unblocked {
                                // we unblocked the read and now we need to alter teh chance of death to be lower as the read was lower
                                true => {
                                    let unblock_time = value.time_unblocked;
                                    let read_start_time = value.start_time_utc;
                                    let elapsed_time =
                                        (unblock_time - read_start_time).num_milliseconds();
                                    // convert the elapsed time into a very rough amount of bases
                                    (elapsed_time as f64 * 0.45)
                                        / potential_yolo_death.mean_read_length
                                }
                                false => 1.0,
                            };
                            value.dead = rng.gen_bool(
                                potential_yolo_death.base_chance * prev_chance_multiplier,
                            );
                        }
                        value.read.clear();
                        // shrink the vec allocation to new empty status
                        value.read.shrink_to_fit();
                        value.was_unblocked = false;
                        // Could be a slow problem here?
                        value.write_out = false;
                        // Our pore died, so sad
                        if value.dead {
                            dead_pores += 1;
                            continue;
                        }
                        // chance to aquire a read
                        if rng.gen_bool(0.8){
                            new_reads += 1;
                            read_number += 1;   
                            generate_read(
                                &files,
                                value,
                                &dist,
                                &views,
                                &mut rng,
                                &mut read_number,
                                &start_time,
                                &barcode_squig,
                            )
                        }       
                    }
                }
                let _end = now.elapsed().as_secs_f64();
                if _end.ceil() > time_logged_at {
                    info!(
                        "New reads: {}, Occupied: {}, Empty pores: {}, Dead pores: {}, Sequenced reads: {}, Awaiting: {}",
                        new_reads, occupied, empty_pores, dead_pores, completed_reads, awaiting_reacquisition
                    );
                    time_logged_at = _end.ceil();
                }

                if dead_pores == 2950 {
                    break;
                }
            }
        });
        // return our newly initialised DataServiceServicer to add onto the GRPC server
        DataServiceServicer {
            read_data: safe,
            action_responses: action_response_safe,
            setup: is_safe_setup,
            break_chunks_ms,
            channel_size,
        }
    }
}

#[tonic::async_trait]
impl DataService for DataServiceServicer {
    type get_live_readsStream =
        Pin<Box<dyn Stream<Item = Result<GetLiveReadsResponse, Status>> + Send + 'static>>;

    async fn get_live_reads(
        &self,
        _request: Request<tonic::Streaming<GetLiveReadsRequest>>,
    ) -> Result<Response<Self::get_live_readsStream>, Status> {
        // Incoming stream
        let mut stream = _request.into_inner();
        // Get a reference to the Data Vec
        let data_lock = Arc::clone(&self.read_data);
        let data_lock_unblock = Arc::clone(&self.read_data);
        let setup = Arc::clone(&self.setup.clone());
        let tx_unblocks = {
            let tx_unblocks = start_unblock_thread(data_lock_unblock, setup);
            tx_unblocks
        };
        let channel_size = self.channel_size;
        let mut stream_counter = 1;
        let break_chunk_ms = self.break_chunks_ms.clone();
        let chunk_size = break_chunk_ms as f64 / 1000.0 * 4000.0;

        // Stream the responses back
        let output = async_stream::try_stream! {
            // Async channel that will await when it ahs one elemnt. This pushes the read response back immediately.
            let (tx_get_live_reads_response, mut rx_get_live_reads_response) = tokio::sync::mpsc::channel(1);
            // spawn an async thread that handles the incoming Get Live Reads Requests.
            //  This is spawned after we receive our first connection.
            tokio::spawn(async move {
                while let Some(live_reads_request) = stream.next().await {
                    let now2 = Instant::now();
                    let live_reads_request = live_reads_request.unwrap();
                    // send all the actions we wish to take to action thread
                    tx_unblocks.send(live_reads_request).unwrap();
                    stream_counter += 1
                }
            });
            // spawn an async thread that will get the read data from the data generation thread and return it.
            tokio::spawn(async move {
                loop{
                    let now2 = Instant::now();
                    let mut container: Vec<(usize, ReadData)> = Vec::with_capacity(channel_size);
                    // Number of chunks that we will send back
                    let size = channel_size as f64 / 24_f64;
                    let size = size.ceil() as usize;
                    let mut channel: u32 = 1;
                    let mut num_reads_stop_receiving: usize = 0;
                    let mut num_channels_empty: usize = 0;
                    // max read len in samples that we will consider sending samples for
                    let max_read_len_samples: usize = 30000;

                    // calculate number of samples to slice - roughly the time we break reads * 4000, so for the default 0.4 seconds
                    // we serve 0.4 * 4000 (1600) samples

                    // The below code block allows us to Send the responses across an await.
                    {
                        // get the channel data vec
                        // The below code block allows us to unlock a syncronous Arc Mutex across an asyncronous await.
                        let mut read_data_vec = {
                            debug!("Getting GRPC lock {:#?}", now2.elapsed().as_millis());
                            let mut z1 = data_lock.lock().unwrap();
                            debug!("Got GRPC lock {:#?}", now2.elapsed().as_millis());
                            z1
                        };
                        // Iterate over each channel
                        for i in 0..channel_size {
                            let mut read_info = read_data_vec.get_mut(i).unwrap();
                            debug!("Elapsed at start of drain {}", now2.elapsed().as_millis());
                            if !read_info.stop_receiving && !read_info.was_unblocked && read_info.read.len() > 0 {
                                // work out where to start and stop our slice of signal
                                let mut start = read_info.prev_chunk_start;
                                let now_time = Utc::now();
                                let read_start_time = read_info.start_time_utc;
                                let elapsed_time = now_time.time() - read_start_time.time();
                                // How far through the read we are in total samples
                                let mut stop = convert_milliseconds_to_samples(elapsed_time.num_milliseconds());
                                // slice of signal is too short
                                if start > stop || (stop - start) < chunk_size as usize {
                                    continue
                                }

                                // Read through pore is too long ( roughly longer than 4.5kb worth of bases through pore
                                if stop > max_read_len_samples {
                                    continue
                                }

                                // only send last chunks worth of data
                                if  (stop - start) > (chunk_size as f64 * 1.1_f64) as usize {
                                    // Work out where a break_reads size finishes 
                                    // i.e if we have gotten 1.5 chunks worth since last time, that is not actually possible on a real sequencer.
                                    // So we need to calculate where the 1 chunk finishes and set that as the prev_chunk_stop and serve it
                                    let full_width = stop - start;
                                    let chunks_in_width = full_width.div_euclid(chunk_size as usize);
                                    stop = chunk_size as usize * chunks_in_width;
                                    start = stop - chunk_size as usize;
                                    if start > read_info.read.len() {
                                        start = read_info.read.len() - 1000;
                                    }
                                }
                                // CHeck start is not past end
                                if start > read_info.read.len() {
                                    continue
                                }

                                // Only send back one chunks worth of data
                                // don't overslice the read by going off the end
                                let stop = min(stop, read_info.read.len());
                                read_info.time_accessed = now_time;
                                read_info.prev_chunk_start = stop;
                                let read_chunk = read_info.read[start..stop].to_vec();
                                // Chunk is too short 
                                if read_chunk.len() < 300 {
                                    continue
                                }
                                container.push((read_info.channel, ReadData{
                                        id: read_info.read_id.clone(),
                                        number: read_info.read_number.clone(),
                                        start_sample: 0,
                                        chunk_start_sample: 0,
                                        chunk_length:  read_chunk.len() as u64,
                                        chunk_classifications: vec![83],
                                        raw_data: convert_to_u8(read_chunk),
                                        median_before: 225.0,
                                        median: 110.0,
                                }));

                            }
                        }
                        // Drop the read data vec to free the lock on it
                        mem::drop(read_data_vec);
                    // reset channel so we don't over total number of channels whilst spinning for data
                    }
                    let mut channel_data = HashMap::with_capacity(24);

                    for chunk in container.chunks(24) {
                        for (channel,read_data) in chunk {
                            channel_data.insert(channel.clone() as u32, read_data.clone());
                        }
                        tx_get_live_reads_response.send(GetLiveReadsResponse{
                            samples_since_start: 0,
                            seconds_since_start: 0.0,
                            channels: channel_data.clone(),
                            action_responses: vec![]
                        }).await.unwrap();
                        channel_data.clear();
                    }
                    container.clear();
                    thread::sleep(Duration::from_millis(break_chunk_ms));
                }

            });
            while let Some(message) = rx_get_live_reads_response.recv().await {
                yield message
            }

        };
        info!("End of stream");
        Ok(Response::new(Box::pin(output) as Self::get_live_readsStream))
    }

    async fn get_data_types(
        &self,
        _request: Request<GetDataTypesRequest>,
    ) -> Result<Response<GetDataTypesResponse>, Status> {
        Ok(Response::new(GetDataTypesResponse {
            uncalibrated_signal: Some(DataType {
                r#type: 0,
                big_endian: false,
                size: 2,
            }),
            calibrated_signal: Some(DataType {
                r#type: 0,
                big_endian: false,
                size: 2,
            }),
            bias_voltages: Some(DataType {
                r#type: 0,
                big_endian: false,
                size: 2,
            }),
        }))
    }
}
