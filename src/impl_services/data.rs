//! This module contains all the code to create a new DataServiceServicer, which spawns a data generation thread that acts an approximation of a sequencer.
//! It has a few issues, but should serve it's purpose. Basically the bread and butter of this server implementation, should be readfish compatibile.
//!
//! The thread shares data with the get_live_reads function through a ARC<Mutex<Vec>>>
//!
//! Issues
//! ------
//!
//! - Provides data on a 0.4 second loop, so you get a lot of data every 0.4 seconds
//! - Potentially can get out of sync between Actions and Reads
//!
//!

use futures::{Stream, StreamExt};
use std::cmp::{self, min};
use std::collections::HashMap;
use std::fs::File;
use std::mem;
use std::path::Path;
use std::pin::Pin;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::Instant;
use std::{thread, u8};
use std::fmt;

use byteorder::{ByteOrder, LittleEndian};
use chrono::prelude::*;
use env_logger::Env;
use fnv::FnvHashSet;
use frust5_api::*;
use memmap2::Mmap;
use ndarray::{s, ArrayBase, ArrayView1, Dim, ViewRepr};
use ndarray_npy::ViewNpyExt;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand_distr::{Distribution, Normal};
use serde::Deserialize;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use crate::_load_toml;
use crate::services::minknow_api::data::data_service_server::DataService;
use crate::services::minknow_api::data::get_data_types_response::DataType;
use crate::services::minknow_api::data::get_live_reads_request::action;
use crate::services::minknow_api::data::get_live_reads_response::ReadData;
use crate::services::minknow_api::data::{
    get_live_reads_request, get_live_reads_response, GetDataTypesRequest, GetDataTypesResponse,
    GetLiveReadsRequest, GetLiveReadsResponse,
};
use crate::services::setup_conf::get_channel_size;

const CHUNK_SIZE_1S: usize = 4000;
const BREAK_READS_MS: u64 = 400;
const MEAN_READ_LEN: f64 = 3000.0 / 450.0 * 4000.0;
const STD_DEV: f64 = 3000.0;

/// unused
#[derive(Debug)]
struct RunSetup {
    setup: bool,
    first: u32,
    last: u32,
    dtype: i32,
}

impl RunSetup {
    pub fn new() -> RunSetup {
        return RunSetup {
            setup: false,
            first: 0,
            last: 0,
            dtype: 0,
        };
    }
}

#[derive(Debug)]
pub struct DataServiceServicer {
    read_data: Arc<Mutex<Vec<ReadInfo>>>,
    complete_read_tx: SyncSender<ReadInfo>,
    // to be implemented
    action_responses: Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>>,
    // also can now be implemented
    setup: Arc<Mutex<bool>>,
    read_count: usize,
    processed_actions: usize,
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
    stop_receiving: bool,
    read_number: u32,
    start_coord: usize,
    stop_coord: usize,
    was_unblocked: bool,
    unblock_stop_coord: usize,
    file_name: String,
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
    time_unblocked: DateTime<Utc>
}

impl fmt::Debug for ReadInfo {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    write!(f, "{{\n
        Read Id: {}
        Stop receiving: {}
        Data len: {}
        Read number: {}
        Was Unblocked: {}
        Duration: {}
        Time Started: {}
        Time Accessed: {}
        Prev Chunk End: {}
        }}", self.read_id, self.stop_receiving, self.read.len(), self.read_number, self.was_unblocked, self.duration, self.start_time_utc, self.time_accessed, self.prev_chunk_start)
  }
}  

/// Add on between 0.7 and 1.5 of a chunk onto unblocked reads
fn extend_unblocked_reads(normal: Normal<f64>, end: usize) -> usize {
    let addition = normal.sample(&mut rand::thread_rng()) * 1600_f64;
    let addition = addition.floor() as usize;
    end + addition
}

/// Convert our i16 signal to bytes to be transferred to the read until API
fn convert_to_u8(raw_data: Vec<i16>) -> Vec<u8> {
    let mut dst: Vec<u8> = vec![0; raw_data.len() * 2];
    LittleEndian::write_i16_into(&raw_data, &mut dst);
    dst
}

/// Start the thread that will handle writing out the FAST5 file,
fn start_write_out_thread(run_id: String) -> SyncSender<ReadInfo> {
    let (complete_read_tx, complete_read_rx) = sync_channel(4000);
    thread::spawn(move || {
        let mut read_infos: Vec<ReadInfo> = Vec::with_capacity(8000);
        let exp_start_time = Utc::now();
        let iso_time = exp_start_time.to_rfc3339_opts(SecondsFormat::Millis, false);
        let config = _load_toml("config.toml");
        // std::env::set_var("HDF5_PLUGIN_PATH", "./vbz_plugin".resolve().as_os_str());
        let context_tags = HashMap::from([
            ("barcoding_enabled", "0"),
            (
                "experiment_duration_set",
                config.experiment_duration_set.as_str(),
            ),
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
            ("device_type", config.position.as_str()),
            ("distribution_status", "stable"),
            ("distribution_version", "21.10.8"),
            (
                "exp_script_name",
                "sequencing/sequencing_MIN106_DNA,FLO-MIN106,SQK-LSK109",
            ),
            ("exp_script_purpose", "sequencing_run"),
            ("exp_start_time", iso_time.as_str()),
            ("flow_cell_id", config.flowcell_name.as_str()),
            ("flow_cell_product_code", "FLO-MIN106"),
            ("guppy_version", "5.0.17+99baa5b"),
            ("heatsink_temp", "34.066406"),
            ("host_product_code", "GRD-X5B003"),
            ("host_product_serial_number", "NOTFOUND"),
            ("hostname", "master"),
            ("installation_type", "nc"),
            ("local_firmware_file", "1"),
            ("operating_system", "ubuntu 16.04"),
            ("protocol_group_id", config.experiment_name.as_str()),
            ("protocol_run_id", "SYNTHETIC_RUN"),
            ("protocol_start_time", iso_time.as_str()),
            ("protocols_version", "6.3.5"),
            ("run_id", run_id.as_str()),
            ("sample_id", config.sample_name.as_str()),
            ("usb_config", "fx3_1.2.4#fpga_1.2.1#bulk#USB300"),
            ("version", "4.4.3"),
        ]);
        let mut read_numbers_seen: FnvHashSet<String> =
            FnvHashSet::with_capacity_and_hasher(4000, Default::default());
        let files = ["NC_002516.2.squiggle.npy", "NC_003997.3.squiggle.npy"];
        let views = read_views_of_data(files);
        let normal: Normal<f64> = Normal::new(1.0, 0.1).unwrap();
        let mut file_counter = 0;
        // loop to collect reads and write out files
        loop {
            for finished_read_info in complete_read_rx.try_iter() {
                read_infos.push(finished_read_info);
                if read_infos.len() >= 1 {
                    let fast5_file_name = format!(
                        "{}_pass_{}_{}.fast5",
                        config.flowcell_name,
                        &run_id[0..6],
                        file_counter
                    );
                    // drain 4000 reads and write them into a FAST5 file
                    let mut multi = MultiFast5File::new(fast5_file_name.clone(), OpenMode::Append);
                    for to_write_info in read_infos.drain(..1) {
                        // skip this read if we are trying to write it out twice
                        if !read_numbers_seen.insert(to_write_info.read_id.clone()) {
                            continue;
                        }
                        let mut new_end = to_write_info.read.len();
                        if to_write_info.was_unblocked {
                            let unblock_time = to_write_info.time_unblocked;
                            let prev_time = to_write_info.start_time_utc;
                            let elapsed_time = unblock_time.time() - prev_time.time();
                            let stop = convert_seconds_to_samples(elapsed_time.num_milliseconds());
                            info!("{:#?}", to_write_info);
                            info!("WE ARE WRITING OUT AN UNBLOCKED READ {} AND THE LENGTH IS FROM 0 to {}", to_write_info.read_id, stop);
                            new_end = min(stop, to_write_info.read.len());              
                        }
                        let signal = to_write_info.read[0..new_end].to_vec();
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
        }
    });
    complete_read_tx
}

fn start_unblock_thread(channel_read_info: Arc<Mutex<Vec<ReadInfo>>>, complete_read_tx: SyncSender<ReadInfo>) -> SyncSender<GetLiveReadsRequest> {
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
            let (setup_proc, unblock_proc, stop_rec_proc) = match request_type {
                // set up request
                get_live_reads_request::Request::Setup(_) => {
                    setup(request_type)
                }
                // list of actions
                get_live_reads_request::Request::Actions(_) => take_actions(
                    request_type,
                    &channel_read_info,
                    &mut read_numbers_actioned,
                ),
            };
            total_unblocks += unblock_proc;
            total_sr += stop_rec_proc;
            info!("Unblocked: {}, Stop receiving: {}, Total unblocks {}, total sr {}", unblock_proc, stop_rec_proc, total_unblocks, total_sr);
        }
    });

    tx
}

/// Process a get_live_reads_request StreamSetup, setting all the fields on the Threads RunSetup struct. This actually has no
/// effect on the run itself, but could be implemented to do so in the future if required.
fn setup(
    setuppy: get_live_reads_request::Request,

) -> (usize, usize, usize) {
    info!("Received stream setup, setting up.");
    match setuppy {
        get_live_reads_request::Request::Setup(h) => {

        }
        _ => {} // ignore everything else
    };
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
                let (action_response, unblock_count, stopped_count) = match action_type {
                    action::Action::Unblock(unblock) => unblock_reads(
                        unblock,
                        action.action_id,
                        zero_index_channel,
                        action.read.unwrap(),
                        read_numbers_actioned,
                        read_infos.get_mut(zero_index_channel).expect(
                            format!("failed to unblock on channel {}", action.channel).as_str()
                        ),
                    ),
                    action::Action::StopFurtherData(stop) => {
                        stop_sending_read(stop, action.action_id, zero_index_channel, read_infos.get_mut(zero_index_channel).expect(
                            format!("failed to stop receiving on channel {}", action.channel).as_str()
                        ))
                    }
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
            info!("Ignoring second unblock! on read {}", read_num);
            return (None, 0, 0);
        }
        if read_num != value.read_number {
            info!("Ignoring unblcok for old read");
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
    channel_number: usize,
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

/// Read in the species distribution JSON, which gives us the odds of a species genome being chosen in a multi species sample.
///
/// The distribution file is pre calculated by the included python script make_squiggle.py. This script uses the lengths of the specified genomes
/// to calculate the likelihood of a genome being sequenced in a library that has the same amount of cells uses in the preperation.
/// This file can be manually created to alter library balances.
fn read_species_distribution(json_file_path: &Path) -> WeightedIndex<usize> {
    let file =
        File::open(json_file_path).expect("Distribution JSON file not found, please see README.");
    let w: Weights =
        serde_json::from_reader(file).expect("Error whilst reading distribution file.");
    let weights = w.weights;
    let names = w.names;
    info!("Read in weights for species {:#?}", names);
    let dist = WeightedIndex::new(&weights).unwrap();
    dist
}

/// Creates Memory mapped views of the precalculated numpy arrays of squiggle for reference genomes, generated by make_squiggle.py
///
/// Returns a Hashmap, keyed to the genome name that is accessed to pull a "read" (A slice of this "squiggle" array)
fn read_views_of_data(
    files: [&str; 2],
) -> HashMap<&str, (usize, ArrayBase<ndarray::OwnedRepr<i16>, Dim<[usize; 1]>>)> {
    let mut views = HashMap::new();
    for file_path in files {
        let file = File::open(file_path).unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        let view: ArrayBase<ViewRepr<&i16>, Dim<[usize; 1]>> =
            ArrayView1::<i16>::view_npy(&mmap).unwrap();
        let size = view.shape()[0];
        views.insert(file_path, (size, view.to_owned()));
    }
    views
}


/// Convert an elapased period of time in milliseconds tinto samples
/// 
/// 
fn convert_seconds_to_samples(milliseconds: i64) -> usize {
    (milliseconds * 4) as usize
}


///
/// Create and return a Vec that stores the internal data generate thread state to be shared bewteen the server and the threads.
///
/// The vec is the length of the set number of channels with each element representing a "channel". These are accessed by index, with channel 1 represented by element at index 0.
/// The created Vec is populated by ReadInfo structs, which are used to track the ongoing state of a channel during a run.
///
/// This vec already exists and is shared around, so is not returned by this function.
fn setup_channel_vec(size: usize, thread_safe: &Arc<Mutex<Vec<ReadInfo>>>) {
    // Create channel Mutexed vec - here we hold a Vec of chunks to be served each iteration below
    let thread_safe_chunks = Arc::clone(&thread_safe);

    let mut num = thread_safe_chunks.lock().unwrap();
    for channel_number in 1..size + 1 {
        let read_info = ReadInfo {
            read_id: Uuid::nil().to_string(),
            // potench use with capacity?
            read: vec![],
            stop_receiving: false,
            read_number: 0,
            start_coord: 0,
            stop_coord: 0,
            was_unblocked: false,
            unblock_stop_coord: 0,
            file_name: "".to_string(),
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
            time_unblocked: Utc::now()
        };
        num.push(read_info);
    }

}

/// Generate an inital read, which is stored as a ReadInfo in the channel_read_info vec. This is mutated in place.
fn generate_read(
    files: [&str; 2],
    value: &mut ReadInfo,
    dist: &WeightedIndex<usize>,
    views: &HashMap<&str, (usize, ArrayBase<ndarray::OwnedRepr<i16>, Dim<[usize; 1]>>)>,
    normal: Normal<f64>,
    rng: &mut ThreadRng,
    read_number: &mut u32,
    start_time: &u64,
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
    value.start_time = (Utc::now().timestamp() as u64 - start_time) * 4000 as u64;
    value.start_time_seconds = (Utc::now().timestamp() as u64 - start_time) as usize;
    value.start_time_utc = Utc::now();
    value.read_number = read_number.clone();
    let file_choice: &str = files[dist.sample(rng)];
    let file_info = &views[file_choice];
    // start point in file
    let start: usize = rng.gen_range(0..file_info.0 - 1000) as usize;
    let read_length: usize = normal.sample(&mut rand::thread_rng()) as usize;
    // don;t over slice our read
    let end: usize = cmp::min(start + read_length, file_info.0 - 1);
    // slice the view to get our full read
    value
        .read
        .append(&mut file_info.1.slice(s![start..end]).to_vec());
    // set estimated duration in seconds
    value.duration = value.read.len() / 4000;
    // set the info we need to write the file out
    value.file_name = file_choice.to_string();
    value.start_coord = start;
    value.stop_coord = end;
    let read_id = Uuid::new_v4().to_string();
    value.read_id = read_id;
    // reset these time based metrics 
    value.time_accessed = Utc::now();
    // prev chunk start has to be zero as there are now no previous chunks on anew read
    value.prev_chunk_start = 0;
}


impl DataServiceServicer {
    pub fn new(size: usize, run_id: String) -> DataServiceServicer {
        assert!(size > 0);
        let now = Instant::now();
        let json_file_path = Path::new("distributions.json");
        let files = ["NC_002516.2.squiggle.npy", "NC_003997.3.squiggle.npy"];
        let chunk_size = CHUNK_SIZE_1S as f64 * (BREAK_READS_MS as f64 / 1000.0);
        let channel_size = get_channel_size();
        let start_time: u64 = Utc::now().timestamp() as u64;
        let safe: Arc<Mutex<Vec<ReadInfo>>> =
            Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let action_response_safe: Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>> =
            Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let thread_safe_responses = Arc::clone(&action_response_safe);
        let thread_safe = Arc::clone(&safe);
        let is_setup = Arc::new(Mutex::new(false));
        let is_safe_setup = Arc::clone(&is_setup);

        let env = Env::default()
            .filter_or("MY_LOG_LEVEL", "info")
            .write_style_or("MY_LOG_STYLE", "always");
        env_logger::init_from_env(env);
        let dist = read_species_distribution(json_file_path);
        let views = read_views_of_data(files);

        let complete_read_tx = start_write_out_thread(run_id);
        let complete_read_tx_2 = complete_read_tx.clone();

        // start the thread to generate data
        thread::spawn(move || {
            let mut rng = rand::thread_rng();

            // read number for adding to unblock
            let mut read_number: u32 = 0;
            setup_channel_vec(channel_size, &thread_safe);

            // normal distribution is great news for smooth read length graphs in minoTour
            let normal: Normal<f64> = Normal::new(MEAN_READ_LEN, STD_DEV).unwrap();


            loop {
                debug!("Sequencer mock loop start");

                let start = now.elapsed().as_millis();
                thread::sleep(Duration::from_millis(400));

                // get some basic stats about what is going on at each channel
                let mut channels_with_reads = 0;
                // let read_chunks_counts = [0; 10];
                let mut reads_incremented = 0;
                let mut reads_generated = 0;
                let channel_size = get_channel_size();
                let mut num = thread_safe.lock().unwrap();

                for i in 0..channel_size {
                    let value = num.get_mut(i).unwrap();
                    let read_estimated_finish_time = value.start_time_seconds as usize + value.duration;
                    // experiment_time is the time the experimanet has started until now
                    let experiment_time = Utc::now().timestamp() as u64 - start_time;
                    info!("exp time: {}, read_finish_time: {}, is exp greater {}", experiment_time, read_estimated_finish_time, experiment_time as usize > read_estimated_finish_time);
                    if experiment_time as usize > read_estimated_finish_time || value.was_unblocked {
                        if value.write_out {
                            complete_read_tx.send(value.clone()).unwrap();
                        }
                        value.read.clear();
                        value.was_unblocked = false;
                        // Could be a slow problem here?
                        value.write_out = false;
                        // chance to aquire a read
                        if rand::thread_rng().gen_bool(0.99) {
                            read_number += 1;
                            reads_generated += 1;
                            generate_read(
                                files,
                                value,
                                &dist,
                                &views,
                                normal,
                                &mut rng,
                                &mut read_number,
                                &start_time,
                            )
                        }
                    }
                    else {
                        if value.read.len() > 0 && !value.was_unblocked{
                            reads_incremented += 1;
                        }
                    }
                }
                // info!("Reaads_newly created {reads_generated}, Reads inc. {reads_incremented} ");
                let _end = now.elapsed().as_millis() - start;
            }
        });
        // return our newly initialised DataServiceServicer to add onto the GRPC server
        DataServiceServicer {
            read_data: safe,
            complete_read_tx: complete_read_tx_2,
            action_responses: action_response_safe,
            read_count: 0,
            processed_actions: 0,
            setup: is_safe_setup,
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
        let now2 = Instant::now();
        let mut stream = _request.into_inner();
        let data_lock = Arc::clone(&self.read_data);
        let data_lock_unblock =  Arc::clone(&self.read_data);
        let complete_read_tx = self.complete_read_tx.clone();
        let tx = start_unblock_thread(data_lock_unblock, complete_read_tx);
        let channel_size = get_channel_size();

        debug!("Dropped lock {:#?}", now2.elapsed().as_millis());

        // let is_setup = self.setup.lock().unwrap().clone();

        let output = async_stream::try_stream! {
            while let Some(live_reads_request) = stream.next().await {
                let now2 = Instant::now();
                info!("Received get live read request at {:#?}", Utc::now());
                info!("{:#?}", live_reads_request);
                let live_reads_request = live_reads_request?;
                tx.send(live_reads_request).unwrap();
                let mut container: Vec<ReadData> = Vec::with_capacity(3000);
                let size = channel_size as f64 / 24_f64;
                let size = size.ceil() as usize;
                let mut channel: u32 = 1;
                let mut num_reads_stop_receiving: usize = 0;
                let mut num_channels_empty: usize = 0;
                // magic splitting into 24 reads using a drain like wow magic
                let mut loop_for_data: bool = true;
                while loop_for_data {
                    let mut z2 = {
                        debug!("Getting GRPC lock {:#?}", now2.elapsed().as_millis());
                        // send all the actions we wish to take and send them
                        let mut z1 = data_lock.lock().unwrap();
                        debug!("Got GRPC lock {:#?}", now2.elapsed().as_millis());
                        z1
                    };
                    let channel_size = get_channel_size();
                    for i in 0..channel_size {
                        let mut read_info = z2.get_mut(i).unwrap();
                        debug!("Elapsed at start of drain {}", now2.elapsed().as_millis());
                        if read_info.read.len() == 0 {
                            num_channels_empty += 1;
                        }
                        if read_info.stop_receiving {
                            num_reads_stop_receiving += 1;
                        }
                        info!("Read is {:#?}", read_info);
                        if (!read_info.stop_receiving || !read_info.was_unblocked) && read_info.read.len() > 0 {
                            // don't overslice our read
                            let start = read_info.prev_chunk_start;
                            let now_time = Utc::now();
                            let prev_time = read_info.start_time_utc;
                            let elapsed_time = now_time.time() - prev_time.time();
                            let stop = convert_seconds_to_samples(elapsed_time.num_milliseconds());
                            let stop = min(stop, read_info.read.len());
                            if start > stop || (stop - start) < 1600{
                                info!("sample isn't long enough ({})", stop as isize - start as isize);
                                thread::sleep(Duration::from_millis(200));
                                continue
                            }
                            read_info.time_accessed = now_time;
                            info!("Read is this long {} - {}  which is {} samples", start, stop, stop -start);
                            read_info.prev_chunk_start = stop;
                            let read_chunk = read_info.read[start..stop].to_vec();
                            container.push(ReadData{
                                    id: read_info.read_id.clone(),
                                    number: read_info.read_number.clone(),
                                    start_sample: 0,
                                    chunk_start_sample: 0,
                                    chunk_length:  read_chunk.len() as u64,
                                    chunk_classifications: vec![83],
                                    raw_data: convert_to_u8(read_chunk),
                                    median_before: 225.0,
                                    median: 110.0,

                            });
                            
                        }
                    }
                    mem::drop(z2);
                    // info!("Dropped GRPC lock {:#?}", now2.elapsed().as_millis());
                    if container.len() > 0 {
                        info!("Breaking out");
                        loop_for_data = false;
                    }
                    
                    // reset channel so we don't over total number of channels whilst spinning for data
                }

                let mut channel_data = HashMap::with_capacity(24);
                for chunk in container.chunks(24) {
                    for read_data in chunk {
                        channel_data.insert(channel, read_data.clone());
                        channel += 1;
                    }
                    yield GetLiveReadsResponse{
                        samples_since_start: 0,
                        seconds_since_start: 0.0,
                        channels: channel_data.clone(),
                        action_responses: vec![]
                    };
                    channel_data.clear();
                }
                container.clear();
                info!("replying {:#?}", now2.elapsed().as_millis());

            }
        };
        info!("End of stream");
        Ok(Response::new(Box::pin(output) as Self::get_live_readsStream))
    }

    async fn get_data_types(
        &self,
        request: Request<GetDataTypesRequest>,
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
