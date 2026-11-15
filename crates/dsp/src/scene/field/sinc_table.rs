use std::sync::OnceLock;

use num_complex::Complex;

use super::{TAP_LEAD, TAPS};
use crate::scene::delay::{self, KAISER_BETA, KaiserSinc};

const PHASES: usize = 1_024;

type C32 = Complex<f32>;
type C64 = Complex<f64>;

/// Kaiser-windowed sinc interpolator, tabulated at `PHASES` fractional positions.
pub(super) struct SincTable {
    rows: Vec<[f32; TAPS]>,
}

impl SincTable {
    pub(super) fn shared() -> &'static Self {
        static TABLE: OnceLock<SincTable> = OnceLock::new();
        TABLE.get_or_init(Self::build)
    }

    fn build() -> Self {
        let design = KaiserSinc::new(TAPS as f64 / 2.0, KAISER_BETA);
        let rows = (0..=PHASES)
            .map(|phase| {
                let mu = phase as f64 / PHASES as f64;
                let mut values = [0.0f64; TAPS];
                for (tap, value) in values.iter_mut().enumerate() {
                    *value = design.tap(tap as f64 - TAP_LEAD as f64 - mu);
                }
                delay::normalise(&mut values);
                values.map(|value| value as f32)
            })
            .collect();
        Self { rows }
    }

    pub(super) fn interpolate(&self, window: &[C32], mu: f64) -> C64 {
        let phase = ((mu * PHASES as f64).round() as usize).min(PHASES);
        let row = &self.rows[phase];
        let (mut re, mut im) = (0.0f32, 0.0f32);
        for (value, tap) in window.iter().zip(row) {
            re += value.re * tap;
            im += value.im * tap;
        }
        C64::new(f64::from(re), f64::from(im))
    }
}
