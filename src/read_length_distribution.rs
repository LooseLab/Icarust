use crate::reacquisition_distribution::SampleDist;
// use rand_distr::{Poisson, Distribution};
use rand::prelude::*;
use std::cmp::Ordering;

use rand::seq::SliceRandom;
use rand_distr::{Distribution, FisherF};

const LOGLOG_SLOPE: f64 = -2.211897875506251;
const LOGLOG_INTERCEPT: f64 = 1.002555437670879;
const DF2: f64 = 500.0;

const K1_SLOPE: f64 = 0.5670830069364579;
const K1_INTERCEPT: f64 = -0.09239985798819927;
const K2_SLOPE: f64 = 0.5823114978219056;
const K2_INTERCEPT: f64 = -0.11748300123471256;

#[derive(Debug)]
pub struct ReadLengthDist {
    dist: Vec<f64>,
}

impl ReadLengthDist {
    pub fn new(mean_read_length: f64) -> ReadLengthDist {
        let dist = _create_skew_dist(mean_read_length, mean_read_length / 2.0_f64, 1.25, 10000.0);
        ReadLengthDist { dist }
    }
}

impl SampleDist for ReadLengthDist {
    fn sample<R>(&self, rng: &mut R) -> f64
    where
        R: Rng,
    {
        self.dist.choose(rng).unwrap().clone()
    }
}

fn _create_skew_dist(mean: f64, sd: f64, skew: f64, size: f64) -> Vec<f64> {
    let mut rng = thread_rng();

    let df1 = 10.0_f64.powf((LOGLOG_SLOPE * skew.abs().log10()) + LOGLOG_INTERCEPT);
    let f = FisherF::new(df1, DF2).unwrap();
    let mut fsample: Vec<f64> = f.sample_iter(&mut rng).take(size as usize).collect();
    fsample.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));
    let scaling_slope: f64 = skew.abs() * K1_SLOPE + K1_INTERCEPT;
    let scaling_intercept: f64 = skew.abs() * K2_SLOPE + K2_INTERCEPT;
    let scale_factor: f64 = (sd - scaling_intercept) / scaling_slope;
    let fsample_mean: f64 = fsample.iter().sum::<f64>() as f64 / fsample.len() as f64;
    fsample
        .iter_mut()
        .for_each(|i| *i = ((*i - fsample_mean) * scale_factor) + *i);
    if skew < 0.0_f64 {
        let fsample_mean: f64 = fsample.iter().sum::<f64>() as f64 / fsample.len() as f64;
        fsample.iter_mut().for_each(|i| *i = fsample_mean - *i);
    }
    let fsample_mean: f64 = fsample.iter().sum::<f64>() as f64 / fsample.len() as f64;
    fsample.iter_mut().for_each(|i| *i += (mean + fsample_mean));
    fsample
}
