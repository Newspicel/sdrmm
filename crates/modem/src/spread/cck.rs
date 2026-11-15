mod codebook;
mod demod;
mod modulator;
mod params;
#[cfg(test)]
mod tests;
mod tracker;

use std::f32::consts::FRAC_1_SQRT_2;

pub use self::{codebook::Codebook, demod::CckDemod, modulator::CckMod, params::CckParams};

pub const TRACK_BW: f64 = 0.02;

pub const CHIPS: usize = 8;

pub const MAX_WORDS: usize = 64;

const MAX_BITS: usize = 8;

pub const CHIP_AMPLITUDE: f32 = FRAC_1_SQRT_2 * 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CckMode {
    Bits4,
    Bits8,
}

impl CckMode {
    #[must_use]
    pub fn bits_per_symbol(self) -> usize {
        match self {
            Self::Bits4 => 4,
            Self::Bits8 => 8,
        }
    }

    #[must_use]
    pub fn words(self) -> usize {
        1 << (self.bits_per_symbol() - 2)
    }

    #[must_use]
    pub fn alphabet(self) -> u32 {
        1 << self.bits_per_symbol()
    }
}
