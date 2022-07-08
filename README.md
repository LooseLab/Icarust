# Icarust
Rust based Minknow simulator
---
🦀🚀
![Lament of Icarust](img/Draper_Herbert_James_Mourning_for_Icarus.jpg "The lament of Icarus")
Figure 1 - Accurate depiction of a man learning Rust ☠️
### `Warning`
Icarust is a work in progress - as such some small bugs are to be expected.

## Quick start

Icarust requires Rust > 1.56. In order to install Rust instructions can be found [here.](https://www.rust-lang.org/tools/install)

In order to run Icarust with the pre set config and squiggle - 

```zsh
git clone https://github.com/Adoni5/Icarust
cd Icarust
cargo run --release -- --Profile_tomls/config.toml
```

## Changing Configured settings
<details open>
<summary></summary>
To configure an Icarust simulation a config [TOML](https://toml.io/en/) file is passed to the initialise command. TOML files are minimal and easy to read markup files. Each line is either a key-value pair or a 'table' 
heading.

The config file is split into a global settings, [Parameters](#parameters) and Sample.
### Global fields
Global fields are applied more as configuration variables that apply throughout the codebase.
|          Key |       Type      | Required | Description |
|:-------------|:---------------:|:-----------:|:--------:|
| output_path | string | True | The path to a directory that the resulting FAST5 and readfish unblocked_read_ids.txt file will be written to. | 
| global_mean_read_length | int | False | If set, any samples that do not have their own read length field will use this value| 
| random_seed | int  | False | The seed to use in any Random Number generation. If set this makes exeriments repeatable if the value is retained. | 

### Parameters
The parameters are applied to the "sequencer". They are used to setup the GRPC server so that it is connectable to. They are also written out in the FAST5 files.
<!-- Provide an example section here -->
|          Key |       Type      | Required | Description |
|:-------------|:---------------:|:-----------:|:--------:|
| sample_name | string | True | The sample name for the simulation | 
| experiment_name | string | True | The experiment name for the simulation| 
| flowcell_name | string  | True | The flowcell name for the simulation | 
| experiment_duration | int  | False | The experiment duration **CURRENTLY UNUSED** | 
| device_id | string  | True | The device ID - can be anything. | 
| position | string  | True | Position name. This has to match what readfish is looking for. |

### Sample
    name: String,
    input_genome: std::path::PathBuf,
    mean_read_length: Option<f64>,
    weight: usize,
    weights_files: Option<Vec<std::path::PathBuf>>,
    amplicon: Option<bool>,
    barcodes: Option<Vec<String>>,
    barcode_weights: Option<Vec<usize>>,
    uneven: Option<bool>,
The sample configures what squiggle will be served. This is provided as an array of tables - i.e it is possible to specify more than one sample field. 
|          Key |       Type      | Required | Description |
|:-------------|:---------------:|:-----------:|:--------:|
| name | string | True | The sample name. | 
| input_genome | string | True | Path to **either** the squiggle array or a directory of squiggle arrays. If a directory, all squiggle files will be considered as possible sources of reads for this sample.| 
| mean_read_length | float  | False | The mean read length for the distribution of this sample. | 
| weight | int  | True | The relative weight of this sample against any other sample. | 
| weights_files | array[string]  | False | An array of paths to [distribution.json](#distributions) files, if you wish to specify relative likelihood of drawing a read from a given squiggle file. | 
| amplicon | bool | False | Is the sample from a PCR amplicon based run. Means that read squiggle is always the complete length of a squiggle file. |
| barcodes | array[string] | False | Array of Barcode names. Multiple Barcodes can be provided for one sample |
| barcode_Weights | array[string] | False | The relative distribution of barcodes. If not provided any barcodes will be assigned a random likelihood. If provided must same length as the barcodes array.|
| uneven | bool | False | Uneven likelihood of choosing a squiggle array. **unused**|
</details>

## Generating squiggle to serve
<details open>
<summary></summary>
In the python directory a script called make_squiggle.py exists. I recommend [conda](https://conda.io/projects/conda/en/latest/user-guide/install/linux.html) in order to create the python environment to use this script. 

`NB` - A python package we _currently_ use is scrappie - which depends on a few C libraries. The names of these for debian systems are listed below. 


    libcunit1
    libcunit1-dev
    libhdf5
    libhdf5-dev
    libopenblas-base
    libopenblas-dev

These can be install with `apt-get install`.

`sudo apt-get install libcunit1 libcunit1-dev libhdf5 libhdf5-dev libopenblas-base libopenblas-dev`

Now that you have all the packages required, in the python directory -

```zsh
conda env create -f icarust.yaml
```

To then generate signal to be served, use the provided script, giving any reference files you wish to use as arguments, space seperated. An example -

```zsh
python make_squiggle.py mixed_ref.fa
```

### Distributions

### `Warning` -> If a distributions.json file already exists, this will append to it.

.npy files containing r9.4.1 sequence should now be present in the base directory. These files will have the name of the contig they contain sequence for.

</details>

# Ideology
<details>
<summary>Ideology</summary>
![Icarust Ideology](img/Icarust_ideology.excalidraw.png "Basic flowchart of icarust architecture")
The image above shows the structure of Icarust. The asynchronous main thread is a tokio runtime that handles GRPC requests from readfish. The core rust package that handles this is called Tonic.

### Read fish connecting
There are two servers, a manager and a position server. Readfish first queries the manager sevrer to get the name and port of the position, then it creates a bi-directional streaming RPC request to the position port, sending actions to perform on reads and receiving read chunks.

### Data generation
When Icarust is started, two threads are created, aptly named the Data generation thread and the Data write out thread. These serve as stand in for the actual sequencer part of minKNOW. The data generation thread is a loop with a 400ms pause. This loop first checks for actions that have been sent by the GRPC end point to be handled, namely unblocks. 

It then manipulates a Vec (If from a python background think a List that can only contain one type of elements). This Vec has one element for each channel. The element is a struct, which contains information about the read that the "channel" is currently "sequencing". If there is no signal data stored in the struct, the thread will randomly select a read length from a normal distribution, a contig to pull from using a weighted choice based on contig length, and a random start point. It then reads the signal from a memory map to the signal .npy files, and stores the signal in the Struct. 

This Vec is shared between the Tonic end point and the Data generation thread using a ARC (atomic reference counter) and a mutex for mutual exclusion. This allows either thread to get a lock on the vec whilst it is being read and modfified. 

### Serving reads
When a GetLiveReadsRequest GRPC request comes in, the GRPC server gets a lock on the channels Vec. 
</details>