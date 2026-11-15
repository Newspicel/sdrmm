mod plan;
mod solver;
mod sums;
mod table;

use std::f64::consts::TAU;

use num_complex::Complex;

pub use plan::GroupPlan;
pub use solver::{SolveStats, WienerSolver};
pub use sums::GroupSums;
pub use table::{MixTerm, WeightTable};

pub const MAX_DOPPLER_TAPS: usize = 2;

fn rotation(turns: f64) -> Complex<f64> {
    Complex::from_polar(1.0, TAU * turns.rem_euclid(1.0))
}

#[cfg(test)]
mod tests;
