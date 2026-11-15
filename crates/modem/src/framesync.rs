mod rotation;

use std::f64::consts::TAU;

use num_complex::Complex;

pub use rotation::{conj_product, derotate, derotate_from, rotor};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Repetition {
    pub offset: usize,
    pub cfo: f64,
    pub metric: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RepetitionDetector {
    period: usize,
    window: usize,
}

impl RepetitionDetector {
    #[must_use]
    pub fn new(period: usize, window: usize) -> Self {
        assert!(
            period > 0 && window > 0,
            "a repetition metric needs a period and a window"
        );
        Self { period, window }
    }

    #[must_use]
    pub fn period(&self) -> usize {
        self.period
    }

    #[must_use]
    pub fn window(&self) -> usize {
        self.window
    }

    #[must_use]
    pub fn detect(&self, x: &[Complex<f32>], search: usize) -> Option<Repetition> {
        let needed = self.window + self.period;
        if x.len() < needed {
            return None;
        }
        let last = search.min(x.len() - needed);
        let mut p = Complex::new(0.0f64, 0.0);
        let mut r = 0.0f64;
        for n in 0..self.window {
            p += conj_product(x[n], x[n + self.period]);
            r += f64::from(x[n + self.period].norm_sqr());
        }
        let mut best = (0usize, p, f64::NEG_INFINITY);
        for d in 0..=last {
            if d > 0 {
                let out = d - 1;
                let into = d - 1 + self.window;
                p -= conj_product(x[out], x[out + self.period]);
                p += conj_product(x[into], x[into + self.period]);
                r -= f64::from(x[out + self.period].norm_sqr());
                r += f64::from(x[into + self.period].norm_sqr());
            }
            let metric = if r > 0.0 { p.norm_sqr() / (r * r) } else { 0.0 };
            if metric > best.2 {
                best = (d, p, metric);
            }
        }
        let (offset, correlation, metric) = best;
        Some(Repetition {
            offset,
            cfo: correlation.arg() / (TAU * self.period as f64),
            metric,
        })
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::rng::Rng;

    use super::*;

    fn periodic(lead: usize, period: usize, repeats: usize, cfo: f64) -> Vec<Complex<f32>> {
        let mut rng = Rng::mixed(0x51c3);
        let mut unit = || Complex::from_polar(1.0, (TAU * rng.uniform()) as f32);
        let base: Vec<Complex<f32>> = (0..period).map(|_| unit()).collect();
        let mut x: Vec<Complex<f32>> = (0..lead).map(|_| unit()).collect();
        for _ in 0..repeats {
            x.extend_from_slice(&base);
        }
        x.extend((0..2 * period).map(|_| unit()));
        for (n, v) in x.iter_mut().enumerate() {
            *v *= Complex::from_polar(1.0, (TAU * cfo * n as f64) as f32);
        }
        x
    }

    #[test]
    fn the_metric_finds_the_repetition_and_its_phase_step() {
        let detector = RepetitionDetector::new(20, 20);
        let x = periodic(37, 20, 2, 0.011);
        let found = detector.detect(&x, 100).unwrap();
        assert_eq!(found.offset, 37);
        assert!(found.metric > 0.99);
        assert!((found.cfo - 0.011).abs() < 1e-6, "cfo {}", found.cfo);
    }
}
