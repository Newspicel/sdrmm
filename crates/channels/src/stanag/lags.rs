use num_complex::Complex;
use sdrmm_dsp::fft::FftPair;

use super::RATE_HZ;

pub(super) const MAX_LAG: usize = 1_100;
const MIN_STRENGTH: f32 = 0.04;
const MIN_Z: f32 = 6.0;
const MIN_TOLERANCE: f64 = 1.5;
const GAP: usize = 2;
const MIN_RING: usize = 6;
const RING_FRACTION: f64 = 0.04;
const MAD_TO_SIGMA: f32 = 1.4826;
const SIGMA_FLOOR: f32 = 1e-4;

#[derive(Clone, Copy, Debug)]
pub(super) struct Lag {
    pub seconds: f64,
    pub tolerance: f64,
}

impl Lag {
    pub const fn symbols(count: f64, baud: f64, tolerance: f64) -> Self {
        Self {
            seconds: count / baud,
            tolerance,
        }
    }

    pub const fn period(hz: f64, tolerance: f64) -> Self {
        Self {
            seconds: 1.0 / hz,
            tolerance,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Peak {
    pub lag: usize,
    pub strength: f32,
    pub z: f32,
    pub value: Complex<f32>,
}

impl Peak {
    pub fn passes(&self) -> bool {
        self.strength >= MIN_STRENGTH && self.z >= MIN_Z
    }
}

pub(super) struct Lags {
    fft: FftPair,
    buf: Vec<Complex<f32>>,
    acf: Vec<Complex<f32>>,
    magnitude: Vec<f32>,
    ring: Vec<f32>,
}

impl Lags {
    pub fn new(window: usize) -> Self {
        let size = (window + MAX_LAG).next_power_of_two();
        Self {
            fft: FftPair::new(size),
            buf: vec![Complex::default(); size],
            acf: vec![Complex::default(); MAX_LAG],
            magnitude: vec![0.0; MAX_LAG],
            ring: Vec::with_capacity(MAX_LAG),
        }
    }

    pub fn measure(&mut self, x: &[Complex<f32>]) {
        let n = x.len().min(self.buf.len());
        self.buf[..n].copy_from_slice(&x[..n]);
        self.buf[n..].fill(Complex::default());
        self.fft.forward(&mut self.buf);
        for bin in &mut self.buf {
            *bin = Complex::new(bin.norm_sqr(), 0.0);
        }
        self.fft.inverse(&mut self.buf);
        let zero = self.buf[0].re / n.max(1) as f32;
        let scale = if zero > f32::MIN_POSITIVE {
            zero.recip()
        } else {
            0.0
        };
        for (lag, (acf, magnitude)) in self.acf.iter_mut().zip(&mut self.magnitude).enumerate() {
            let overlap = n.saturating_sub(lag).max(1) as f32;
            *acf = self.buf[lag] * (scale / overlap);
            *magnitude = acf.norm();
        }
    }

    pub fn peak(&mut self, lag: Lag) -> Peak {
        let center = lag.seconds * RATE_HZ;
        let tolerance = (lag.tolerance * center).max(MIN_TOLERANCE);
        let width = ((RING_FRACTION * center) as usize).max(MIN_RING);
        let lo = ((center - tolerance).floor() as usize).max(GAP + width + 1);
        let hi = ((center + tolerance).ceil() as usize).min(MAX_LAG - GAP - width - 1);
        if lo > hi {
            return Peak::default();
        }
        let Some((best, &top)) = self.magnitude[lo..=hi]
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
        else {
            return Peak::default();
        };
        let (median, sigma) = self.ring_statistics(
            lo - GAP - width..lo - GAP,
            hi + GAP + 1..hi + GAP + 1 + width,
        );
        let strength = top - median;
        Peak {
            lag: lo + best,
            strength,
            z: strength / sigma,
            value: self.acf[lo + best],
        }
    }

    fn ring_statistics(
        &mut self,
        below: std::ops::Range<usize>,
        above: std::ops::Range<usize>,
    ) -> (f32, f32) {
        self.ring.clear();
        self.ring.extend_from_slice(&self.magnitude[below]);
        self.ring.extend_from_slice(&self.magnitude[above]);
        let middle = median(&mut self.ring);
        for value in &mut self.ring {
            *value = (*value - middle).abs();
        }
        let spread = median(&mut self.ring);
        (middle, (MAD_TO_SIGMA * spread).max(SIGMA_FLOOR))
    }
}

fn median(values: &mut [f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let middle = values.len() / 2;
    *values.select_nth_unstable_by(middle, f32::total_cmp).1
}
