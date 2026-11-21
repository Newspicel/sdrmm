use std::f64::consts::{FRAC_PI_2, TAU};

use num_complex::Complex;

use super::{
    LoopFilter,
    lock::LockDetector,
    phase::{phase_error, phasor, step_phase, wrap_pi},
};

#[derive(Clone, Debug)]
pub struct Costas {
    filter: LoopFilter,
    lock: LockDetector,
    center: f64,
    phase: f64,
    inc: f64,
}

impl Costas {
    #[must_use]
    pub fn new(loop_bw: f64, damping: f64, center_norm: f64, range_norm: f64) -> Self {
        Self {
            filter: LoopFilter::new(loop_bw, damping, range_norm),
            lock: LockDetector::new(loop_bw, FRAC_PI_2),
            center: TAU * center_norm,
            phase: 0.0,
            inc: 0.0,
        }
    }

    #[inline]
    #[must_use]
    pub fn process(&mut self, sample: Complex<f32>) -> Complex<f32> {
        self.phase = step_phase(self.phase, self.inc);
        let derotated = sample * phasor(self.phase).conj();
        let error = bpsk_error(derotated);
        self.lock.update(error);
        self.inc = wrap_pi(self.center + self.filter.advance(error.unwrap_or(0.0)));
        derotated
    }

    #[must_use]
    pub fn freq_norm(&self) -> f64 {
        self.center / TAU + self.filter.freq_norm()
    }

    #[must_use]
    pub fn lock(&self) -> f32 {
        self.lock.value()
    }
}

fn bpsk_error(derotated: Complex<f32>) -> Option<f64> {
    let symbol = if derotated.re < 0.0 { -1.0 } else { 1.0 };
    phase_error(derotated * symbol)
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_1_SQRT_2;

    use super::*;
    use crate::{
        math::complex::narrow,
        testutil::{XorShift32, complex_tone},
    };

    const DAMPING: f64 = FRAC_1_SQRT_2;
    const OFFSET: f64 = 0.001;

    #[test]
    fn costas_recovers_bpsk_symbols() {
        const SPS: usize = 8;
        const SYMBOLS: usize = 2_000;

        let mut rng = XorShift32(0x1234_5678);
        let symbols: Vec<f64> = (0..SYMBOLS)
            .map(|_| if rng.next_f32() < 0.0 { -1.0 } else { 1.0 })
            .collect();
        let mut costas = Costas::new(0.01, DAMPING, 0.0, 0.01);
        let out: Vec<Complex<f32>> = (0..SYMBOLS * SPS)
            .map(|n| {
                let carrier = Complex::from_polar(1.0, TAU * OFFSET * n as f64 + 0.7);
                costas.process(narrow(carrier * symbols[n / SPS]))
            })
            .collect();

        let settled = SYMBOLS * SPS / 2;
        let correlation: f64 = out[settled..]
            .iter()
            .enumerate()
            .map(|(k, y)| f64::from(y.re) * symbols[(settled + k) / SPS])
            .sum();
        let polarity = if correlation < 0.0 { -1.0 } else { 1.0 };
        for (k, y) in out[settled..].iter().enumerate() {
            let n = settled + k;
            let expected = symbols[n / SPS] * polarity;
            assert!(
                f64::from(y.re) * expected > 0.5,
                "symbol {expected} lost at {n}: {y}"
            );
            assert!(y.im.abs() < 0.25, "quadrature leakage at {n}: {y}");
        }
        assert!(costas.lock() > 0.5, "lock {}", costas.lock());
        assert!(
            (costas.freq_norm() - OFFSET).abs() < 0.01 * OFFSET,
            "freq {}",
            costas.freq_norm()
        );
    }

    #[test]
    fn costas_recovers_after_a_non_finite_sample() {
        let mut costas = Costas::new(0.01, DAMPING, 0.0, 0.01);
        for (n, x) in complex_tone(OFFSET, 20_000).into_iter().enumerate() {
            let poisoned = n == 100;
            let x = if poisoned {
                Complex::new(f32::INFINITY, f32::NAN)
            } else {
                x
            };
            let y = costas.process(x);
            assert!(poisoned || y.is_finite(), "output poisoned at {n}");
        }
        assert!(
            (costas.freq_norm() - OFFSET).abs() < 0.01 * OFFSET,
            "loop stuck at {}",
            costas.freq_norm()
        );
        assert!(costas.lock() > 0.5, "lock {}", costas.lock());
    }
}
