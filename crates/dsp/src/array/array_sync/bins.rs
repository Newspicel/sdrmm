mod lane;
mod residual;

use std::cmp::Ordering;

use num_complex::Complex;

use super::{
    EQ_POINTS,
    eigen::{POWER_ITERATIONS, dominant},
    frequency::bin_frequency,
};
use crate::{math::fft::FftPair, math::window::hann};

pub const FIT_BAND: f64 = 0.4;
pub const MIN_BINS: usize = 64;
pub const NOISE_XCORR_MIN: f32 = 0.5;
pub const NOISE_PURITY_MIN: f32 = 0.8;
pub const NOISE_COHERENCE_MIN: f32 = 0.5;
pub const NOISE_CLIPPED_MAX: f32 = 0.75;

const REFERENCE_SHARE: f32 = 0.05;

#[derive(Clone, Debug, PartialEq)]
pub struct LaneResponse {
    pub delay_frac: f32,
    pub phase_rad: f32,
    pub gain: f32,
    pub coherence: f32,
    pub equaliser: Vec<Complex<f32>>,
}

impl LaneResponse {
    fn reference() -> Self {
        Self {
            delay_frac: 0.0,
            phase_rad: 0.0,
            gain: 1.0,
            coherence: 1.0,
            equaliser: vec![Complex::new(1.0, 0.0); EQ_POINTS],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BinSolution {
    pub lanes: Vec<LaneResponse>,
    pub purity: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum BinError {
    #[error("lane {lane} coherence {coherence}")]
    LowCoherence { lane: usize, coherence: f32 },
    #[error("too few usable bins")]
    FewBins,
    #[error("capture too short")]
    Short,
    #[error("expected {expected} lanes, got {got}")]
    LaneCount { expected: usize, got: usize },
}

#[derive(Clone, Copy, Debug, Default)]
struct BinFit {
    lambda: f32,
    trace: f32,
    used: bool,
}

pub struct BinSolver {
    fft: FftPair,
    len: usize,
    window: Vec<f32>,
    spectra: Vec<Vec<Complex<f32>>>,
    cov: Vec<Complex<f32>>,
    lanes: usize,
    fits: Vec<BinFit>,
    responses: Vec<Complex<f32>>,
    phase: Vec<f64>,
    residual: Vec<Complex<f32>>,
    smoothed: Vec<Complex<f32>>,
}

impl BinSolver {
    #[must_use]
    pub fn new(lanes: usize, len: usize) -> Self {
        let len = len.max(2);
        Self {
            fft: FftPair::new(len),
            len,
            window: hann(len),
            spectra: vec![vec![Complex::default(); len]; lanes],
            cov: vec![Complex::default(); len * lanes * lanes],
            lanes,
            fits: vec![BinFit::default(); len],
            responses: vec![Complex::default(); len * lanes],
            phase: vec![0.0; len],
            residual: vec![Complex::default(); len],
            smoothed: vec![Complex::default(); len],
        }
    }

    #[must_use]
    pub const fn lanes(&self) -> usize {
        self.lanes
    }

    #[must_use]
    pub const fn bins(&self) -> usize {
        self.len
    }

    pub fn solve(
        &mut self,
        lanes: &[&[Complex<f32>]],
        purity_min: f32,
        coherence_min: f32,
    ) -> Result<BinSolution, BinError> {
        if lanes.len() != self.lanes {
            return Err(BinError::LaneCount {
                expected: self.lanes,
                got: lanes.len(),
            });
        }
        let samples = lanes.iter().map(|lane| lane.len()).min().unwrap_or(0);
        if samples < self.len {
            return Err(BinError::Short);
        }
        self.accumulate(lanes, samples);
        self.decompose(purity_min);
        if self.fits.iter().filter(|fit| fit.used).count() < MIN_BINS {
            return Err(BinError::FewBins);
        }
        let mut responses = Vec::with_capacity(self.lanes);
        responses.push(LaneResponse::reference());
        for lane in 1..self.lanes {
            responses.push(self.lane_response(lane));
        }
        let weakest = responses
            .iter()
            .enumerate()
            .skip(1)
            .filter(|(_, response)| {
                matches!(
                    response.coherence.partial_cmp(&coherence_min),
                    None | Some(Ordering::Less)
                )
            })
            .min_by(|a, b| a.1.coherence.total_cmp(&b.1.coherence));
        if let Some((lane, response)) = weakest {
            return Err(BinError::LowCoherence {
                lane,
                coherence: response.coherence,
            });
        }
        Ok(BinSolution {
            lanes: responses,
            purity: self.purity(),
        })
    }

    fn accumulate(&mut self, lanes: &[&[Complex<f32>]], samples: usize) {
        let order = self.lanes;
        let hop = self.len / 2;
        let blocks = (samples - self.len) / hop + 1;
        self.cov.fill(Complex::default());
        for block in 0..blocks {
            let start = block * hop;
            for (spectrum, lane) in self.spectra.iter_mut().zip(lanes) {
                for ((value, sample), weight) in spectrum
                    .iter_mut()
                    .zip(&lane[start..start + self.len])
                    .zip(&self.window)
                {
                    *value = sample * weight;
                }
                self.fft.forward(spectrum);
            }
            for (bin, matrix) in self.cov.chunks_exact_mut(order * order).enumerate() {
                for (row, cells) in matrix.chunks_exact_mut(order).enumerate() {
                    let left = self.spectra[row][bin];
                    for (cell, right) in cells.iter_mut().zip(&self.spectra) {
                        *cell += left * right[bin].conj();
                    }
                }
            }
        }
        let scale = 1.0 / blocks as f32;
        for cell in &mut self.cov {
            *cell *= scale;
        }
    }

    fn decompose(&mut self, purity_min: f32) {
        let order = self.lanes;
        let len = self.len;
        for (bin, ((matrix, vector), fit)) in self
            .cov
            .chunks_exact(order * order)
            .zip(self.responses.chunks_exact_mut(order))
            .zip(&mut self.fits)
            .enumerate()
        {
            let lambda = dominant(matrix, order, POWER_ITERATIONS, vector);
            let trace: f32 = (0..order).map(|lane| matrix[lane * order + lane].re).sum();
            let reference = vector[0];
            let usable = trace > 0.0
                && lambda > 0.0
                && reference.norm_sqr() * order as f32 >= REFERENCE_SHARE;
            if usable {
                for value in vector.iter_mut() {
                    *value /= reference;
                }
            }
            let purity = if usable { lambda / trace } else { 0.0 };
            *fit = BinFit {
                lambda,
                trace,
                used: usable && bin_frequency(bin, len).abs() <= FIT_BAND && purity >= purity_min,
            };
        }
    }

    fn purity(&self) -> f32 {
        let (lambda, trace) = self
            .fits
            .iter()
            .filter(|fit| fit.used)
            .fold((0.0f64, 0.0f64), |(lambda, trace), fit| {
                (lambda + f64::from(fit.lambda), trace + f64::from(fit.trace))
            });
        if trace > 0.0 {
            (lambda / trace) as f32
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::TAU;

    use super::*;
    use crate::{
        array::array_sync::{
            equaliser_frequency,
            signals::{Gaussian, lane_response, shaped, with_noise},
        },
        math::db::Decibels,
        math::special::wrap_rad,
    };

    const SOLVE_LEN: usize = 65_536;
    const BINS: usize = 1_024;

    fn lanes_from(
        source: &[Complex<f32>],
        responses: &[(f64, f64, f64)],
        noise: f32,
        seed: u64,
    ) -> Vec<Vec<Complex<f32>>> {
        let mut generator = Gaussian::new(seed);
        let mut lanes = vec![with_noise(source, noise, &mut generator)];
        for &(delay, phase_deg, gain_db) in responses {
            let response = lane_response(delay, phase_deg.to_radians(), gain_db.db_to_amplitude());
            lanes.push(with_noise(&shaped(source, response), noise, &mut generator));
        }
        lanes
    }

    fn solve(
        lanes: &[Vec<Complex<f32>>],
        purity_min: f32,
        coherence_min: f32,
    ) -> Result<BinSolution, BinError> {
        let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        BinSolver::new(lanes.len(), BINS).solve(&views, purity_min, coherence_min)
    }

    fn assert_close(response: &LaneResponse, (delay, phase_deg, gain_db): (f64, f64, f64)) {
        let delay_error = f64::from(response.delay_frac) - delay;
        let phase_error =
            wrap_rad(f64::from(response.phase_rad) - phase_deg.to_radians()).to_degrees();
        let gain_error = f64::from(response.gain).amplitude_to_db() - gain_db;
        assert!(delay_error.abs() < 0.01, "delay {delay}: {delay_error}");
        assert!(phase_error.abs() < 0.5, "phase {phase_deg}: {phase_error}");
        assert!(gain_error.abs() < 0.05, "gain {gain_db}: {gain_error}");
    }

    #[test]
    fn bin_solver_recovers_delay_phase_and_gain() {
        let truth = [(0.37, 73.0, -2.5), (-0.21, -140.0, 1.5), (0.05, 12.0, 0.0)];
        let source = Gaussian::new(1).block(SOLVE_LEN);
        let lanes = lanes_from(&source, &truth, 0.03, 2);
        let solution = solve(&lanes, 0.8, 0.9).unwrap();
        assert_eq!(solution.lanes.len(), 4);
        assert!(solution.purity > 0.99, "{}", solution.purity);
        assert_eq!(solution.lanes[0], LaneResponse::reference());
        for (response, truth) in solution.lanes[1..].iter().zip(truth) {
            assert_close(response, truth);
            assert!(response.coherence > 0.99, "{}", response.coherence);
            assert_eq!(response.equaliser.len(), EQ_POINTS);
            for point in &response.equaliser {
                assert!(point.norm().amplitude_to_db().abs() < 0.2, "{point}");
            }
        }
    }

    #[test]
    fn bin_solver_recovers_an_equaliser_ripple() {
        let ripple_db = |nu: f64| (TAU * 3.0 * nu).cos();
        let source = Gaussian::new(5).block(SOLVE_LEN);
        let mut generator = Gaussian::new(6);
        let lane = shaped(&source, |nu| {
            lane_response(0.2, 40f64.to_radians(), 0.9)(nu) * ripple_db(nu).db_to_amplitude()
        });
        let lanes = [
            with_noise(&source, 0.03, &mut generator),
            with_noise(&lane, 0.03, &mut generator),
        ];
        let solution = solve(&lanes, 0.8, 0.9).unwrap();
        let response = &solution.lanes[1];
        assert!((f64::from(response.delay_frac) - 0.2).abs() < 0.01);
        assert!(
            wrap_rad(f64::from(response.phase_rad) - 40f64.to_radians())
                .to_degrees()
                .abs()
                < 0.5
        );
        let compared: Vec<(f64, f64)> = response
            .equaliser
            .iter()
            .enumerate()
            .map(|(point, value)| (f64::from(equaliser_frequency(point, EQ_POINTS)), value))
            .filter(|(nu, _)| nu.abs() <= 0.375)
            .map(|(nu, value)| {
                (
                    nu,
                    f64::from(value.norm()).amplitude_to_db() - ripple_db(nu),
                )
            })
            .collect();
        let offset = compared.iter().map(|(_, error)| error).sum::<f64>() / compared.len() as f64;
        for (nu, error) in compared {
            assert!((error - offset).abs() < 0.2, "nu {nu}: {error} dB off");
        }
    }

    #[test]
    fn bin_solver_flags_an_incoherent_lane() {
        let source = Gaussian::new(8).block(SOLVE_LEN);
        let mut lanes = lanes_from(
            &source,
            &[(0.1, 30.0, 0.0), (0.0, 0.0, 0.0), (-0.2, -60.0, 0.0)],
            0.03,
            9,
        );
        lanes[2] = Gaussian::new(10).block(SOLVE_LEN);
        match solve(&lanes, 0.5, 0.6) {
            Err(BinError::LowCoherence { lane, coherence }) => {
                assert_eq!(lane, 2);
                assert!(coherence < 0.05, "{coherence}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_notch_on_the_reference_lane_stays_out_of_the_fit() {
        let truth = (0.3, -50.0, -1.0);
        let source = Gaussian::new(14).block(SOLVE_LEN);
        let mut lanes = lanes_from(&source, &[truth], 0.03, 15);
        let notched = shaped(&source, |nu| {
            if (0.1..=0.2).contains(&nu) {
                Complex::default()
            } else {
                Complex::new(1.0, 0.0)
            }
        });
        lanes[0] = with_noise(&notched, 0.03, &mut Gaussian::new(16));
        let solution = solve(&lanes, 0.8, 0.9).unwrap();
        assert_close(&solution.lanes[1], truth);
    }

    #[test]
    fn phase_slope_fit_unwraps_across_bins() {
        for truth in [
            (0.49, 179.0, 0.0),
            (-0.49, -179.0, -1.0),
            (1.49, 175.0, 0.0),
        ] {
            let source = Gaussian::new(12).block(SOLVE_LEN);
            let lanes = lanes_from(&source, &[truth], 0.03, 13);
            let solution = solve(&lanes, 0.8, 0.9).unwrap();
            assert_close(&solution.lanes[1], truth);
        }
    }

    #[test]
    fn a_short_capture_or_a_wrong_lane_count_is_refused() {
        let lanes = [
            vec![Complex::new(1.0, 0.0); 100],
            vec![Complex::new(1.0, 0.0); 100],
        ];
        assert_eq!(solve(&lanes, 0.8, 0.9), Err(BinError::Short));
        let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        assert_eq!(
            BinSolver::new(3, 64).solve(&views, 0.8, 0.9),
            Err(BinError::LaneCount {
                expected: 3,
                got: 2
            })
        );
        let silent = [
            vec![Complex::default(); 4096],
            vec![Complex::default(); 4096],
        ];
        assert_eq!(solve(&silent, 0.8, 0.9), Err(BinError::FewBins));
    }
}
