mod adaptive;
mod cma;
mod constraints;
mod gsc;
mod lane_noise;
mod lanes;
mod metrics;
mod pattern;
mod ramp;
mod tdl;
#[cfg(test)]
pub(crate) mod testing;
mod weight_set;
mod weights;

use num_complex::Complex;

use crate::math::linalg::LinalgError;

pub use cma::Cma;
pub use constraints::Constraints;
pub use gsc::Gsc;
pub use lane_noise::{LaneNoise, NOISE_FRAME};
pub use metrics::{BeamMetrics, beam_metrics};
pub use pattern::pattern;
pub use ramp::WeightRamp;
pub use tdl::{Adaptation, TDL_MAX_CMAC_PER_S, TDL_MAX_TAPS, TdlCanceller, tdl_cmac_per_sample};
pub use weight_set::WeightSet;
pub use weights::BlockSolver;

pub const MAX_CONSTRAINTS: usize = 4;
pub const NULL_OVERLAP_LIMIT: f32 = 0.7;

const ZERO: Complex<f32> = Complex::new(0.0, 0.0);

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum BeamError {
    #[error(transparent)]
    Linalg(#[from] LinalgError),
    #[error("null {0} sits in the main lobe")]
    NullInMainLobe(usize),
    #[error("at most {0} constraints fit this array")]
    TooManyConstraints(usize),
    #[error("weights diverged")]
    Diverged,
    #[error("{0} lanes do not fit")]
    Lanes(usize),
    #[error("lanes differ in length")]
    LaneLength,
    #[error("{0} out of range")]
    Setting(&'static str),
}
