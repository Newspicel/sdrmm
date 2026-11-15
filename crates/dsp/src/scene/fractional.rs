use num_complex::Complex;

use super::delay::{self, KAISER_BETA, KaiserSinc};
use super::history::History;

pub(super) const DELAY_TAPS: usize = 129;
pub(super) const DELAY_CENTRE: i64 = 64;
pub(super) const ECHO_FRACTIONS: usize = 256;

type C64 = Complex<f64>;

/// A fixed delay in samples: a whole-sample shift plus, when needed, a Kaiser-windowed sinc
/// for the fraction.
#[derive(Clone, Debug)]
pub(super) struct DelayPlan {
    pub(super) delay: f64,
    pub(super) whole: i64,
    taps: Option<Vec<f64>>,
}

impl DelayPlan {
    pub(super) fn new(delay: f64) -> Self {
        let whole = delay.floor();
        let fraction = delay - whole;
        let taps = (fraction > 0.0).then(|| fractional_taps(fraction));
        Self {
            delay,
            whole: whole as i64,
            taps,
        }
    }

    pub(super) fn reach(&self, n: i64) -> i64 {
        let reach = n - self.whole;
        if self.taps.is_some() {
            reach + DELAY_CENTRE
        } else {
            reach
        }
    }

    pub(super) fn read(&self, history: &History, n: i64) -> C64 {
        let at = n - self.whole;
        match &self.taps {
            None => history.get(at),
            Some(taps) => history.filtered(at, taps, DELAY_CENTRE),
        }
    }
}

/// Fractional-delay filters for every step of `1 / ECHO_FRACTIONS` sample.
pub(super) fn fraction_table() -> Vec<Vec<f64>> {
    (0..ECHO_FRACTIONS)
        .map(|step| fractional_taps(step as f64 / ECHO_FRACTIONS as f64))
        .collect()
}

/// Sample `n` delayed by `delay`, rounded to the nearest step of the fraction table.
pub(super) fn read_quantised(history: &History, n: i64, delay: f64, fractions: &[Vec<f64>]) -> C64 {
    let steps = ECHO_FRACTIONS as f64;
    let quantised = (delay * steps).round() / steps;
    let whole = quantised.floor();
    let step = ((quantised - whole) * steps).round() as usize;
    let at = n - whole as i64;
    match fractions.get(step).filter(|_| step > 0) {
        None => history.get(at),
        Some(taps) => history.filtered(at, taps, DELAY_CENTRE),
    }
}

fn fractional_taps(fraction: f64) -> Vec<f64> {
    let centre = DELAY_CENTRE as f64 + fraction;
    let design = KaiserSinc::new(DELAY_CENTRE as f64 + 1.0, KAISER_BETA);
    let mut taps: Vec<f64> = (0..DELAY_TAPS)
        .map(|k| design.tap(k as f64 - centre))
        .collect();
    delay::normalise(&mut taps);
    taps
}
