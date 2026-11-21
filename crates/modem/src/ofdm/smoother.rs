use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::{
    complex::dot,
    linalg::{Cholesky, LinalgError},
};

use super::subcarrier::SubcarrierMap;

/// The least diagonal loading, as a noise-to-signal ratio, so a clean training never solves an
/// ill-conditioned system.
const MIN_TRAINING_NSR: f64 = 1e-4;

#[derive(Clone, Debug)]
pub struct DelaySmoother {
    taps: usize,
    basis: Vec<Complex<f32>>,
    basis_conj_t: Vec<Complex<f32>>,
    gram: Vec<Complex<f32>>,
    loaded: Vec<Complex<f32>>,
    h_occ: Vec<Complex<f32>>,
    response: Vec<Complex<f32>>,
    solver: Cholesky,
}

impl DelaySmoother {
    pub fn new(map: &SubcarrierMap, earliest: isize, taps: usize) -> Result<Self, LinalgError> {
        let fft = map.fft() as f64;
        let occupied = map.occupied().len();
        let basis: Vec<Complex<f32>> = map
            .occupied()
            .iter()
            .flat_map(|c| {
                (0..taps).map(move |tap| {
                    let delay = (earliest + tap as isize) as f64;
                    let phase = -TAU * f64::from(c.offset) * delay / fft;
                    Complex::new(phase.cos() as f32, phase.sin() as f32)
                })
            })
            .collect();
        let mut basis_conj_t = vec![Complex::new(0.0, 0.0); taps * occupied];
        for (k, row) in basis.chunks_exact(taps).enumerate() {
            for (t, &a) in row.iter().enumerate() {
                basis_conj_t[t * occupied + k] = a.conj();
            }
        }
        let mut gram = vec![Complex::new(0.0, 0.0); taps * taps];
        for row in basis.chunks_exact(taps) {
            for (i, cell) in gram.chunks_exact_mut(taps).enumerate() {
                for (slot, &value) in cell.iter_mut().zip(row) {
                    *slot += row[i].conj() * value;
                }
            }
        }
        Ok(Self {
            taps,
            loaded: gram.clone(),
            gram,
            basis,
            basis_conj_t,
            h_occ: vec![Complex::new(0.0, 0.0); occupied],
            response: vec![Complex::new(0.0, 0.0); taps],
            solver: Cholesky::new(taps)?,
        })
    }

    #[must_use]
    pub fn taps(&self) -> usize {
        self.taps
    }

    pub fn smooth(&mut self, map: &SubcarrierMap, h: &mut [Complex<f32>], estimate_var: f64) {
        let occupied = map.occupied();
        let mut sum_sq = 0.0f64;
        for (slot, c) in self.h_occ.iter_mut().zip(occupied) {
            let v = h[c.bin];
            *slot = v;
            sum_sq += f64::from(v.norm_sqr());
        }
        let power = sum_sq / occupied.len().max(1) as f64;
        let signal = (power - estimate_var).max(estimate_var);
        let ratio = (estimate_var / signal).max(MIN_TRAINING_NSR);
        if !ratio.is_finite() {
            return;
        }
        let loading = (self.taps as f64 * ratio) as f32;
        self.loaded.copy_from_slice(&self.gram);
        for diagonal in self.loaded.iter_mut().step_by(self.taps + 1) {
            diagonal.re += loading;
        }
        if self.solver.factor(&self.loaded).is_err() {
            return;
        }
        let occ = occupied.len();
        for (slot, row) in self
            .response
            .iter_mut()
            .zip(self.basis_conj_t.chunks_exact(occ))
        {
            *slot = dot(row, &self.h_occ);
        }
        if self.solver.solve(&mut self.response).is_err() {
            return;
        }
        for (c, row) in occupied.iter().zip(self.basis.chunks_exact(self.taps)) {
            h[c.bin] = dot(row, &self.response);
        }
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::rng::Rng;

    use super::*;
    use crate::ofdm::OfdmParams;

    fn map() -> SubcarrierMap {
        OfdmParams::wifi_like().map().clone()
    }

    fn multipath(map: &SubcarrierMap, paths: &[(f64, Complex<f64>)]) -> Vec<Complex<f32>> {
        let mut h = vec![Complex::new(0.0, 0.0); map.fft()];
        for c in map.occupied() {
            let value: Complex<f64> = paths
                .iter()
                .map(|&(delay, gain)| {
                    gain * Complex::from_polar(1.0, -TAU * f64::from(c.offset) * delay / 64.0)
                })
                .sum();
            h[c.bin] = Complex::new(value.re as f32, value.im as f32);
        }
        h
    }

    #[test]
    fn the_smoother_keeps_a_channel_inside_its_delay_support() {
        let map = map();
        let mut smoother = DelaySmoother::new(&map, -4, 25).unwrap();
        let paths = [
            (0.0, Complex::new(0.9, 0.1)),
            (3.0, Complex::new(-0.3, 0.2)),
            (11.0, Complex::new(0.1, -0.15)),
            (-2.0, Complex::new(0.05, 0.05)),
        ];
        let truth = multipath(&map, &paths);
        let mut h = truth.clone();
        smoother.smooth(&map, &mut h, 1e-9);
        for c in map.occupied() {
            let error = (h[c.bin] - truth[c.bin]).norm();
            assert!(error < 2e-3, "offset {}: error {error}", c.offset);
        }
    }

    #[test]
    fn the_smoother_keeps_only_the_noise_inside_its_delay_support() {
        let map = map();
        let taps = 25;
        let mut smoother = DelaySmoother::new(&map, -4, taps).unwrap();
        let truth = multipath(&map, &[(1.0, Complex::new(0.7, -0.7))]);
        let estimate_var = 0.02f64;
        let mut rng = Rng::mixed(0x5eed);
        let (mut raw, mut smoothed, mut count) = (0.0, 0.0, 0.0);
        for _ in 0..400 {
            let mut h = truth.clone();
            for c in map.occupied() {
                let sigma = (estimate_var / 2.0).sqrt();
                h[c.bin] +=
                    Complex::new((rng.normal() * sigma) as f32, (rng.normal() * sigma) as f32);
                raw += f64::from((h[c.bin] - truth[c.bin]).norm_sqr());
            }
            smoother.smooth(&map, &mut h, estimate_var);
            for c in map.occupied() {
                smoothed += f64::from((h[c.bin] - truth[c.bin]).norm_sqr());
                count += 1.0;
            }
        }
        let kept = taps as f64 / map.occupied().len() as f64;
        assert!((raw / count / estimate_var - 1.0).abs() < 0.05);
        let ratio = smoothed / raw;
        assert!(
            (ratio / kept - 1.0).abs() < 0.2,
            "smoothed {ratio} of the noise, the delay support predicts {kept}"
        );
    }
}
