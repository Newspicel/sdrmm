use num_complex::Complex;

use super::{
    super::framing_overhead_db, MAX_SPREADING_FACTOR, MIN_SPREADING_FACTOR, chirp_turns, phasor,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssParams {
    spreading_factor: u32,
}

impl CssParams {
    #[must_use]
    pub fn new(spreading_factor: u32) -> Self {
        assert!(
            (MIN_SPREADING_FACTOR..=MAX_SPREADING_FACTOR).contains(&spreading_factor),
            "spreading factor {spreading_factor} is outside \
             {MIN_SPREADING_FACTOR}..={MAX_SPREADING_FACTOR}"
        );
        Self { spreading_factor }
    }

    #[must_use]
    pub fn spreading_factor(&self) -> u32 {
        self.spreading_factor
    }

    #[must_use]
    pub fn chips(&self) -> usize {
        1 << self.spreading_factor
    }

    #[must_use]
    pub fn alphabet(&self) -> usize {
        self.chips()
    }

    #[must_use]
    pub fn bits_per_symbol(&self) -> usize {
        self.spreading_factor as usize
    }

    #[must_use]
    pub fn framing_overhead_db(preamble: usize, payload: usize) -> f64 {
        framing_overhead_db(preamble, payload)
    }

    #[must_use]
    pub fn base_chirp(&self) -> Vec<Complex<f32>> {
        let n = self.chips() as f64;
        (0..self.chips())
            .map(|k| phasor(chirp_turns(k as f64, n)))
            .collect()
    }
}
