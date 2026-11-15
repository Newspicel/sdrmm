mod acquire;
mod resample;
mod window;

use std::f64::consts::{FRAC_1_SQRT_2, TAU};

use num_complex::Complex;
use sdrmm_dsp::{LoopFilter, math::fft::FftPair};

use self::{acquire::Point, resample::Interpolator};
use super::{MAX_CLOCK_PPM, TIMING_BW, TRACK_BW, params::CssParams};
use crate::{constellation::demap::energy_llrs, soft::Llr};

const TRACK_DAMPING: f64 = FRAC_1_SQRT_2;

const TRACK_RANGE_BINS: f64 = 0.25;

const MIN_TIMING_RANGE: f64 = 0.5;

const SPLIT_WEIGHT: f64 = 6.0;

#[derive(Clone)]
pub struct CssDemod {
    params: CssParams,
    fft: FftPair,
    reference: Vec<Complex<f32>>,
    stretch: f64,
    window: Vec<Complex<f32>>,
    source: Vec<Complex<f32>>,
    interpolator: Interpolator,
    dechirped: Vec<Complex<f32>>,
    bins: Vec<Complex<f32>>,
    energies: Vec<f32>,
    symbol_energies: Vec<f32>,
    symbol_llrs: Vec<Llr>,
    points: Vec<Point>,
    hypotheses: Vec<Complex<f64>>,
    offset_bins: f64,
    tracker: LoopFilter,
    timing: LoopFilter,
    anchor: Option<f64>,
}

impl std::fmt::Debug for CssDemod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CssDemod")
            .field("params", &self.params)
            .finish_non_exhaustive()
    }
}

impl CssDemod {
    #[must_use]
    pub fn new(params: CssParams) -> Self {
        let n = params.chips();
        let zero = Complex::new(0.0, 0.0);
        Self {
            reference: params.base_chirp().into_iter().map(|c| c.conj()).collect(),
            stretch: 1.0,
            window: vec![zero; n],
            source: vec![zero; resample::source_len(n)],
            interpolator: Interpolator::new(),
            dechirped: vec![zero; n],
            bins: vec![zero; n],
            energies: vec![0.0; n],
            symbol_energies: vec![0.0; n],
            symbol_llrs: vec![Llr(0.0); params.bits_per_symbol()],
            points: Vec::new(),
            hypotheses: vec![Complex::new(0.0, 0.0); n],
            offset_bins: 0.0,
            tracker: tracker(),
            timing: timing_loop(n),
            anchor: None,
            params,
            fft: FftPair::new(n),
        }
    }

    #[must_use]
    pub fn offset_bins(&self) -> f64 {
        self.offset_bins
    }

    #[must_use]
    pub fn samples_per_symbol(&self) -> f64 {
        self.params.chips() as f64 + self.timing.freq_norm()
    }

    #[must_use]
    pub fn clock_ppm(&self) -> f64 {
        self.timing.freq_norm() / self.params.chips() as f64 * 1e6
    }

    #[must_use]
    pub fn params(&self) -> &CssParams {
        &self.params
    }

    pub fn energies(&mut self, iq: &[Complex<f32>], origin: usize, symbol: usize, out: &mut [f32]) {
        assert_eq!(
            out.len(),
            self.params.chips(),
            "one energy per cyclic shift"
        );
        let start = self.position(origin, symbol);
        self.observe(iq, start);
        out.copy_from_slice(&self.symbol_energies);
    }

    fn position(&self, origin: usize, symbol: usize) -> f64 {
        let nominal = (origin + symbol * self.params.chips()) as f64;
        match self.anchor {
            None => nominal,
            Some(anchor) => {
                let period = self.samples_per_symbol();
                anchor + ((nominal - anchor) / period).round() * period
            }
        }
    }

    fn observe(&mut self, iq: &[Complex<f32>], start: f64) {
        self.load(iq, start, self.offset_bins, self.samples_per_symbol());
        self.dechirp();
    }

    fn peak(&self) -> f32 {
        self.energies.iter().copied().fold(0.0f32, f32::max)
    }

    fn decide(&mut self, iq: &[Complex<f32>], origin: usize, symbol: usize) -> u32 {
        let n = self.params.chips() as f64;
        let start = self.position(origin, symbol);
        self.observe(iq, start);
        let bin = argmax_bin(&self.energies);
        let value = self.symbol_of(f64::from(bin));
        let (tone, late) = self.resolve(f64::from(value) * self.stretch, value);
        let weight = self.timing_weight(value);
        let freq_error = (wrap(tone - f64::from(value) * self.stretch, n) + weight.min(1.0) * late)
            .clamp(-0.5, 0.5);
        let timing_error = (weight * late).clamp(-0.5, 0.5);
        self.offset_bins += self.tracker.advance(TAU * freq_error) / TAU;
        let step = self.timing.advance(TAU * timing_error) / TAU;
        self.anchor = Some(start + n + step);
        value
    }

    fn symbol_of(&self, tone: f64) -> u32 {
        let n = self.params.chips() as i64;
        ((tone / self.stretch).round() as i64).rem_euclid(n) as u32
    }

    fn resolve(&self, tone: f64, value: u32) -> (f64, f64) {
        let n = self.params.chips();
        let split = self.wrap_point(value);
        if split == 0 || split >= n {
            return (tone, 0.0);
        }
        let resolved = self.split_tone(tone, split);
        (resolved.tone, resolved.late)
    }

    fn timing_weight(&self, value: u32) -> f64 {
        let share = self.wrap_point(value) as f64 / self.params.chips() as f64;
        SPLIT_WEIGHT * share * (1.0 - share)
    }

    fn wrap_point(&self, value: u32) -> usize {
        let n = self.params.chips();
        let split = ((n as f64 - f64::from(value)) / self.stretch).round();
        split.clamp(0.0, n as f64) as usize
    }

    pub fn demodulate(
        &mut self,
        iq: &[Complex<f32>],
        origin: usize,
        symbols: usize,
        out: &mut Vec<u32>,
    ) {
        out.reserve(symbols);
        for symbol in 0..symbols {
            let value = self.decide(iq, origin, symbol);
            out.push(value);
        }
    }

    pub fn llrs(
        &mut self,
        iq: &[Complex<f32>],
        origin: usize,
        symbols: usize,
        noise_var: f64,
        out: &mut Vec<Llr>,
    ) {
        out.reserve(symbols * self.params.bits_per_symbol());
        for symbol in 0..symbols {
            let _ = self.decide(iq, origin, symbol);
            energy_llrs(&self.symbol_energies, noise_var, &mut self.symbol_llrs);
            out.extend_from_slice(&self.symbol_llrs);
        }
    }

    pub fn noise_var(&mut self, iq: &[Complex<f32>], origin: usize, symbols: usize) -> f64 {
        assert!(
            symbols > 0,
            "a variance needs at least one symbol to measure"
        );
        let n = self.params.chips();
        let mut sum = 0.0f64;
        for symbol in 0..symbols {
            let start = self.position(origin, symbol);
            self.observe(iq, start);
            let peak = f64::from(self.peak());
            sum += self.energies.iter().map(|&e| f64::from(e)).sum::<f64>() - peak;
        }
        sum / (symbols * (n - 1)) as f64
    }

    fn restart(&mut self) {
        self.offset_bins = 0.0;
        self.tracker = tracker();
        self.timing = timing_loop(self.params.chips());
        self.anchor = None;
    }
}

fn tracker() -> LoopFilter {
    LoopFilter::new(TRACK_BW, TRACK_DAMPING, TRACK_RANGE_BINS)
}

fn timing_loop(chips: usize) -> LoopFilter {
    let range = (chips as f64 * MAX_CLOCK_PPM * 1e-6).max(MIN_TIMING_RANGE);
    LoopFilter::new(TIMING_BW, TRACK_DAMPING, range)
}

fn wrap(value: f64, period: f64) -> f64 {
    value - (value / period).round() * period
}

pub(super) fn argmax_bin(energies: &[f32]) -> u32 {
    let mut best = 0usize;
    for (k, &e) in energies.iter().enumerate() {
        if e >= energies[best] {
            best = k;
        }
    }
    best as u32
}
