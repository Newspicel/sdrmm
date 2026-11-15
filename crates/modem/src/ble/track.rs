mod acquire;
mod blocks;
mod coherent;

use num_complex::Complex;

pub(crate) use self::{
    acquire::{Acquired, acquire},
    blocks::{Blocks, carrier_from_pilot, smoothed_soft},
    coherent::Coherent,
};

const HALF_SAMPLE: f64 = 0.5;
const MAX_TIMING_STEP: f64 = 0.5;
const QUARTER: Complex<f32> = Complex::new(0.0, 1.0);

pub(crate) fn pseudo_symbols(symbols: impl Iterator<Item = bool>, out: &mut Vec<Complex<f32>>) {
    let mut current = Complex::new(1.0, 0.0);
    for symbol in symbols {
        current *= if symbol { QUARTER } else { -QUARTER };
        out.push(current);
    }
}

#[derive(Clone, Copy)]
pub(crate) struct View<'a> {
    y: &'a [Complex<f32>],
    origin: f64,
    omega: f32,
}

impl<'a> View<'a> {
    pub(crate) fn new(y: &'a [Complex<f32>]) -> Self {
        Self {
            y,
            origin: 0.0,
            omega: 0.0,
        }
    }

    fn raw(&self, t: f64) -> Option<Complex<f32>> {
        if t < 0.0 {
            return None;
        }
        let index = t.floor();
        let weight = (t - index) as f32;
        let index = index as usize;
        Some(self.y.get(index)? * (1.0 - weight) + self.y.get(index + 1)? * weight)
    }

    pub(crate) fn at(&self, t: f64) -> Option<Complex<f32>> {
        let turn = -self.omega * (t - self.origin) as f32;
        Some(self.raw(t)? * Complex::from_polar(1.0, turn))
    }
}
