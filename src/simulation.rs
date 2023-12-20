//! Defines code used to create the R10 signal from pore models.
use fnv::{FnvHashMap, FnvHashSet};
use indicatif::{ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use memmap2::Mmap;
use ndarray::ArrayView1;
use ndarray_npy::ViewNpyExt;
use needletail::parse_fastx_file;
use needletail::parser::SequenceRecord;
use needletail::{FastxReader, Sequence};
use nom::character::complete::{alpha1, char, digit1, line_ending, multispace0, multispace1, tab};
use nom::combinator::{map, map_res, recognize};
use nom::multi::many1;
use nom::number::complete::double;
use nom::sequence::{preceded, separated_pair, terminated, tuple};
use nom::IResult;
use rand::seq::SliceRandom;
use rand::prelude::*;
use rand_distr::Normal;
use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;

lazy_static! {
    static ref HASHSET: FnvHashSet<char> = {
        let mut set = FnvHashSet::default();
        set.insert('A');
        set.insert('C');
        set.insert('G');
        set.insert('T');
        set
    };
}

/// Transform a nucleic acid sequence into its "normalized" form.
///
/// The normalized form is:
///  - only AGCTN and possibly - (for gaps)
///  - strip out any whitespace or line endings
///  - lowercase versions of these are uppercased
///  - U is converted to T (make everything a DNA sequence)
///  - some other punctuation is converted to gaps
///  - IUPAC bases may be converted to N's depending on the parameter passed in
///  - everything else is considered a N
pub fn normalize(seq: &[u8]) -> Option<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::with_capacity(seq.len());
    let mut changed: bool = false;

    for n in seq.iter() {
        let (new_char, char_changed) = match (*n, false) {
            c @ (b'A', _) | c @ (b'C', _) | c @ (b'G', _) | c @ (b'T', _) => (c.0, false),
            (b'a', _) => (b'A', true),
            (b'c', _) => (b'C', true),
            (b'g', _) => (b'G', true),
            // normalize uridine to thymine
            (b't', _) | (b'u', _) | (b'U', _) => (b'T', true),
            // normalize gaps
            (b'.', _) | (b'~', _) => (b'-', true),
            // remove all whitespace and line endings
            (b' ', _) | (b'\t', _) | (b'\r', _) | (b'\n', _) => (b' ', true),
            // everything else is an N
            _ => (b' ', true),
        };
        changed = changed || char_changed;
        if new_char != b' ' {
            buf.push(new_char);
        }
    }
    if changed {
        Some(buf)
    } else {
        // if return None will cause panic.
        // Example very short siRNA without line breaks in genecode transcript.
        Some(buf)
    }
}

/// hold a kmer
pub struct Kmer {
    /// The 9mer sequence
    pub sequence: String,
    /// The value for the signal of that 9mer
    pub value: f64,
    /// std_level
    pub std_level: f64
}

/// Profile for sequencing
pub struct R10Settings {
    /// Digitisation to i16 I dunno
    digitisation: f64,
    /// range
    range: f64,
    /// sampling
    sampling: i16,
    /// kmer
    kmer_len: i16,
    /// noise
    noise: bool,
    /// 3to5
    direction: bool
}

/// Simulation type - Promethion or MInion. We always use Promethion
#[derive(Clone)]
pub enum SimType {
    /// R10
    DNAR1041,
    /// R10
    RNAR94,
}

// const PREFIX: &str =
//     "TTTTTTTTTTTTTTTTTTAATCAAGCAGCGGAGTTGAGGACGCGAGACGGGACTTTTTTAGCAGACTTTACGGACTACGACT";
const RANDOM_CHARS: [char; 4] = ['A', 'C', 'G', 'T'];

/// return the simulation profile for a given simulation type
pub fn get_sim_profile(sim_type: SimType) -> R10Settings {
    match sim_type {
        SimType::DNAR1041 => R10Settings {
            digitisation: 2048.0,
            range: 200.0,
            sampling: 10,
            kmer_len: 9,
            noise: false,
            direction: false
        },
        SimType::RNAR94 => R10Settings {
            digitisation: 2048.0,
            range: 200.0,
            sampling: 43,
            kmer_len: 5,
            noise: true,
            direction: true
        },
    }
}

///
pub fn get_is_rna(sim_type: SimType) -> bool {
    match sim_type {
        SimType::DNAR1041 => false,
        SimType::RNAR94 => true,
    }
}

/// Use nom to parse an individual line in the kmers file
fn parse_kmer_record(input: &str) -> IResult<&str, Kmer> {
    let (remaining, (kmer, _, value, _, std_level)) =
    terminated(
        tuple((
            alpha1,
            tab,
            double,
            tab,
            double,
        )),
        multispace0
    )(input)?;

    let kmer = kmer.to_string();
    Ok((
        remaining,
        Kmer {
            sequence: kmer,
            value,
            std_level
        },
    ))
}

/// Parse kmers and corresponding values from the file
pub fn parse_kmers(input: &str) -> IResult<&str, FnvHashMap<String, Vec<f64>>> {
    let (remainder, kmer_vec) = many1(parse_kmer_record)(input)?;
    let mut kmers: HashMap<String, Vec<f64>, std::hash::BuildHasherDefault<fnv::FnvHasher>> =
        FnvHashMap::default();
    for x in kmer_vec {
        kmers.insert(x.sequence, vec![x.value, x.std_level]);
    }
    Ok((remainder, kmers))
}

/// Generate signal for a stall sequence and adpator DNA
pub fn generate_prefix() -> Result<Vec<i16>, Box<dyn Error>> {
    let file = File::open("static/prefix.squiggle.npy").unwrap();
    let mmap = unsafe { Mmap::map(&file).unwrap() };
    let view: Vec<i16> = ArrayView1::<i16>::view_npy(&mmap).unwrap().to_vec();
    Ok(view)
}

/// Replace all occurences of a character in a string with a randomly chosen A,C,G, or T.
///  Used to remove Ns from reference.
/// If char to replace is None, We actually replace anything that is not a base
fn replace_char_with_base(string: &str, char_to_replace: Option<char>) -> String {
    let mut rng: rand::rngs::ThreadRng = rand::thread_rng();

    let replaced_string: String = string
        .chars()
        .map(|c| {
            if !HASHSET.contains(&c) {
                *RANDOM_CHARS.choose(&mut rng).unwrap()
            } else {
                c
            }
        })
        .collect();

    replaced_string
}

/// Return the given number of sequences contained in a provided Fasta/Fastq file. File can be gzipped,
/// will error if not a FASTQ/FASTA file.
pub fn num_sequences<P: AsRef<Path> + std::fmt::Debug>(path: P) -> usize {
    let mut reader: Box<dyn FastxReader> =
        parse_fastx_file(&path).unwrap_or_else(|_| panic!("Can't find FASTA file at {path:#?}"));
    let mut num_seq: usize = 0;
    while reader.next().is_some() {
        num_seq += 1;
    }
    num_seq
}

/// Get the lengths of all contigs in a given FASTA/FASTQ file
pub fn sequence_lengths<P: AsRef<Path> + std::fmt::Debug>(path: P) -> Vec<usize> {
    let mut read_lengths = vec![];
    let mut reader: Box<dyn FastxReader> =
        parse_fastx_file(&path).unwrap_or_else(|_| panic!("Can't find FASTA file at {path:#?}"));
    while let Some(record) = reader.next() {
        read_lengths.push(record.unwrap().num_bases())
    }
    read_lengths
}

/// Convert a given FASTA sequence to signal, digitising it and return a Vector of I16
pub fn convert_to_signal<'a>(
    kmers: &FnvHashMap<String, Vec<f64>>,
    record: &SequenceRecord,
    profile: &R10Settings
) -> Result<Vec<i16>, Box<dyn Error>> {
    let sampling = profile.sampling;
    let kmer_len = profile.kmer_len;
    let direction = profile.direction;
    let mut signal_vec: Vec<i16> = Vec::with_capacity(record.num_bases() * sampling as usize);
    warn!("{:?}", record.num_bases() * sampling as usize);
    let r: Cow<'a, [u8]> = normalize(record.sequence()).unwrap().into();
    let num_kmers: usize = r.len() - (kmer_len as usize);
    warn!("{:?}", r.len());
    warn!("{:?}", kmer_len as usize - 1);
    let sty = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");
    let pb: ProgressBar = ProgressBar::new(num_kmers.try_into().unwrap());
    pb.set_style(sty);
    for kmer in r.kmers(kmer_len as u8) {
        // warn!("{:?}", kmer);
        let mut kmer = String::from_utf8(kmer.to_vec()).unwrap();
        kmer = replace_char_with_base(&kmer, None);
        // debug!("{kmer}");
        let value = kmers.get(&kmer.to_uppercase()).unwrap_or_else(|| {
            panic!(
                "failed to retrieve value for kmer {kmer}, on contig{}",
                String::from_utf8(record.id().to_vec()).unwrap()
            )
        });
        let mut rng = StdRng::seed_from_u64(123);

        // N sampling for each base (sample_rate / bases per second )
        // This could also be worked out from profile.dwell_mean
        // Iterate and push samples into the signal_vec
        for _ in 0..sampling {
            let gauss: f64 = rng.sample(Normal::new(0.0, 1.0).unwrap());
            let mut x = 0.0;
            if profile.noise {
                // Generate a random number using the normal distribution
                // Apply noise to the value;
                let value_with_noise = (gauss * value[1] * 1.0) + value[0];
    
                // Calculate the final value based on the profile
                x = (value_with_noise * profile.digitisation) / profile.range;
            } else {
                // Calculate the value without noise based on the profile
                x = (value[0] * profile.digitisation) / profile.range;
            }

            signal_vec.push(x as i16);
        }
        pb.inc(1);
    }
    pb.finish_with_message("done");
    signal_vec.reverse();

    Ok(signal_vec)
}

// read_tag, u32
// read_id
// raw_data
// daq_offset
