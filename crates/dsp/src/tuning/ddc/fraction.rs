use num_complex::Complex;

use super::{MIN_FILTERED_UPSAMPLING, STOPBAND_DB, is_unity};
use crate::{CubicInterpolator, FracResampler};

/// The rate change left after the integer stages.
#[derive(Clone, Debug)]
pub(super) enum Fraction {
    None,
    Resample(Box<FracResampler>),
    Interpolate(CubicInterpolator),
}

impl Fraction {
    pub(super) fn for_ratio(ratio: f64, keep: Option<f64>) -> Self {
        if is_unity(ratio) {
            Self::None
        } else if (1.0..MIN_FILTERED_UPSAMPLING).contains(&ratio) {
            Self::Interpolate(CubicInterpolator::new(ratio))
        } else {
            Self::Resample(Box::new(FracResampler::rejecting(ratio, keep, STOPBAND_DB)))
        }
    }

    pub(super) fn reset(&mut self) {
        match self {
            Self::None => {}
            Self::Resample(r) => r.reset(),
            Self::Interpolate(r) => r.reset(),
        }
    }

    pub(super) fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        match self {
            Self::None => {
                out.clear();
                out.extend_from_slice(input);
            }
            Self::Resample(r) => r.process(input, out),
            Self::Interpolate(r) => r.process(input, out),
        }
    }
}
