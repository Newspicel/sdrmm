use std::f64::consts::FRAC_1_SQRT_2;

use sdrmm_dsp::LoopFilter;

const TIMING_STEP_LIMIT: f64 = 0.05;

#[derive(Clone, Debug)]
pub struct ChipTiming {
    offset: f64,
    half_chip: f64,
    filter: LoopFilter,
}

impl ChipTiming {
    #[must_use]
    pub fn new(sps: usize, loop_bw: f64) -> Self {
        Self {
            offset: 0.0,
            half_chip: sps as f64 / 2.0,
            filter: LoopFilter::new(loop_bw, FRAC_1_SQRT_2, TIMING_STEP_LIMIT),
        }
    }

    #[must_use]
    pub fn offset(&self) -> f64 {
        self.offset
    }

    #[must_use]
    pub fn early(&self) -> f64 {
        self.offset - self.half_chip
    }

    #[must_use]
    pub fn late(&self) -> f64 {
        self.offset + self.half_chip
    }

    pub fn update(&mut self, early_energy: f32, late_energy: f32) {
        let total = f64::from(early_energy + late_energy);
        if total <= 0.0 || !total.is_finite() {
            return;
        }
        let error = f64::from(late_energy - early_energy) / total * self.half_chip;
        self.offset += self.filter.advance(error);
    }
}
