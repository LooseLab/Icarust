//! Reacquisition distribution manages the probability distributions for drawing reacquistion times
//! on a pore.
//!
//! Also contains the function for calculating the chance of a pore dying.
//!
//!

use ndarray::prelude::*;
use rand::prelude::*;
use std::ops::Neg;

use rand::seq::SliceRandom;
use std::f64;

//  Lifted from statrs https://docs.rs/statrs/latest/src/statrs/function/gamma.rs.html#1-808
const GAMMA_R: f64 = 10.900511;
const TWO_SQRT_E_OVER_PI: f64 = 1.8603827342052657173362492472666631120594218414085755;

/// Polynomial coefficients for approximating the `gamma_ln` function
const GAMMA_DK: &[f64] = &[
    2.48574089138753565546e-5,
    1.05142378581721974210,
    -3.45687097222016235469,
    4.51227709466894823700,
    -2.98285225323576655721,
    1.05639711577126713077,
    -1.95428773191645869583e-1,
    1.70970543404441224307e-2,
    -5.71926117404305781283e-4,
    4.63399473359905636708e-6,
    -2.71994908488607703910e-9,
];

/// calculate the gamme of a float, returning the gamma multiplied by the original float.
/// Equivalent to the factorial of a decimal point number.
///
/// Examples
///
/// ```
///  let x = gamma_fac(0.01)
///  assert!(x == 0.9943258511915062);
/// ```
fn gamma_fac(x: f64) -> f64 {
    if x < 0.0 {
        0.0
    } else if x == 0.0 {
        1.0
    } else if x < 0.5 {
        let s = GAMMA_DK
            .iter()
            .enumerate()
            .skip(1)
            .fold(GAMMA_DK[0], |s, t| s + t.1 / (t.0 as f64 - x));

        (f64::consts::PI
            / ((f64::consts::PI * x).sin()
                * s
                * TWO_SQRT_E_OVER_PI
                * ((0.5 - x + GAMMA_R) / f64::consts::E).powf(0.5 - x)))
            * x
    } else {
        let s = GAMMA_DK
            .iter()
            .enumerate()
            .skip(1)
            .fold(GAMMA_DK[0], |s, t| s + t.1 / (x + t.0 as f64 - 1.0));

        (s * TWO_SQRT_E_OVER_PI * ((x - 0.5 + GAMMA_R) / f64::consts::E).powf(x - 0.5)) * x
    }
}

/// Models the reacquisition time for a pore.
pub struct ReacquisitionPoisson {
    t: Vec<f64>,
}

pub trait SampleDist {
    fn sample<R>(&self, rng: &mut R) -> f64
    where
        R: Rng;
}

impl ReacquisitionPoisson {
    /// Return a new Reacquisition poisson.
    /// Upper is the maximum possible upper value
    /// Lower is the lowest possible value
    /// step is how tight the distribution curves
    /// k is the skew ( I think)
    pub fn new(upper: f64, lower: f64, step: f64, k: f64) -> ReacquisitionPoisson {
        let mut t = Array::range(lower, upper, step);
        t.mapv_inplace(|x| ((k.powf(x)) * k.neg().exp()) / gamma_fac(x));
        let t = t.to_vec();
        ReacquisitionPoisson { t }
    }
}

impl SampleDist for ReacquisitionPoisson {
    fn sample<R: rand::RngCore>(&self, rng: &mut R) -> f64 {
        self.t.choose(rng).unwrap().clone()
    }
}

/// Calculate the chance of a pore to die, providing that we need to reach a target yield given a mean read length.
pub fn _calculate_death_chance(
    starting_channels: f64,
    target_yield: f64,
    mean_read_length: f64,
) -> f64 {
    1.0_f64 / (target_yield as f64 / mean_read_length / starting_channels)
}

#[derive(Debug)]
pub struct DeathChance {
    pub base_chance: f64,
    pub mean_read_length: f64,
}
