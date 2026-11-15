use std::f64::consts::TAU;

use crate::math::window::hann_weight;

const BLACKMAN_HARRIS: [f64; 4] = [0.358_75, 0.488_29, 0.141_28, 0.011_68];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DopplerTaper {
    #[default]
    Hann,
    BlackmanHarris,
    Rectangular,
}

impl DopplerTaper {
    /// Equivalent noise bandwidth in bins.
    #[must_use]
    pub const fn enbw(self) -> f64 {
        match self {
            Self::Hann => 1.5,
            Self::BlackmanHarris => 2.0,
            Self::Rectangular => 1.0,
        }
    }

    pub fn fill(self, out: &mut [f32]) {
        let len = out.len();
        for (index, value) in out.iter_mut().enumerate() {
            *value = match self {
                Self::Hann => hann_weight(index, len),
                Self::BlackmanHarris => blackman_harris_weight(index, len),
                Self::Rectangular => 1.0,
            } as f32;
        }
    }
}

fn blackman_harris_weight(index: usize, len: usize) -> f64 {
    let [a0, a1, a2, a3] = BLACKMAN_HARRIS;
    let x = TAU * index as f64 / len as f64;
    a0 - a1 * x.cos() + a2 * (2.0 * x).cos() - a3 * (3.0 * x).cos()
}
