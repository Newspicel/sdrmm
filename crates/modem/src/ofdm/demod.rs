use num_complex::Complex;
use sdrmm_dsp::math::{complex::narrow, fft::Transform};

use super::{
    estimate::{ChannelEstimate, ChannelEstimator, noise_var_from_repeats},
    params::OfdmParams,
    smoother::DelaySmoother,
    sync::{Acquisition, PreambleSync},
    tracker::{PilotFit, PilotTracker},
};
use crate::{
    constellation::{Constellation, demap::max_log_llrs},
    framesync::rotor,
    soft::Llr,
};

pub const DEFAULT_BACKOFF: usize = 4;

pub const DELAY_GUARD_TAPS: usize = 4;

#[derive(Clone)]
pub struct OfdmDemod {
    params: OfdmParams,
    fft: Transform,
    grid: Vec<Complex<f32>>,
    sync: PreambleSync,
    estimator: ChannelEstimator,
    pilot_tracking: bool,
    backoff: usize,
    channel: ChannelEstimate,
    smoother: Option<DelaySmoother>,
    tracker: PilotTracker,
    cfo: f64,
    data_start: usize,
    first: Vec<Complex<f32>>,
    second: Vec<Complex<f32>>,
    pilot_offsets: Vec<i32>,
    pilot_errors: Vec<Complex<f32>>,
    pilot_weights: Vec<f32>,
    points: Vec<Complex<f32>>,
    sync_derotated: Vec<Complex<f32>>,
    sync_scores: Vec<f64>,
}

impl OfdmDemod {
    #[must_use]
    pub fn new(params: OfdmParams) -> Self {
        let fft_size = params.fft();
        let occupied = params.map().occupied().len();
        let pilots = params.map().pilots().len();
        let pilot_offsets = params.map().pilots().iter().map(|c| c.offset).collect();
        let data = params.data_subcarriers();
        let sync = PreambleSync::new(&params);
        let (derot_cap, score_cap) = sync.scratch_sizes();
        Self {
            fft: Transform::forward(fft_size),
            grid: vec![Complex::new(0.0, 0.0); fft_size],
            sync,
            estimator: ChannelEstimator::LongTraining,
            pilot_tracking: true,
            backoff: DEFAULT_BACKOFF,
            channel: ChannelEstimate::new(fft_size),
            smoother: DelaySmoother::new(
                params.map(),
                -(DELAY_GUARD_TAPS as isize),
                params.cp() + 2 * DELAY_GUARD_TAPS + 1,
            )
            .ok(),
            tracker: PilotTracker::new(),
            cfo: 0.0,
            data_start: params.data_offset(),
            first: vec![Complex::new(0.0, 0.0); occupied],
            second: vec![Complex::new(0.0, 0.0); occupied],
            pilot_offsets,
            pilot_errors: vec![Complex::new(0.0, 0.0); pilots],
            pilot_weights: vec![0.0; pilots],
            points: vec![Complex::new(0.0, 0.0); data],
            sync_derotated: Vec::with_capacity(derot_cap),
            sync_scores: Vec::with_capacity(score_cap),
            params,
        }
    }

    #[must_use]
    pub fn with_estimator(mut self, estimator: ChannelEstimator) -> Self {
        if estimator == ChannelEstimator::ShortComb {
            let preamble = self.params.preamble();
            assert!(
                preamble.short_samples(self.params.fft()) >= 2 * self.params.fft(),
                "the comb estimator needs two transform lengths of short training"
            );
        }
        self.estimator = estimator;
        self
    }

    #[must_use]
    pub fn with_window_backoff(mut self, samples: usize) -> Self {
        self.backoff = samples.min(self.params.cp());
        self
    }

    #[must_use]
    pub fn with_pilot_tracking(mut self, on: bool) -> Self {
        self.pilot_tracking = on;
        self
    }

    #[must_use]
    pub fn params(&self) -> &OfdmParams {
        &self.params
    }

    #[must_use]
    pub fn channel(&self) -> &ChannelEstimate {
        &self.channel
    }

    #[must_use]
    pub fn cfo(&self) -> f64 {
        self.cfo
    }

    #[must_use]
    pub fn data_start(&self) -> usize {
        self.data_start
    }

    pub fn acquire(&mut self, x: &[Complex<f32>], search: usize) -> Option<Acquisition> {
        let acquisition =
            self.sync
                .acquire_into(x, search, &mut self.sync_derotated, &mut self.sync_scores)?;
        self.cfo = acquisition.cfo;
        self.data_start = acquisition.data_start;
        self.tracker.reset();
        match self.estimator {
            ChannelEstimator::LongTraining => self.estimate_long(x, acquisition.long_start),
            ChannelEstimator::ShortComb => self.estimate_comb(x, acquisition.long_start),
        }
        Some(acquisition)
    }

    pub fn genie(&mut self, data_start: usize, channel: &[Complex<f32>], noise_var: f64) {
        assert_eq!(
            channel.len(),
            self.params.map().occupied().len(),
            "one channel gain per occupied subcarrier"
        );
        self.cfo = 0.0;
        self.data_start = data_start;
        self.tracker.reset();
        self.channel.clear();
        for (c, &h) in self.params.map().occupied().iter().zip(channel) {
            self.channel.set(c.bin, h);
        }
        self.channel.finish(self.params.map(), noise_var, 0.0, None);
    }

    pub fn symbol(&mut self, x: &[Complex<f32>], symbol: usize, out: &mut [Complex<f32>]) {
        assert_eq!(
            out.len(),
            self.params.data_subcarriers(),
            "one point per data subcarrier"
        );
        self.read_symbol(x, symbol);
        out.copy_from_slice(&self.points);
    }

    pub fn demodulate(&mut self, x: &[Complex<f32>], symbols: usize, out: &mut Vec<Complex<f32>>) {
        out.reserve(symbols * self.params.data_subcarriers());
        for symbol in 0..symbols {
            self.read_symbol(x, symbol);
            out.extend_from_slice(&self.points);
        }
    }

    pub fn llrs(&self, points: &[Complex<f32>], table: &Constellation, out: &mut [Llr]) {
        let bits = table.bits_per_symbol();
        assert_eq!(points.len(), self.params.data_subcarriers());
        assert_eq!(out.len(), points.len() * bits);
        for (index, (&point, slot)) in points.iter().zip(out.chunks_exact_mut(bits)).enumerate() {
            let bin = self.params.map().data()[index].bin;
            match self.channel.bin_noise_var(bin) {
                Some(noise_var) => max_log_llrs(point, table, noise_var, slot),
                None => slot.fill(Llr(0.0)),
            }
        }
    }

    pub fn observe(
        &mut self,
        x: &[Complex<f32>],
        symbol: usize,
        raw: &mut [Complex<f32>],
    ) -> PilotFit {
        assert_eq!(
            raw.len(),
            self.params.map().occupied().len(),
            "one value per occupied subcarrier"
        );
        let fit = self.open_symbol(x, symbol);
        for (slot, c) in raw.iter_mut().zip(self.params.map().occupied()) {
            *slot = self.grid[c.bin];
        }
        fit
    }

    fn window_start(&self, symbol: usize) -> usize {
        self.data_start + symbol * self.params.symbol_samples() + self.params.cp() - self.backoff
    }

    /// Transforms the symbol's window and fits its pilot phase, when tracking.
    fn open_symbol(&mut self, x: &[Complex<f32>], symbol: usize) -> PilotFit {
        self.transform_window(x, self.window_start(symbol));
        if self.pilot_tracking {
            self.fit_pilots(symbol)
        } else {
            PilotFit::default()
        }
    }

    fn read_symbol(&mut self, x: &[Complex<f32>], symbol: usize) {
        let fit = self.open_symbol(x, symbol);
        let data = self.params.map().data();
        if fit.common_rad == 0.0 && fit.slope_rad_per_bin == 0.0 {
            for (index, c) in data.iter().enumerate() {
                self.points[index] = self.channel.equalize(c.bin, self.grid[c.bin]);
            }
            return;
        }
        let mut offset = data[0].offset;
        let (rot64, step64) = phase_line(fit.common_rad, fit.slope_rad_per_bin, offset);
        let (mut rot, step) = (narrow(rot64), narrow(step64));
        for (index, c) in data.iter().enumerate() {
            while offset < c.offset {
                rot *= step;
                offset += 1;
            }
            let z = self.channel.equalize(c.bin, self.grid[c.bin]);
            self.points[index] = z * rot;
        }
    }

    fn fit_pilots(&mut self, symbol: usize) -> PilotFit {
        for (index, c) in self.params.map().pilots().iter().enumerate() {
            let z = self.channel.equalize(c.bin, self.grid[c.bin]);
            let expect = self.params.pilot_pattern().value(index, symbol);
            self.pilot_errors[index] = z * expect.conj();
            self.pilot_weights[index] = self.channel.gain(c.bin);
        }
        self.tracker
            .fit(&self.pilot_offsets, &self.pilot_errors, &self.pilot_weights)
    }

    fn transform_window(&mut self, x: &[Complex<f32>], start: usize) {
        let fft = self.params.fft();
        if self.cfo == 0.0 {
            let avail = x.len().saturating_sub(start).min(fft);
            if avail > 0 {
                self.grid[..avail].copy_from_slice(&x[start..start + avail]);
            }
            self.grid[avail..fft].fill(Complex::new(0.0, 0.0));
        } else {
            let (rot64, step64) = rotor(self.cfo, start);
            let (mut rot, step) = (narrow(rot64), narrow(step64));
            let avail = x.len().saturating_sub(start).min(fft);
            for (dst, &s) in self.grid[..avail].iter_mut().zip(&x[start..start + avail]) {
                *dst = s * rot;
                rot *= step;
            }
            self.grid[avail..fft].fill(Complex::new(0.0, 0.0));
        }
        self.fft.process_unitary(&mut self.grid);
    }

    fn estimate_long(&mut self, x: &[Complex<f32>], long_start: usize) {
        let fft = self.params.fft();
        let long_start =
            long_start.saturating_sub(self.backoff.min(self.params.preamble().long_guard));
        self.read_training(x, long_start, long_start + fft, |params, index| {
            (
                params.map().occupied()[index].bin,
                params.long_training(index),
            )
        });
    }

    fn estimate_comb(&mut self, x: &[Complex<f32>], long_start: usize) {
        let fft = self.params.fft();
        let preamble = self.params.preamble();
        let short_samples = preamble.short_samples(fft);
        let short_start = long_start
            .saturating_sub(preamble.long_guard)
            .saturating_sub(short_samples);
        let base = (short_start + (short_samples - 2 * fft)).saturating_sub(self.backoff);
        self.read_training(x, base, base + fft, |params, index| {
            (params.short_bins()[index].bin, params.short_training(index))
        });
    }

    fn read_training(
        &mut self,
        x: &[Complex<f32>],
        first: usize,
        second: usize,
        known: fn(&OfdmParams, usize) -> (usize, Complex<f32>),
    ) {
        let count = match self.estimator {
            ChannelEstimator::LongTraining => self.params.map().occupied().len(),
            ChannelEstimator::ShortComb => self.params.short_bins().len(),
        };
        self.transform_window(x, first);
        for index in 0..count {
            self.first[index] = self.grid[known(&self.params, index).0];
        }
        self.transform_window(x, second);
        for index in 0..count {
            self.second[index] = self.grid[known(&self.params, index).0];
        }
        let noise_var = noise_var_from_repeats(&self.first[..count], &self.second[..count]);
        self.channel.clear();
        let mut spread = 0.0;
        for index in 0..count {
            let (bin, value) = known(&self.params, index);
            let mean = (self.first[index] + self.second[index]) * 0.5;
            self.channel
                .set(bin, mean * value.conj() / value.norm_sqr());
            spread += 1.0 / f64::from(value.norm_sqr());
        }
        let estimate_var = 0.5 * noise_var * spread / count.max(1) as f64;
        let smoothing = match self.estimator {
            ChannelEstimator::LongTraining => self.smoother.as_mut().map(|s| (s, estimate_var)),
            ChannelEstimator::ShortComb => None,
        };
        let ramp = -(self.backoff as f64) / self.params.fft() as f64;
        self.channel
            .finish(self.params.map(), noise_var, ramp, smoothing);
    }
}

fn phase_line(common: f64, slope: f64, first: i32) -> (Complex<f64>, Complex<f64>) {
    (
        Complex::from_polar(1.0, -(common + slope * f64::from(first))),
        Complex::from_polar(1.0, -slope),
    )
}

impl std::fmt::Debug for OfdmDemod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OfdmDemod")
            .field("params", &self.params)
            .field("estimator", &self.estimator)
            .field("pilot_tracking", &self.pilot_tracking)
            .field("cfo", &self.cfo)
            .field("data_start", &self.data_start)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests;
