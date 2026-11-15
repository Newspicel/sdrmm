use std::f64::consts::TAU;

use num_complex::Complex;

use super::{BinSolver, LaneResponse};
use crate::{
    array::array_sync::{frequency::bin_frequency, line_fit::LineFit},
    math::complex::widen,
    math::special::wrap_rad,
};

impl BinSolver {
    pub(super) fn lane_response(&mut self, lane: usize) -> LaneResponse {
        self.unwrap_phase(lane);
        let (phase, delay) = self.fit_phase_slope();
        let gain = self.mean_gain(lane);
        let coherence = self.coherence(lane, phase, delay);
        let equaliser = self.equaliser(lane, phase, delay, gain);
        LaneResponse {
            delay_frac: delay as f32,
            phase_rad: wrap_rad(phase) as f32,
            gain: gain as f32,
            coherence,
            equaliser,
        }
    }

    pub(super) fn bin_of(&self, centred: usize) -> usize {
        (centred + self.len - self.len / 2) % self.len
    }

    pub(super) fn is_used(&self, centred: usize) -> bool {
        self.fits[self.bin_of(centred)].used
    }

    fn centred_arg(&self, centred: usize, lane: usize) -> f64 {
        f64::from(self.responses[self.bin_of(centred) * self.lanes + lane].arg())
    }

    fn unwrap_phase(&mut self, lane: usize) {
        let half = self.len / 2;
        let Some(anchor) = (0..self.len)
            .filter(|&centred| self.is_used(centred))
            .min_by_key(|&centred| centred.abs_diff(half))
        else {
            return;
        };
        let start = self.centred_arg(anchor, lane);
        let mut phase = std::mem::take(&mut self.phase);
        phase[self.bin_of(anchor)] = start;
        self.unwrap_run(anchor + 1..self.len, start, lane, &mut phase);
        self.unwrap_run((0..anchor).rev(), start, lane, &mut phase);
        self.phase = phase;
    }

    fn unwrap_run(
        &self,
        run: impl Iterator<Item = usize>,
        start: f64,
        lane: usize,
        phase: &mut [f64],
    ) {
        let mut previous = start;
        for centred in run.filter(|&centred| self.is_used(centred)) {
            previous += wrap_rad(self.centred_arg(centred, lane) - previous);
            phase[self.bin_of(centred)] = previous;
        }
    }

    fn fit_phase_slope(&self) -> (f64, f64) {
        let mut fit = LineFit::default();
        for (bin, used) in self.fits.iter().enumerate().filter(|(_, fit)| fit.used) {
            let x = -TAU * bin_frequency(bin, self.len);
            fit.add(f64::from(used.lambda), x, self.phase[bin]);
        }
        fit.solve()
    }

    fn mean_gain(&self, lane: usize) -> f64 {
        let order = self.lanes;
        let (mut weighted, mut total) = (0.0, 0.0);
        for (bin, fit) in self.fits.iter().enumerate().filter(|(_, fit)| fit.used) {
            let weight = f64::from(fit.lambda);
            weighted += weight * f64::from(self.responses[bin * order + lane].norm());
            total += weight;
        }
        if total > 0.0 { weighted / total } else { 0.0 }
    }

    fn coherence(&self, lane: usize, phase: f64, delay: f64) -> f32 {
        let order = self.lanes;
        let mut cross = Complex::<f64>::default();
        let (mut reference, mut power) = (0.0, 0.0);
        for (bin, _) in self.fits.iter().enumerate().filter(|(_, fit)| fit.used) {
            let matrix = &self.cov[bin * order * order..(bin + 1) * order * order];
            let model = self.fitted(bin, 1.0, phase, delay);
            cross += widen(matrix[lane * order]) * model.conj();
            reference += f64::from(matrix[0].re);
            power += f64::from(matrix[lane * order + lane].re);
        }
        if reference <= 0.0 || power <= 0.0 {
            return 0.0;
        }
        (cross.norm_sqr() / (reference * power)) as f32
    }

    /// The fitted lane response at `bin`: a gain, a phase and a pure delay.
    pub(super) fn fitted(&self, bin: usize, gain: f64, phase: f64, delay: f64) -> Complex<f64> {
        Complex::from_polar(gain, phase - TAU * bin_frequency(bin, self.len) * delay)
    }
}
