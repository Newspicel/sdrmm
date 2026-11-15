mod bank;
mod forward_backward;
mod hermitian;
mod loading;
mod phase_mode;
mod sample;
mod smoothing;
#[cfg(test)]
mod testing;

use crate::math::linalg::LinalgError;
use crate::math::special::SpecialError;

pub use bank::CovarianceBank;
pub use forward_backward::forward_backward;
pub use loading::load_diagonal;
pub use phase_mode::{BESSEL_FLOOR, PhaseMode};
pub use sample::SampleCovariance;
pub use smoothing::{smooth, smooth_diagonal};

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum CovarianceError {
    #[error(transparent)]
    Linalg(#[from] LinalgError),
    #[error(transparent)]
    Special(#[from] SpecialError),
    #[error("needs a line or circle")]
    NotStructured,
    #[error("FB needs a symmetric array")]
    NotSymmetric,
    #[error("too few elements for smoothing")]
    TooFewForSmoothing,
    #[error("Bessel null here")]
    BesselNull,
    #[error("fft size {0} is not a power of two in 64..=8192")]
    FftSize(usize),
    #[error("hop {0} is outside 1..=fft size")]
    Hop(usize),
    #[error("bins {0}..{1} are outside the bank")]
    Bins(usize, usize),
    #[error("lanes differ in length")]
    LaneLength,
}
