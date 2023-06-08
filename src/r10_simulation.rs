//! Defines code used to create the R10 signal from pore models.

use fnv::{FnvHashMap, FnvHashSet};
use lazy_static::lazy_static;
use needletail::parser::SequenceRecord;
use needletail::Sequence;
use nom::character::complete::{alpha1, multispace0, tab};
use nom::multi::many1;
use nom::number::complete::double;
use nom::sequence::{separated_pair, terminated};
use nom::IResult;
use rand::seq::SliceRandom;
use std::collections::HashMap;
use std::error::Error;
use std::fs::read_to_string;

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

/// hold a kmer
pub struct Kmer {
    /// The 9mer sequence
    pub sequence: String,
    /// The value for the signal of that 9mer
    pub value: f64,
}

/// Profile for sequencing
pub struct R10Settings {
    /// Digitisation to i16 I dunno
    digitisation: f64,
    /// range
    range: f64,
}

/// Simulation type - Promethion or MInion. We always use Promethion
pub enum SimType {
    /// R10
    R10,
}
const PREFIX: &str =
    "TTTTTTTTTTTTTTTTTTAATCAAGCAGCGGAGTTGAGGACGCGAGACGGGACTTTTTTAGCAGACTTTACGGACTACGACT";
const RANDOM_CHARS: [char; 4] = ['A', 'C', 'G', 'T'];

/// return the simulation profile for a given simulation type
pub fn get_sim_profile(sim_type: SimType) -> R10Settings {
    match sim_type {
        SimType::R10 => R10Settings {
            digitisation: 2048.0,
            range: 200.0,
        },
    }
}

/// Use nom to parse an individual line in the kmers file
pub fn parse_kmer_record(input: &str) -> IResult<&str, Kmer> {
    let (remaining, (kmer, value)) =
        separated_pair(alpha1, tab, terminated(double, multispace0))(input)?;
    let kmer = kmer.to_string();
    Ok((
        remaining,
        Kmer {
            sequence: kmer,
            value,
        },
    ))
}

/// Parse kmers and corresponding values from the file
pub fn parse_kmers(input: &str) -> IResult<&str, FnvHashMap<String, f64>> {
    let (remainder, kmer_vec) = many1(parse_kmer_record)(input)?;
    let mut kmers: HashMap<String, f64, std::hash::BuildHasherDefault<fnv::FnvHasher>> =
        FnvHashMap::default();
    for x in kmer_vec {
        kmers.insert(x.sequence, x.value);
    }
    Ok((remainder, kmers))
}

/// Generate signal for a stall sequence and adpator DNA
pub fn generate_prefix() -> Result<Vec<i16>, Box<dyn Error>> {
    let kmer_string =
        read_to_string("static/r10_squig_model.tsv").expect("Failed to read kmers to string");
    let (_, kmer_hashmap) = parse_kmers(&kmer_string).expect("Failed to parse R10 kmers");
    let profile = get_sim_profile(SimType::R10);
    let mut prefix_signal = Vec::with_capacity(2000);
    let mut prefix = String::new();
    prefix.push_str(PREFIX);
    for i in 0..prefix.len() - 8 {
        let value = kmer_hashmap.get(&prefix[i..i + 9]).unwrap();
        let x = (value * profile.digitisation) / profile.range;
        for _ in 0..10 {
            prefix_signal.push(x as i16);
        }
    }
    prefix_signal.shrink_to_fit();
    Ok(prefix_signal)
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

/// Convert a given FASTA sequence to signal, digitising it and return a Vector of I16
pub fn convert_to_signal(
    kmers: &FnvHashMap<String, f64>,
    record: &SequenceRecord,
    profile: &R10Settings,
) -> Result<Vec<i16>, Box<dyn Error>> {
    let mut signal_vec: Vec<i16> = Vec::with_capacity(record.num_bases() * 10);
    let seq = record.seq();
    let num_kmers = seq.len() - 8;
    for kmer in record.kmers(9) {
        let mut kmer = String::from_utf8(kmer.to_vec()).unwrap();
        kmer = replace_char_with_base(&kmer, None);
        debug!("{kmer}");
        let value = kmers.get(&kmer.to_uppercase()).unwrap_or_else(|| {
            panic!(
                "failed to retrieve value for kmer {kmer}, on contig{}",
                String::from_utf8(record.id().to_vec()).unwrap()
            )
        });
        debug!("{value}");

        let x = (value * profile.digitisation) / profile.range;
        // 10 sample for each base (sample_rate (4000) / base per second (400))
        // could also be worked out from profile.dwell_mean
        for _ in 0..10 {
            signal_vec.push(x as i16);
        }
    }
    Ok(signal_vec)
}

// read_tag, u32
// read_id
// raw_data
// daq_offset
