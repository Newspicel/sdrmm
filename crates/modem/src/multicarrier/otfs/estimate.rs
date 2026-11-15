use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::{
    complex::{narrow, widen},
    linalg::{cholesky_c64, cholesky_solve_c64},
};

use super::{inverse::inverse_diagonal, projector::polynomial_projector};

pub const RIDGE: f64 = 1e-6;
pub const TAP_SIGNIFICANCE: f64 = 9.0;
pub const RELATIVE_FLOOR: f64 = 1e-3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    pub first_tap: i32,
    pub last_tap: i32,
    pub doppler_order: usize,
}

impl Support {
    #[must_use]
    pub fn taps(&self) -> usize {
        (self.last_tap - self.first_tap + 1).max(1) as usize
    }
}

#[derive(Clone, Debug)]
pub struct DelayDopplerFit {
    occupied: usize,
    taps: usize,
    symbols: usize,
    basis: Vec<Complex<f64>>,
    smoother: Vec<f64>,
    normal: Vec<Complex<f64>>,
    factor: Vec<Complex<f64>>,
    rhs: Vec<Complex<f64>>,
    gains: Vec<Complex<f64>>,
    smoothed: Vec<Complex<f64>>,
    tap_noise: Vec<f64>,
    active: Vec<usize>,
    doppler_dof: f64,
}

impl DelayDopplerFit {
    #[must_use]
    pub fn new(offsets: &[i32], fft: usize, symbols: usize, support: Support) -> Self {
        let taps = support.taps();
        let (smoother, doppler_dof) = polynomial_projector(symbols, support.doppler_order);
        Self {
            occupied: offsets.len(),
            taps,
            symbols,
            basis: basis(offsets, fft, support),
            smoother,
            normal: vec![Complex::new(0.0, 0.0); taps * taps],
            factor: vec![Complex::new(0.0, 0.0); taps * taps],
            rhs: vec![Complex::new(0.0, 0.0); taps],
            gains: vec![Complex::new(0.0, 0.0); symbols * taps],
            smoothed: vec![Complex::new(0.0, 0.0); symbols * taps],
            tap_noise: vec![0.0; taps],
            active: Vec::with_capacity(taps),
            doppler_dof,
        }
    }

    #[must_use]
    pub fn active_taps(&self) -> usize {
        self.active.len()
    }

    pub fn fit(
        &mut self,
        received: &[Complex<f32>],
        known: &[Complex<f32>],
        noise_var: f64,
        channel: &mut [Complex<f32>],
    ) -> Option<f64> {
        debug_assert_eq!(received.len(), self.symbols * self.occupied);
        debug_assert_eq!(known.len(), received.len());
        debug_assert_eq!(channel.len(), received.len());
        self.active.clear();
        self.active.extend(0..self.taps);
        self.pass(received, known, noise_var)?;
        self.select_taps();
        if self.active.is_empty() {
            return None;
        }
        self.pass(received, known, noise_var)?;
        self.synthesise(channel);
        Some(residual_noise(received, known, channel, self.dof()))
    }

    fn dof(&self) -> f64 {
        self.active.len() as f64 * self.doppler_dof
    }

    fn pass(
        &mut self,
        received: &[Complex<f32>],
        known: &[Complex<f32>],
        noise_var: f64,
    ) -> Option<()> {
        let o = self.occupied;
        self.tap_noise.fill(0.0);
        self.gains.fill(Complex::new(0.0, 0.0));
        for symbol in 0..self.symbols {
            let rows = symbol * o..(symbol + 1) * o;
            self.fit_symbol(&received[rows.clone()], &known[rows], symbol, noise_var)?;
        }
        self.smooth();
        Some(())
    }

    fn fit_symbol(
        &mut self,
        received: &[Complex<f32>],
        known: &[Complex<f32>],
        symbol: usize,
        noise_var: f64,
    ) -> Option<()> {
        let a = self.accumulate(received, known);
        let scale = (0..a).map(|k| self.normal[k * a + k].re).sum::<f64>() / a.max(1) as f64;
        let ridge = (noise_var * a as f64)
            .max(RIDGE * scale)
            .max(f64::MIN_POSITIVE);
        cholesky_c64(&self.normal[..a * a], a, ridge, &mut self.factor[..a * a]).ok()?;
        cholesky_solve_c64(&self.factor[..a * a], a, &mut self.rhs[..a]);
        let t = self.taps;
        for (k, &tap) in self.active.iter().enumerate() {
            self.gains[symbol * t + tap] = self.rhs[k];
            self.tap_noise[tap] += noise_var * inverse_diagonal(&self.factor[..a * a], a, k);
        }
        Some(())
    }

    fn accumulate(&mut self, received: &[Complex<f32>], known: &[Complex<f32>]) -> usize {
        let (t, a) = (self.taps, self.active.len());
        self.normal[..a * a].fill(Complex::new(0.0, 0.0));
        self.rhs[..a].fill(Complex::new(0.0, 0.0));
        for (row, (&y, &x)) in self.basis.chunks_exact(t).zip(received.iter().zip(known)) {
            let x = widen(x);
            let weight = x.norm_sqr();
            let target = widen(y) * x.conj();
            for (i, &ti) in self.active.iter().enumerate() {
                let fi = row[ti].conj();
                self.rhs[i] += fi * target;
                for (j, &tj) in self.active.iter().enumerate() {
                    self.normal[i * a + j] += fi * row[tj] * weight;
                }
            }
        }
        a
    }

    fn smooth(&mut self) {
        let (s, t) = (self.symbols, self.taps);
        for n in 0..s {
            for k in 0..t {
                let mut acc = Complex::new(0.0, 0.0);
                for m in 0..s {
                    acc += self.gains[m * t + k] * self.smoother[n * s + m];
                }
                self.smoothed[n * t + k] = acc;
            }
        }
    }

    fn select_taps(&mut self) {
        let (s, t) = (self.symbols, self.taps);
        let share = self.doppler_dof / (s as f64 * s as f64);
        let energy = |k: usize| {
            (0..s)
                .map(|n| self.smoothed[n * t + k].norm_sqr())
                .sum::<f64>()
                / s as f64
        };
        let peak = (0..t).map(energy).fold(0.0f64, f64::max);
        self.active.clear();
        for k in 0..t {
            let floor = (TAP_SIGNIFICANCE * self.tap_noise[k] * share).max(RELATIVE_FLOOR * peak);
            if energy(k) > floor {
                self.active.push(k);
            }
        }
    }

    fn synthesise(&self, channel: &mut [Complex<f32>]) {
        let (o, t) = (self.occupied, self.taps);
        for n in 0..self.symbols {
            let gains = &self.smoothed[n * t..(n + 1) * t];
            for (slot, row) in channel[n * o..(n + 1) * o]
                .iter_mut()
                .zip(self.basis.chunks_exact(t))
            {
                *slot = narrow(row.iter().zip(gains).map(|(f, g)| f * g).sum());
            }
        }
    }
}

fn basis(offsets: &[i32], fft: usize, support: Support) -> Vec<Complex<f64>> {
    let mut basis = Vec::with_capacity(offsets.len() * support.taps());
    for &offset in offsets {
        for tap in support.first_tap..=support.last_tap {
            let phase = -TAU * f64::from(offset) * f64::from(tap) / fft as f64;
            basis.push(Complex::from_polar(1.0, phase));
        }
    }
    basis
}

fn residual_noise(
    received: &[Complex<f32>],
    known: &[Complex<f32>],
    channel: &[Complex<f32>],
    dof: f64,
) -> f64 {
    let count = received.len() as f64;
    let sum: f64 = received
        .iter()
        .zip(known)
        .zip(channel)
        .map(|((&y, &x), &h)| f64::from((y - h * x).norm_sqr()))
        .sum();
    sum / (count - dof).max(1.0)
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::rng::Rng;

    use super::*;

    fn offsets() -> Vec<i32> {
        (-26..=26).filter(|&k| k != 0).collect()
    }

    fn support() -> Support {
        Support {
            first_tap: -2,
            last_tap: 16,
            doppler_order: 3,
        }
    }

    fn symbols_of(n: usize) -> Vec<Complex<f32>> {
        let mut rng = Rng::mixed(0x3c1);
        (0..n)
            .map(|_| Complex::from_polar(1.0, (TAU * rng.uniform()) as f32))
            .collect()
    }

    fn response(offset: i32, symbol: usize) -> Complex<f32> {
        let paths = [(0.0f64, 0.9f64, 0.0f64, 0.002f64), (5.0, 0.4, 1.3, -0.004)];
        paths
            .iter()
            .map(|&(delay, gain, phase, doppler)| {
                let angle = phase - TAU * f64::from(offset) * delay / 64.0
                    + TAU * doppler * symbol as f64 * 5.0;
                Complex::from_polar(gain as f32, angle as f32)
            })
            .sum()
    }

    #[test]
    fn a_noiseless_doubly_selective_channel_is_recovered() {
        let offsets = offsets();
        let symbols = 16;
        let mut fit = DelayDopplerFit::new(&offsets, 64, symbols, support());
        let known = symbols_of(symbols * offsets.len());
        let truth: Vec<Complex<f32>> = (0..symbols)
            .flat_map(|n| offsets.iter().map(move |&k| response(k, n)))
            .collect();
        let received: Vec<Complex<f32>> = truth.iter().zip(&known).map(|(h, x)| h * x).collect();
        let mut channel = vec![Complex::new(0.0, 0.0); truth.len()];
        let noise = fit.fit(&received, &known, 1e-6, &mut channel).unwrap();
        let worst = truth
            .iter()
            .zip(&channel)
            .map(|(a, b)| (a - b).norm())
            .fold(0.0f32, f32::max);
        assert!(worst < 0.02, "worst channel error {worst}");
        assert!(noise < 1e-3, "residual {noise}");
    }

    #[test]
    fn a_fit_with_no_significant_tap_keeps_the_channel_it_was_given() {
        let offsets = offsets();
        let symbols = 16;
        let mut fit = DelayDopplerFit::new(&offsets, 64, symbols, support());
        let known = symbols_of(symbols * offsets.len());
        let received = vec![Complex::new(0.0f32, 0.0); known.len()];
        let kept = Complex::new(0.5f32, -0.25);
        let mut channel = vec![kept; known.len()];
        assert_eq!(fit.fit(&received, &known, 1e-3, &mut channel), None);
        assert_eq!(fit.active_taps(), 0);
        assert!(channel.iter().all(|&h| h == kept), "the channel was erased");
    }
}
