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
use crate::services::minknow_api::data::data_service_server::DataService;
use crate::services::minknow_api::data::get_live_reads_request::action;
use crate::services::minknow_api::data::{GetLiveReadsRequest, GetLiveReadsResponse, get_live_reads_request, get_live_reads_response, GetDataTypesRequest, GetDataTypesResponse};
use crate::services::minknow_api::data::get_live_reads_response::ReadData;
use crate::services::minknow_api::data::get_data_types_response::{DataType, data_type};
use crate::services::setup_conf::get_channel_size;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use std::sync::mpsc::{SyncSender, Receiver, sync_channel};
use futures::{Stream, StreamExt};
use tonic::{Request, Response, Status};
use std::collections::HashMap;
use std::{thread, u8};
use std::time::Duration;
use std::mem;
use std::fs::File;
use std::path::Path;
use std::cmp::{self, min};
use uuid::Uuid;

use rand::prelude::*;
use rand::distributions::WeightedIndex;
use rand_distr::{Distribution, Normal};
use memmap2::Mmap;
use ndarray::{ArrayView1, s, ArrayBase, ViewRepr, Dim};
use ndarray_npy::ViewNpyExt;
use serde::Deserialize;
use env_logger::Env;

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
struct RunSetup{
    setup: bool,
    first: u32,
    last: u32,
    dtype: i32,
}

impl RunSetup {
    pub fn new() -> RunSetup {
        return RunSetup{
            setup: false,
            first: 0,
            last: 0,
            dtype: 0
        };
    }
}

#[derive(Debug)]
pub struct DataServiceServicer{
    read_data: Arc<Mutex<Vec<ReadChunk>>>,
    tx:  SyncSender<GetLiveReadsRequest>,
    action_responses: Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>>,
    setup: Arc<Mutex<bool>>,
    read_count: usize,
    processed_actions: usize,
}

#[derive(Debug, Deserialize)]
struct Weights {
    weights: Vec<usize>,
    names: Vec<String>
}

#[derive(Debug, Deserialize)]
struct ReadInfo {
    read_id: String,
    read: Vec<u8>,
    stop_receiving: bool,
    read_number: u32
}

/// Process a get_live_reads_request StreamSetup, setting all the fields on the Threads RunSetup struct. This actually has no 
/// effect on the run itself, but could be implemented to do so in the future if required.
fn setup (setuppy: get_live_reads_request::Request, run_setup: & mut RunSetup, is_setup: &Arc<Mutex<bool>>) -> usize {
    info!("Received stream setup, setting up.");
    match setuppy {
        get_live_reads_request::Request::Setup(h) => {
            run_setup.first = h.first_channel;
            run_setup.last = h.last_channel;
            run_setup.setup = true;
            run_setup.dtype = h.raw_data_type;
            *is_setup.lock().unwrap() = true;
        },
        _ => {} // ignore everything else
    };
    // return we have prcessed 1 action
    1
}

/// Iterate through a given set of received actions and match the type of action to take
/// Unblock a read by emptying the ReadChunk held in channel readinfo dict
/// Stop receving a read sets the stop_receiving field on a ReadInfo struct to True, so we don't send it back.
/// Action Responses are appendable to a Vec which can be shared between threads, so can be accessed by the GRPC, which drains the Vec and sends back all responses.
/// Returns the number of actions processed.
fn take_actions(action_request: get_live_reads_request::Request, response_carrier: &Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>>, channel_read_info: &mut Vec<ReadInfo>) -> usize{
    // check that we have an action type and not a setup, whihc should be impossible
    info!("Processing non setup actions");
    let actions_processed = match action_request {
        get_live_reads_request::Request::Actions(actions) => {
            let mut add_response = response_carrier.lock().unwrap();
            let mut actions_processed: usize = 0;
            // iterate a vec of Action
            for action in actions.actions{
                let action_type = action.action.unwrap();
                let action_response = match action_type {
                    action::Action::Unblock(unblock) => {
                        unblock_reads(unblock, action.action_id, action.channel, channel_read_info)
                    },
                    action::Action::StopFurtherData(stop) => {
                        stop_sending_read(stop, action.action_id, action.channel, channel_read_info)
                    }
                };
                add_response.push(action_response);
                actions_processed += 1;
            }
            actions_processed
        },
        _ => {0}
    };
    actions_processed
}

/// Unblocks reads by clearing the channels (Represented by the index in a Vec) read vec.
fn unblock_reads(_action: get_live_reads_request::UnblockAction, action_id: String, channel_number: u32, channel_read_info: &mut Vec<ReadInfo>) -> get_live_reads_response::ActionResponse {
    // need a way of picking out channel by channel number or read ID, lets go by channel number for now -> lame but might work
    let value = channel_read_info.get_mut(channel_number as usize).expect("Failed on channel {channel_number}");
    value.read.clear();
    get_live_reads_response::ActionResponse{
        action_id,
        response: 0
    }
}

/// Stop sending read data, sets Stop receiving to True.
fn stop_sending_read(_action: get_live_reads_request::StopFurtherData, action_id: String, channel_number: u32, channel_read_info: &mut Vec<ReadInfo>)  -> get_live_reads_response::ActionResponse {
    // need a way of picking out channel by channel number or read ID
    let value = channel_read_info.get_mut(channel_number as usize).unwrap();
    value.stop_receiving = true;
    get_live_reads_response::ActionResponse{
        action_id,
        response: 0
    }
}

/// Read in the species distribution JSON, which gives us the odds of a species genome being chosen in a multi species sample.
/// 
/// The distirbution file is pre calculated by the included python script make_squiggle.py. This script uses the lengths of the specified genomes 
/// to calculate the likelihood of a genome being sequenced in a library that has the same amount of cells uses in the preperation.
/// This file can be manually created to alter library balances.
fn read_species_distribution(json_file_path: &Path) -> WeightedIndex<usize>{
    let file = File::open(json_file_path).expect("Distribution JSON file not found, please see README.");
    let w: Weights = serde_json::from_reader(file).expect("Error whilst reading distribution file.");
    let weights = w.weights;
    let names = w.names;
    info!("Read in weights for species {:#?}", names);
    let dist = WeightedIndex::new(&weights).unwrap();
    dist
}

/// Creates Memory mapped views of the precalculated numpy arrays of squiggle for reference genomes, generated by make_squiggle.py
/// 
/// Returns a Hashamap, keyed to the genome name that is accessed to pull a "read" (A slice of this "squiggle" array)
fn read_views_of_data(files: [&str; 2]) -> HashMap<&str, (usize, ArrayBase<ndarray::OwnedRepr<u8>, Dim<[usize; 1]>>)> {
    let mut views = HashMap::new();
    for file_path in files {
        let file = File::open(file_path).unwrap();
        let mmap = unsafe { Mmap::map(&file).unwrap() };
        let view: ArrayBase<ViewRepr<&u8>, Dim<[usize; 1]>>  = ArrayView1::<u8>::view_npy(&mmap).unwrap();
        let size = view.shape()[0];
        views.insert(file_path, (size, view.to_owned()));
    }
    views
}

/// TODO check if channels start at 1?
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
    for _ in 0..size{
        // setup the mutex vec
        let empty_read_chunk = ReadChunk{
            raw_data: vec![],
            read_id: String::from(""),
            ignore_me_lol: false,
            read_number: 0
        };
        num.push(empty_read_chunk);
        let read_info = ReadInfo{
            read_id: Uuid::nil().to_string(),
            // potench use with capacity?
            read: vec![],
            stop_receiving: false,
            read_number: 0
        };
        channel_read_info.push(read_info);
    }
    channel_read_info
}

/// Generate an inital read, which is stored as a ReadInfo in the channel_read_info vec. This is mutated in place.
fn generate_read(read_chunk: &mut ReadChunk, files: [&str; 2],
    value: &mut ReadInfo, dist: &WeightedIndex<usize>,
    views: &HashMap<&str, (usize, ArrayBase<ndarray::OwnedRepr<u8>, Dim<[usize; 1]>>)>,
    normal:  Normal<f64>,
    rng: &mut ThreadRng,
    read_number: &mut u32) {
    value.read.clear();
    
    // ne read, so don't ignore it on the actual sgrpc side
    read_chunk.ignore_me_lol = false;
    // chance to acquire
    if rand::thread_rng().gen_bool(0.7){
        value.read_number = *read_number;
        let file_choice = files[dist.sample(rng)];
        let file_info = &views[file_choice];
        // start point in file
        let start: usize = rng.gen_range(0..file_info.0-1000) as usize;
        let read_length: usize = normal.sample(&mut rand::thread_rng()) as usize;
        // don;t over slice our read
        let end: usize = cmp::min(start+ read_length, file_info.0 -1);
        // slice the view to get our full read
        value.read.append(&mut file_info.1.slice(s![start..end]).to_vec());
        value.read_id = Uuid::new_v4().to_string();
    } else {
        // we actually haven't started a read so we need to take the read numebr back down one, as the addition is fixed
        *read_number -= 1
    }
        

}

/// Increment the length of the read raw data available to be served over the GRPC by draining it from the vec that the 
/// thread stores data in, and placing the consumed drain data into the ReadChunk that is accessible by the GRPC open thread.
/// 
fn increment_shared_read(value: &mut ReadInfo,
                  chunk_size: &usize,
                  read_chunk: &mut ReadChunk,
                ) {
    let ran = min(value.read.len(), *chunk_size);
    let mut a: Vec<u8> = value.read.drain(..ran).collect();
    // a bit approximate but if we have finished the read, we can clear the shared cache
    if value.read.len() == 0{
        read_chunk.raw_data.clear();
    }
    // issued a stop receiving so no more data sent please
    if value.stop_receiving{
        read_chunk.ignore_me_lol = true
    } else {
        if value.read.len() != 0 {
            read_chunk.raw_data.append(&mut a);
        }
        read_chunk.read_id = value.read_id.clone();
    }
}


impl DataServiceServicer {
    pub fn new(size: usize, ) -> DataServiceServicer{
        assert!(size > 0);
        let now = Instant::now();
        let json_file_path = Path::new("distributions.json");
        let files = ["NC_002516.2.squiggle.npy", "NC_003997.3.squiggle.npy"];
        let chunk_size = CHUNK_SIZE_1S as f64 * (BREAK_READS_MS as f64 / 1000.0);
        let channel_size = get_channel_size();
        let chunk_size = chunk_size as usize;
        let safe: Arc<Mutex<Vec<ReadChunk>>> = Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let action_response_safe: Arc<Mutex<Vec<get_live_reads_response::ActionResponse>>> = Arc::new(Mutex::new(Vec::with_capacity(channel_size)));
        let thread_safe_responses = Arc::clone(&action_response_safe);
        let thread_safe = Arc::clone(&safe);
        let is_setup = Arc::new(Mutex::new(false));
        let is_safe_setup = Arc::clone(&is_setup);
        let (tx, rx): (SyncSender<GetLiveReadsRequest>, Receiver<GetLiveReadsRequest>) = sync_channel(channel_size);
        let env = Env::default()
        .filter_or("MY_LOG_LEVEL", "info")
        .write_style_or("MY_LOG_STYLE", "always");
        env_logger::init_from_env(env);
        info!("some information log");
        let dist = read_species_distribution(json_file_path);
        let views = read_views_of_data(files);

        // start the thread to generate data
        thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut total_actions_processed: usize = 0;
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

                let mut num = thread_safe.lock().unwrap();
                let received: Vec<GetLiveReadsRequest> = rx.try_iter().collect();

                // We have like some actions to adress before we do anything, if this is
                // fast enough we don't have to thread it
                if !received.is_empty(){
                    debug!("Actions received");
                    for get_live_req in received {
                        let request_type = get_live_req.request.unwrap();
                        let actions_processed = match request_type {
                            // set up request
                            get_live_reads_request::Request::Setup(_) => setup(request_type, & mut run_setup, &is_setup),
                            // list of actions
                            get_live_reads_request::Request::Actions(_) => take_actions(request_type, &thread_safe_responses, &mut channel_read_info)
                        };
                        total_actions_processed += actions_processed;
                    }
                }
                info!("Total actions processed: {}", total_actions_processed);
                // get some basic stats about what is going on at each channel
                let mut channels_with_reads = 0;
                // let read_chunks_counts = [0; 10];
                let mut reads_incremented = 0;
                let mut reads_generated = 0;
                let channel_size= get_channel_size();
                for i in 0..channel_size
                {
                    let read_chunk = num.get_mut(i).unwrap();
                    let value = channel_read_info.get_mut(i).unwrap();
                    if value.read.is_empty() {
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
                            &mut read_number
                        )
                    } else {
                        reads_incremented += 1;
                        // read increment_shared_read doc to understand what is going on here.
                        increment_shared_read(
                            value,
                            &chunk_size,
                            read_chunk,
                        )
                    }
                    if !value.read.is_empty(){
                        channels_with_reads += 1;
                    }
                    // read_chunks_counts[(read_chunk.raw_data.len() / 4000)] += 1;
                }
                debug!("Channels with reads {channels_with_reads}");
                debug!("Reads incremented {reads_incremented}");
                debug!("Reaads_newly generate {reads_generated}");
                // info!("Chunk ;engh distribution {:#?}", read_chunks_counts);
                
                let _end =  now.elapsed().as_millis() - start;
            }
        });
        // return our newly initialised DataServiceServicer to add onto the GRPC server
        DataServiceServicer{
            read_data: safe,
            tx,
            action_responses: action_response_safe,
            read_count: 0,
            processed_actions: 0,
            setup: is_safe_setup
        }
    }
}

#[tonic::async_trait]
impl DataService for DataServiceServicer {

    type get_live_readsStream = Pin<Box<dyn Stream<Item = Result<GetLiveReadsResponse, Status>> + Send + 'static>>;

    async fn get_live_reads(
        &self,
        _request:  Request<tonic::Streaming<GetLiveReadsRequest>>,
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
                    info!("Getting GRPC lock {:#?}", now2.elapsed().as_millis());
                    // send all the actions we wish to take and send them
                    let mut z1 = data_lock.lock().unwrap();
                    info!("Got GRPC lock {:#?}", now2.elapsed().as_millis());

                    let mut z2 = z1.clone();

                    for i in 0..channel_size {
                        z1[i].raw_data.clear();
                    }
                    mem::drop(z1);
                    info!("Dropped GRPC lock {:#?}", now2.elapsed().as_millis());

                    z2
                };
                let mut container = vec![];
                let size = channel_size as f64 / 24 as f64;
                let size = size.ceil() as usize;
                let mut channel: u32 = 0;
                // magic splitting into 24 reads using a drain like wow magic
                for _ in 0..(size){
                    let mut h = HashMap::with_capacity(24);
                    for read_chunk in z2.drain(..min(24, z2.len())){
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
                
                for channel_slice in 0..size {
                    debug!("channel slice number is {}", channel_slice);
                    yield GetLiveReadsResponse{
                        samples_since_start: 0,
                        seconds_since_start: 0.0,
                        channels: container[channel_slice].clone(),
                        action_responses: vec![]
                    };
                }
            }
        };

        info!("replying {:#?}", now2.elapsed().as_millis());
        Ok(Response::new(Box::pin(output)
            as Self::get_live_readsStream))
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