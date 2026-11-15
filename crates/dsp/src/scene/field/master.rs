use std::f32::consts::FRAC_1_SQRT_2;

use num_complex::Complex;

use super::sinc_table::SincTable;
use super::{TAP_LEAD, TAPS};
use crate::math::rng::{Rng, mix};

const MASTER_BLOCK: usize = 4_096;
const MASTER_BLOCKS: usize = 3;

type C32 = Complex<f32>;
type C64 = Complex<f64>;

/// Seeded random symbols at their own rate, regenerated block by block so any time can be
/// read back, interpolated to any instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Symbols {
    Gaussian,
    Binary,
}

pub(super) struct Master {
    seed: u64,
    rate: f64,
    symbols: Symbols,
    first_block: i64,
    filled: bool,
    data: Vec<C32>,
}

impl Master {
    pub(super) fn new(seed: u64, rate: f64, symbols: Symbols) -> Self {
        Self {
            seed,
            rate,
            symbols,
            first_block: 0,
            filled: false,
            data: vec![C32::new(0.0, 0.0); MASTER_BLOCK * MASTER_BLOCKS],
        }
    }

    pub(super) fn at(&mut self, time_s: f64) -> C64 {
        let x = time_s * self.rate;
        let floor = x.floor();
        let start = floor as i64 - TAP_LEAD;
        self.ensure(start);
        let offset = (start - self.first_block * MASTER_BLOCK as i64) as usize;
        SincTable::shared().interpolate(&self.data[offset..offset + TAPS], x - floor)
    }

    fn ensure(&mut self, start: i64) {
        let block_len = MASTER_BLOCK as i64;
        let block = start.div_euclid(block_len);
        let last = (start + TAPS as i64 - 1).div_euclid(block_len);
        let span = MASTER_BLOCKS as i64;
        if self.filled && block >= self.first_block && last < self.first_block + span {
            return;
        }
        let shift = block - self.first_block;
        let kept = if self.filled && (1..span).contains(&shift) {
            let shift = shift as usize;
            self.data.copy_within(shift * MASTER_BLOCK.., 0);
            MASTER_BLOCKS - shift
        } else {
            0
        };
        for slot in kept..MASTER_BLOCKS {
            self.fill(slot, block + slot as i64);
        }
        self.first_block = block;
        self.filled = true;
    }

    fn fill(&mut self, slot: usize, block: i64) {
        let mut rng = Rng::mixed(mix(self.seed ^ mix(block as u64)));
        let symbols = self.symbols;
        for value in &mut self.data[slot * MASTER_BLOCK..(slot + 1) * MASTER_BLOCK] {
            *value = match symbols {
                Symbols::Gaussian => rng.normal_pair(FRAC_1_SQRT_2),
                Symbols::Binary if rng.bits() >> 63 == 0 => C32::new(1.0, 0.0),
                Symbols::Binary => C32::new(-1.0, 0.0),
            };
        }
    }
}
