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
use uuid::Uuid;

use env_logger::Env;
use memmap2::Mmap;
use ndarray::{s, ArrayBase, ArrayView1, Dim, ViewRepr};
use ndarray_npy::ViewNpyExt;
use rand::distributions::WeightedIndex;
use rand::prelude::*;
use rand_distr::{Distribution, Normal};
use serde::Deserialize;
use tonic::{Request, Response, Status};
use chrono::prelude::*;
use frust5_api::*;
use fnv::FnvHashSet;


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
const MEAN_READ_LEN: f64 = 20000.0 / 450.0 * 4000.0;
const STD_DEV: f64 = 3000.0;
#[derive(Debug, Deserialize, Clone)]
struct ReadChunk {
    raw_data: Vec<u8>,
    read_id: String,
    ignore_me_lol: bool,
    read_number: u32,
}

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
    read_data: Arc<Mutex<Vec<ReadChunk>>>,
    tx: SyncSender<GetLiveReadsRequest>,
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
#[derive(Debug, Deserialize, Clone)]
struct ReadInfo {
    read_id: String,
    read: Vec<u8>,
    stop_receiving: bool,
    read_number: u32,
    start_coord: usize,
    stop_coord: usize,
    was_unblocked: bool,
    file_name: String,
    write_out: bool,
    start_time: u64,
    start_mux: u8,
    end_reason: u8,
    channel_number: String
}

/// Add on between 0.7 and 1.5 of a chunk onto unblocked reads
fn extend_unblocked_reads(normal: Normal<f64>, end: usize) -> usize {
    let addition = normal.sample(&mut rand::thread_rng()) * 4000_f64;
    let addition = addition.floor() as usize;
    end + addition
}

fn convert(b: Vec<u8>) -> Vec<i16> {
    b.into_iter().map(|f| f as i16).collect()
}

/// Start the thread that will ahndle writing out the FAST5 file,
fn start_write_out_thread(run_id: String) -> SyncSender<ReadInfo> {
    let (complete_read_tx, complete_read_rx) = sync_channel(4000);
    thread::spawn(move || {
        let mut read_infos: Vec<ReadInfo> = Vec::with_capacity(6000);
        let exp_start_time = Utc::now();
        let iso_time = exp_start_time.to_rfc3339_opts(SecondsFormat::Millis, false);
        let config = _load_toml("config.toml");
        let context_tags = HashMap::from([
            ("barcoding_enabled", "0"),
            ("experiment_duration_set", config.experiment_duration_set.as_str()),
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
            ("exp_start_time", ""),
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
        let mut read_numbers_seen: FnvHashSet<u32> = FnvHashSet::with_capacity_and_hasher(4000, Default::default());
        let files = ["NC_002516.2.squiggle.npy", "NC_003997.3.squiggle.npy"];
        let views = read_views_of_data(files);
        let normal: Normal<f64> = Normal::new(1.1, 0.1).unwrap();
        let mut file_counter = 0;
        // loop to collect reads and write out files
        loop {

            for finished_read_info in complete_read_rx.try_iter(){
                read_infos.push(finished_read_info);
                if read_infos.len() >= 4000{
                    let fast5_file_name = format!("{}_pass_{}_{}.fast5", config.flowcell_name, &run_id[0..6], file_counter);
                    // drain 4000 reads and write them into a FAST5 file
                    let mut multi =
                    MultiFast5File::new(fast5_file_name.clone(), OpenMode::Append);
                    for to_write_info in read_infos.drain(..4000) {
                        if !read_numbers_seen.insert(to_write_info.read_number){
                            continue
                        }
                        let new_end = extend_unblocked_reads(normal, to_write_info.stop_coord);
                        let file_info = &views[to_write_info.file_name.as_str()];
                        let new_end: usize = cmp::min(new_end, file_info.0 - 1);
                        let signal = file_info.1.slice(s![to_write_info.start_coord..new_end]).to_vec();
                        let signal = convert(signal);
                        let raw_attrs = HashMap::from([
                            ("duration", RawAttrsOpts::Duration(signal.len() as u32)),
                            ("end_reason", RawAttrsOpts::EndReason(to_write_info.end_reason)),
                            ("median_before", RawAttrsOpts::MedianBefore(100.0)),
                            ("read_id", RawAttrsOpts::ReadId(to_write_info.read_id.as_str())),
                            ("read_number", RawAttrsOpts::ReadNumber(to_write_info.read_number as i32)),
                            ("start_mux", RawAttrsOpts::StartMux(to_write_info.start_mux)),
                            ("start_time", RawAttrsOpts::StartTime(to_write_info.start_time)),
                        ]);
                        let channel_info = ChannelInfo::new(8192_f64, 6.0, 1500.0, 4000.0, to_write_info.channel_number.clone());
                        multi
                        .create_empty_read(
                            to_write_info.read_id.clone(),
                            run_id.clone(),
                            &tracking_id,
                            &context_tags,
                            channel_info,
                            &raw_attrs,
                            signal
                        ).unwrap();
                    }
                    file_counter += 1;
                    read_numbers_seen.clear();
                }                
            }
        }
    });
    complete_read_tx
} 

/// Process a get_live_reads_request StreamSetup, setting all the fields on the Threads RunSetup struct. This actually has no
/// effect on the run itself, but could be implemented to do so in the future if required.
fn setup(
    setuppy: get_live_reads_request::Request,
    run_setup: &mut RunSetup,
    is_setup: &Arc<Mutex<bool>>,
) -> (usize, usize, usize) {
    info!("Received stream setup, setting up.");
    match setuppy {
        get_live_reads_request::Request::Setup(h) => {
            run_setup.first = h.first_channel;
            run_setup.last = h.last_channel;
            run_setup.setup = true;
            run_setup.dtype = h.raw_data_type;
            *is_setup.lock().unwrap() = true;
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
    response_carrier: &Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>>,
    channel_read_info: &mut Vec<ReadInfo>,
) -> (usize, usize, usize) {
    // check that we have an action type and not a setup, whihc should be impossible
    debug!("Processing non setup actions");
    let (unblocks_processed, stop_rec_processed) = match action_request {
        get_live_reads_request::Request::Actions(actions) => {
            let mut add_response = response_carrier.lock().unwrap();
            let mut unblocks_processed: usize = 0;
            let mut stop_rec_processed: usize = 0;

            // iterate a vec of Action
            for action in actions.actions {
                let action_type = action.action.unwrap();
                let (action_response, unblock_count, stopped_count) = match action_type {
                    action::Action::Unblock(unblock) => {
                        unblock_reads(unblock, action.action_id, action.channel, channel_read_info)
                    }
                    action::Action::StopFurtherData(stop) => {
                        stop_sending_read(stop, action.action_id, action.channel, channel_read_info)
                    }
                };
                add_response.push(action_response);
                unblocks_processed += unblock_count;
                stop_rec_processed += stopped_count;
            }
            (unblocks_processed, stop_rec_processed)
        }
        _ => (0, 0),
    };
    (0, unblocks_processed, stop_rec_processed)
}

/// Unblocks reads by clearing the channels (Represented by the index in a Vec) read vec.
fn unblock_reads(
    _action: get_live_reads_request::UnblockAction,
    action_id: String,
    channel_number: u32,
    channel_read_info: &mut Vec<ReadInfo>,
) -> (get_live_reads_response::ActionResponse, usize, usize) {
    // need a way of picking out channel by channel number or read ID, lets go by channel number for now -> lame but might work
    let value = channel_read_info
        .get_mut(channel_number as usize)
        .expect("Failed on channel {channel_number}");
    value.read.clear();
    // set the was unblocked field for writing out
    value.was_unblocked = true;
    value.write_out = true;
    // end reason of unblock
    value.end_reason = 4;
    (
        get_live_reads_response::ActionResponse {
            action_id,
            response: 0,
        },
        1,
        0,
    )
}

/// Stop sending read data, sets Stop receiving to True.
fn stop_sending_read(
    _action: get_live_reads_request::StopFurtherData,
    action_id: String,
    channel_number: u32,
    channel_read_info: &mut Vec<ReadInfo>,
) -> (get_live_reads_response::ActionResponse, usize, usize) {
    // need a way of picking out channel by channel number or read ID
    let value = channel_read_info.get_mut(channel_number as usize).unwrap();
    value.stop_receiving = true;
    (
        get_live_reads_response::ActionResponse {
            action_id,
            response: 0,
        },
        0,
        1,
    )
}

/// Read in the species distribution JSON, which gives us the odds of a species genome being chosen in a multi species sample.
///
/// The distirbution file is pre calculated by the included python script make_squiggle.py. This script uses the lengths of the specified genomes
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
) -> HashMap<&str, (usize, ArrayBase<ndarray::OwnedRepr<u8>, Dim<[usize; 1]>>)> {
    let mut views = HashMap::new();
    for file_path in files {
        let file = File::open(file_path).unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        let view: ArrayBase<ViewRepr<&u8>, Dim<[usize; 1]>> =
            ArrayView1::<u8>::view_npy(&mmap).unwrap();
        let size = view.shape()[0];
        views.insert(file_path, (size, view.to_owned()));
    }
    views
}

/// 
/// Create and return a Vec that stores the internal data generate thread state. Also populate the shared state Vec that provides data to the GRPC endpoint
///  with empty ReadChunk structs. Only called once, when the server is booted up.
///
/// The vec is the length of the set number of channels with each element representing a "channel". These are accessed by index, with channel 1 represented by element at index 1.
/// The created Vec is populated by ReadInfo structs, which are used to track the ongoing state of a channel during a run.
///
/// The function also populates a similar Vec which is shared between the data generation thread and the GRPC get_live_reads endpoint with empty ReadChunks, which are served
///  to any clients requesting them. This vec already exists and is shared aroun, so is not returned by this function.
fn setup_channel_vecs(size: usize, thread_safe: &Arc<Mutex<Vec<ReadChunk>>>) -> Vec<ReadInfo> {
    // Create channel dictionary - here we hold a vecDeque of chunks to be served each iteration below
    // setup channel read info dict
    let mut channel_read_info: Vec<ReadInfo> = vec![];
    let thread_safe_chunks = Arc::clone(&thread_safe);

    let mut num = thread_safe_chunks.lock().unwrap();
    for channel_number in 0..size {
        // setup the mutex vec
        let empty_read_chunk = ReadChunk {
            raw_data: vec![],
            read_id: String::from(""),
            ignore_me_lol: false,
            read_number: 0,
        };
        num.push(empty_read_chunk);
        let read_info = ReadInfo {
            read_id: Uuid::nil().to_string(),
            // potench use with capacity?
            read: vec![],
            stop_receiving: false,
            read_number: 0,
            start_coord: 0,
            stop_coord: 0,
            was_unblocked: false,
            file_name: "".to_string(),
            write_out: false, 
            start_time: 0,
            channel_number: channel_number.to_string(),
            end_reason: 0,
            start_mux: 1
        };
        channel_read_info.push(read_info);
    }
    channel_read_info
}

/// Generate an inital read, which is stored as a ReadInfo in the channel_read_info vec. This is mutated in place.
fn generate_read(
    read_chunk: &mut ReadChunk,
    files: [&str; 2],
    value: &mut ReadInfo,
    dist: &WeightedIndex<usize>,
    views: &HashMap<&str, (usize, ArrayBase<ndarray::OwnedRepr<u8>, Dim<[usize; 1]>>)>,
    normal: Normal<f64>,
    rng: &mut ThreadRng,
    read_number: &mut u32,
) {
    // make sure the read data is clear and set stop receieivng to false so we don't accidentally keep the read
    value.read.clear();
    value.stop_receiving = false;
    // update as this read hasn't yet been unblocked
    value.was_unblocked = false;
    // signal psoitive end_reason
    value.end_reason = 1;
    // we want to write this out at the end
    value.write_out = true;
    // read start time in samples (seconds * 4000)
    value.start_time = Utc::now().timestamp() as u64;

    // new read, so don't ignore it on the actual grpc side
    read_chunk.ignore_me_lol = false;
    value.read_number = *read_number;
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
    // set the info we need to write the file out
    value.file_name = file_choice.to_string();
    value.start_coord = start;
    value.stop_coord = end;
    let read_id = Uuid::new_v4().to_string();
    value.read_id = read_id;
}

/// Increment the length of the read raw data available to be served over the GRPC by draining it from the vec that the
/// thread stores data in, and placing the consumed drain data into the ReadChunk that is accessible by the GRPC open thread.
///
fn increment_shared_read(value: &mut ReadInfo, chunk_size: &usize, read_chunk: &mut ReadChunk) {
    let ran = min(value.read.len(), *chunk_size);
    let mut a: Vec<u8> = value.read.drain(..ran).collect();

    // issued a stop receiving so no more data sent please
    if value.stop_receiving {
        read_chunk.ignore_me_lol = true
    } else {
        read_chunk.raw_data.append(&mut a);
        read_chunk.read_id = value.read_id.clone();
    }
}

impl DataServiceServicer {
    pub fn new(size: usize, run_id: String) -> DataServiceServicer {
        assert!(size > 0);
        let now = Instant::now();
        let json_file_path = Path::new("distributions.json");
        let files = ["NC_002516.2.squiggle.npy", "NC_003997.3.squiggle.npy"];
        let chunk_size = CHUNK_SIZE_1S as f64 * (BREAK_READS_MS as f64 / 1000.0);
        let channel_size = get_channel_size();
        let chunk_size = chunk_size as usize;
        let safe: Arc<Mutex<Vec<ReadChunk>>> =
            Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let action_response_safe: Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>> =
            Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let thread_safe_responses = Arc::clone(&action_response_safe);
        let thread_safe = Arc::clone(&safe);
        let is_setup = Arc::new(Mutex::new(false));
        let is_safe_setup = Arc::clone(&is_setup);
        let (tx, rx): (
            SyncSender<GetLiveReadsRequest>,
            Receiver<GetLiveReadsRequest>,
        ) = sync_channel(channel_size);
        let env = Env::default()
            .filter_or("MY_LOG_LEVEL", "info")
            .write_style_or("MY_LOG_STYLE", "always");
        env_logger::init_from_env(env);
        info!("some information log");
        let dist = read_species_distribution(json_file_path);
        let views = read_views_of_data(files);

        let complete_read_tx = start_write_out_thread(run_id);

        // start the thread to generate data
        thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut total_unblocks_processed: usize = 0;
            let mut total_stop_rec_processed: usize = 0;
            let mut total_setup_processed: usize = 0;

            // read number for adding to unblock
            let mut read_number: u32 = 0;
            let mut channel_read_info = setup_channel_vecs(channel_size, &thread_safe);

            // normal distribution is great news for smooth read length graphs in minoTour
            let normal: Normal<f64> = Normal::new(MEAN_READ_LEN, STD_DEV).unwrap();
            let mut run_setup = RunSetup::new();

            loop {
                debug!("Sequencer mock loop start");

                let start = now.elapsed().as_millis();
                thread::sleep(Duration::from_millis(400));

                let received: Vec<GetLiveReadsRequest> = rx.try_iter().collect();
                let mut num = thread_safe.lock().unwrap();
                // We have like some actions to adress before we do anything, if this is
                // fast enough we don't have to thread it
                if !received.is_empty() {
                    debug!("Actions received");
                    for get_live_req in received {
                        let request_type = get_live_req.request.unwrap();
                        let (setup_proc, unblock_proc, stop_rec_proc) = match request_type {
                            // set up request
                            get_live_reads_request::Request::Setup(_) => {
                                setup(request_type, &mut run_setup, &is_setup)
                            }
                            // list of actions
                            get_live_reads_request::Request::Actions(_) => take_actions(
                                request_type,
                                &thread_safe_responses,
                                &mut channel_read_info,
                            ),
                        };
                        total_unblocks_processed += unblock_proc;
                        total_setup_processed += setup_proc;
                        total_stop_rec_processed += stop_rec_proc
                    }
                }
                info!(
                    "Total unblocks processed {}, Total stop rec processed {}",
                    total_unblocks_processed, total_stop_rec_processed
                );
                // get some basic stats about what is going on at each channel
                let mut channels_with_reads = 0;
                // let read_chunks_counts = [0; 10];
                let mut reads_incremented = 0;
                let mut reads_generated = 0;
                let channel_size = get_channel_size();
                for i in 0..channel_size {
                    let read_chunk = num.get_mut(i).unwrap();
                    let value = channel_read_info.get_mut(i).unwrap();
                    if value.read.is_empty() {
                        if value.write_out {
                            complete_read_tx.send(value.clone()).unwrap();
                        }
                        value.write_out = false;
                        // chance to aquire a read
                        if rand::thread_rng().gen_bool(0.07) {
                            read_number += 1;
                            reads_generated += 1;
                            generate_read(
                                read_chunk,
                                files,
                                value,
                                &dist,
                                &views,
                                normal,
                                &mut rng,
                                &mut read_number,
                            )
                        }
                    } else {
                        reads_incremented += 1;
                        // read increment_shared_read doc to understand what is going on here.
                        increment_shared_read(value, &chunk_size, read_chunk)
                    }
                    if !value.read.is_empty() {
                        channels_with_reads += 1;
                    }
                }
                info!("Reaads_newly created {reads_generated}, Reads inc. {reads_incremented} ");
                let _end = now.elapsed().as_millis() - start;
            }
        });
        // return our newly initialised DataServiceServicer to add onto the GRPC server
        DataServiceServicer {
            read_data: safe,
            tx,
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
        info!("Received get_live_reads request");
        let tx = self.tx.clone();
        let mut stream = _request.into_inner();
        let data_lock = Arc::clone(&self.read_data);
        let channel_size = get_channel_size();

        debug!("Dropped lock {:#?}", now2.elapsed().as_millis());

        // let is_setup = self.setup.lock().unwrap().clone();

        let output = async_stream::try_stream! {
            while let Some(live_reads_request) = stream.next().await {
                let now2 = Instant::now();
                let live_reads_request = live_reads_request?;
                tx.send(live_reads_request).unwrap();
                let mut z2 = {
                    debug!("Getting GRPC lock {:#?}", now2.elapsed().as_millis());
                    // send all the actions we wish to take and send them
                    let mut z1 = data_lock.lock().unwrap();
                    debug!("Got GRPC lock {:#?}", now2.elapsed().as_millis());

                    let mut z2 = z1.clone();

                    for i in 0..channel_size {
                        z1[i].raw_data.clear();
                    }
                    mem::drop(z1);
                    debug!("Dropped GRPC lock {:#?}", now2.elapsed().as_millis());

                    z2
                };
                let mut container = vec![];
                let size = channel_size as f64 / 24 as f64;
                let size = size.ceil() as usize;
                let mut channel: u32 = 0;
                let mut num_reads_stop_receiving = 0;
                let mut num_channels_empty = 0;
                // magic splitting into 24 reads using a drain like wow magic
                for _ in 0..(size){
                    let mut h = HashMap::with_capacity(24);
                    for read_chunk in z2.drain(..min(24, z2.len())){
                        if read_chunk.raw_data.len() == 0 {
                            num_channels_empty += 1;
                        }
                        if read_chunk.ignore_me_lol {
                            num_reads_stop_receiving += 1;
                        }
                        if !read_chunk.ignore_me_lol && read_chunk.raw_data.len() > 0{
                            h.insert(channel, ReadData{
                                id: read_chunk.read_id,
                                number: 0,
                                start_sample: 0,
                                chunk_start_sample: 0,
                                chunk_length: 0,
                                chunk_classifications: vec![83],
                                raw_data: read_chunk.raw_data,
                                median_before: 225.0,
                                median: 110.0,

                            });
                        }
                        channel += 1;
                    }
                    container.push(h)
                }
                info!("Reads in this batch ignored {}, Reads empty in this batch {}", num_reads_stop_receiving, num_channels_empty);
                for channel_slice in 0..size {
                    debug!("channel slice number is {}", channel_slice);
                    if container[channel_slice].len() > 0 {
                        yield GetLiveReadsResponse{
                            samples_since_start: 0,
                            seconds_since_start: 0.0,
                            channels: container[channel_slice].clone(),
                            action_responses: vec![]
                        };
                    }

                }
                thread::sleep(Duration::from_millis(250));
            }
        };

        info!("replying {:#?}", now2.elapsed().as_millis());
        Ok(Response::new(Box::pin(output) as Self::get_live_readsStream))
    }

    async fn get_data_types(
        &self,
        request: Request<GetDataTypesRequest>,
    ) -> Result<Response<GetDataTypesResponse>, Status> {
        Ok(Response::new(GetDataTypesResponse {
            uncalibrated_signal: Some(DataType {
                r#type: 2,
                big_endian: false,
                size: 4,
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
