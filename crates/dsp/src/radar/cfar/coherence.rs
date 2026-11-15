use num_complex::Complex;

use super::{LOOKS_TOLERANCE, MAX_LANES};
use crate::math::complex::widen;
use crate::radar::RadarDspError;

/// Counts the independent looks a set of surveillance lanes adds, from their coherence.
pub struct LaneCoherence {
    lanes: usize,
    samples: usize,
    cross: [[Complex<f64>; MAX_LANES]; MAX_LANES],
}

impl LaneCoherence {
    pub fn new(lanes: usize) -> Result<Self, RadarDspError> {
        if !(1..=MAX_LANES).contains(&lanes) {
            return Err(RadarDspError::Setting);
        }
        Ok(Self {
            lanes,
            samples: 0,
            cross: [[Complex::new(0.0, 0.0); MAX_LANES]; MAX_LANES],
        })
    }

    pub fn clear(&mut self) {
        self.samples = 0;
        self.cross = [[Complex::new(0.0, 0.0); MAX_LANES]; MAX_LANES];
    }

    pub fn add(&mut self, snapshot: &[Complex<f32>]) {
        self.samples += 1;
        let lanes = self.lanes.min(snapshot.len());
        for (k, first) in snapshot[..lanes].iter().enumerate() {
            let a = widen(*first);
            for (cell, second) in self.cross[k][k..lanes].iter_mut().zip(&snapshot[k..lanes]) {
                *cell += a * widen(*second).conj();
            }
        }
    }

    #[must_use]
    pub fn looks(&self) -> u32 {
        let lanes = self.lanes;
        let bias = 1.0 / self.samples.max(1) as f64;
        let mut frobenius = lanes as f64;
        for k in 0..lanes {
            for j in k + 1..lanes {
                let norm = self.cross[k][k].re * self.cross[j][j].re;
                if norm > 0.0 {
                    let squared = self.cross[k][j].norm_sqr() / norm;
                    frobenius += 2.0 * (squared - bias).max(0.0);
                }
            }
        }
        if !frobenius.is_finite() {
            return 1;
        }
        let ratio = (lanes * lanes) as f64 / frobenius;
        ((ratio + LOOKS_TOLERANCE).floor() as u32).clamp(1, lanes as u32)
    }
}
