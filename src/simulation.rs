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
use nom::character::complete::{alpha1, multispace0, tab};

use nom::multi::many1;
use nom::number::complete::double;
use nom::sequence::{separated_pair, terminated, tuple};
use nom::IResult;
use probability::prelude::*;
use rand::prelude::*;
use rand::seq::SliceRandom;
use rand_distr::Normal;
use rayon::prelude::*;
use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::fs::File;
use std::path::Path;
use std::sync::{Arc, Mutex};

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

    Some(buf)
}

/// hold a kmer
pub struct Kmer {
    /// The 9mer sequence
    pub sequence: String,
    /// The value for the signal of that 9mer
    pub value: f64,
    /// std_level
    pub std_level: Option<f64>,
}

/// Length of the kmer expected from the kmer model
pub enum KmerType {
    /// 5mer nucleotide record
    FiveMer,
    /// 9mer nucleotide record
    NineMer,
}

/// Profile for sequencing
pub struct SimSettings {
    /// Digitisation to i16 I dunno
    pub digitisation: u32,
    /// range
    pub range: f64,
    /// scale
    pub scale: f64,
    /// offset
    pub offset: f64,
    /// samples_per_base
    samples_per_base: i16,
    /// kmer
    kmer_len: i16,
    /// noise
    noise: bool,
    /// 3to5
    reverse: bool,
    /// Simulation type
    sim_type: SimType,
}

/// Simulation type - Promethion or MInion. We always use Promethion
#[derive(Clone, PartialEq)]
pub enum SimType {
    /// R10
    DNAR10,
    /// R9
    RNAR9,
    /// R9 DNA
    DNAR9,
}

// const PREFIX: &str =
//     "TTTTTTTTTTTTTTTTTTAATCAAGCAGCGGAGTTGAGGACGCGAGACGGGACTTTTTTAGCAGACTTTACGGACTACGACT";
const RANDOM_CHARS: [char; 4] = ['A', 'C', 'G', 'T'];

/// return the simulation profile for a given simulation type
pub fn get_sim_profile(sim_type: SimType) -> SimSettings {
    match sim_type {
        SimType::DNAR10 => SimSettings {
            digitisation: 2048,
            scale: 0.1462070643901825,
            range: 2048.0 * 0.1462070643901825, // Digitisation * scale
            offset: -243.0,
            samples_per_base: 12,
            kmer_len: 9,
            noise: true,
            reverse: false,
            sim_type: SimType::DNAR10,
        },
        SimType::RNAR9 => SimSettings {
            digitisation: 8192,
            scale: 1.0,
            range: 1158.63,
            offset: 0.0,
            samples_per_base: 43,
            kmer_len: 5,
            noise: true,
            reverse: true,
            sim_type: SimType::RNAR9,
        },
        _ => {
            unimplemented!()
        }
    }
}

///is it an RNA read
pub fn get_is_rna(sim_type: SimType) -> bool {
    matches!(sim_type, SimType::RNAR9)
}

/// Use nom to parse an individual line in the kmers file
fn parse_9mer_record(input: &str) -> IResult<&str, Kmer> {
    let (remaining, (kmer, value)) =
        separated_pair(alpha1, tab, terminated(double, multispace0))(input)?;
    let kmer = kmer.to_string();
    Ok((
        remaining,
        Kmer {
            sequence: kmer,
            value,
            std_level: None,
        },
    ))
}

/// Use nom to parse an individual line in the kmers file
fn parse_5mer_record(input: &str) -> IResult<&str, Kmer> {
    let (remaining, (kmer, _, value, _, std_level)) =
        terminated(tuple((alpha1, tab, double, tab, double)), multispace0)(input)?;

    let kmer = kmer.to_string();
    Ok((
        remaining,
        Kmer {
            sequence: kmer,
            value,
            std_level: Some(std_level),
        },
    ))
}
/// Parse kmers and corresponding values from the file
pub fn parse_kmers(
    input: &str,
    kmer_length: KmerType,
) -> IResult<&str, FnvHashMap<String, (f64, Option<f64>)>> {
    let (remainder, kmer_vec) = match kmer_length {
        KmerType::NineMer => many1(parse_9mer_record)(input)?,
        KmerType::FiveMer => many1(parse_5mer_record)(input)?,
    };
    let mut kmers: HashMap<
        String,
        (f64, Option<f64>),
        std::hash::BuildHasherDefault<fnv::FnvHasher>,
    > = FnvHashMap::default();
    for x in kmer_vec {
        kmers.insert(x.sequence, (x.value, x.std_level));
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
fn replace_char_with_base(string: &str, _char_to_replace: Option<char>) -> String {
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

/// Add laplace noise to each sample for a whole signal
fn add_laplace_noise<'a>(
    data: &mut f64,
    sampler: &mut Independent<&'a Laplace, &'a mut source::Default>,
) {
    let noise: f64 = sampler.next().unwrap();
    *data += noise;
}

/// Add Gaussian noise to each sample individual, for RNA
fn add_gaussian_noise(value: &mut f64, std_dev: f64, rng: &mut StdRng) {
    let normal = Normal::new(0.0, std_dev).unwrap();
    let noise: f64 = rng.sample(normal);
    *value += noise;
}

/// Convert a given FASTA sequence to signal, digitising it and return a Vector of I16
pub fn convert_to_signal<'a>(
    kmers: &FnvHashMap<String, (f64, Option<f64>)>,
    record: &SequenceRecord,
    profile: &SimSettings,
) -> Result<Vec<i16>, Box<dyn Error>> {
    let samples_per_base = profile.samples_per_base;
    let kmer_len = profile.kmer_len;
    println!("{}", record.num_bases() * samples_per_base as usize);
    let signal_vec = Arc::new(Mutex::new(Vec::with_capacity(
        record.num_bases() * samples_per_base as usize,
    )));
    let r: Cow<'a, [u8]> = normalize(record.sequence()).unwrap().into();
    let num_kmers: usize = r.len() - (kmer_len as usize);
    let sty = ProgressStyle::with_template(
        "[{elapsed_precise}] {bar:40.cyan/blue} {pos:>7}/{len:7} {msg}",
    )
    .unwrap()
    .progress_chars("##-");
    let pb: ProgressBar = ProgressBar::new(num_kmers.try_into().unwrap());
    pb.set_style(sty);
    let laplace: Laplace = Laplace::new(0.0, 1.0 / 2.0f64.sqrt());
    let mut source = source::default(42);
    let sampler = Arc::new(Mutex::new(Independent(&laplace, &mut source)));
    let rng = Arc::new(Mutex::new(StdRng::seed_from_u64(123)));
    r.kmers(kmer_len as u8).par_bridge().for_each(|kmer| {
        let mut kmer = String::from_utf8(kmer.to_vec()).unwrap();
        kmer = replace_char_with_base(&kmer, None);
        let kmer_upper = kmer.to_uppercase();
        let value = kmers.get(&kmer_upper).unwrap_or_else(|| {
            panic!(
                "failed to retrieve value for kmer {}, on contig {}",
                kmer,
                String::from_utf8(record.id().to_vec()).unwrap()
            )
        });

        let mut local_signal_vec = vec![];
        for _ in 0..samples_per_base {
            let mut x = value.0;
            if profile.noise & (profile.sim_type == SimType::RNAR9) {
                let mut rng = rng.lock().unwrap();
                add_gaussian_noise(&mut x, value.1.unwrap(), &mut rng)
            }
            if profile.noise & (profile.sim_type == SimType::DNAR10) {
                let mut sampler = sampler.lock().unwrap();
                add_laplace_noise(&mut x, &mut sampler);
            }
            local_signal_vec.push(x);
        }

        let mut signal_vec = signal_vec.lock().unwrap();
        signal_vec.extend(local_signal_vec);
        pb.inc(1);
    });
    let mut signal_vec = signal_vec.lock().unwrap();

    let mut signal_vec: Vec<i16> = signal_vec
        .par_iter_mut()
        .map(|x| ((*x / profile.scale) - profile.offset) as i16)
        .collect();
    signal_vec.shrink_to_fit();
    if profile.reverse {
        signal_vec.reverse();
    }
    pb.finish_with_message("done");
    Ok(signal_vec)
}
