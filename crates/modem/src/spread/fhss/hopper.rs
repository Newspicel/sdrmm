use std::f64::consts::TAU;

use num_complex::Complex;

use super::sequence::HopSequence;

#[derive(Clone, Debug)]
pub struct FhssMod {
    sequence: HopSequence,
}

#[derive(Clone, Debug)]
pub struct FhssDemod {
    sequence: HopSequence,
}

impl FhssMod {
    #[must_use]
    pub fn new(sequence: HopSequence) -> Self {
        Self { sequence }
    }

    #[must_use]
    pub fn sequence(&self) -> &HopSequence {
        &self.sequence
    }

    pub fn hop(&self, wave: &mut [Complex<f32>]) {
        apply(&self.sequence, wave, 1.0);
    }
}

impl FhssDemod {
    #[must_use]
    pub fn new(sequence: HopSequence) -> Self {
        Self { sequence }
    }

    #[must_use]
    pub fn sequence(&self) -> &HopSequence {
        &self.sequence
    }

    pub fn dehop(&self, wave: &mut [Complex<f32>]) {
        apply(&self.sequence, wave, -1.0);
    }

    #[must_use]
    pub fn dwells_on(&self, channel: usize, hops: usize) -> usize {
        (0..hops)
            .filter(|&hop| self.sequence.channel(hop) == channel)
            .count()
    }
}

fn apply(sequence: &HopSequence, wave: &mut [Complex<f32>], sign: f64) {
    for (index, sample) in wave.iter_mut().enumerate() {
        let hop = index / sequence.dwell_samples();
        let offset = sign * sequence.offset_cycles(sequence.channel(hop));
        let turns = offset * index as f64;
        let (sin, cos) = (TAU * (turns - turns.floor())).sin_cos();
        let re = f64::from(sample.re);
        let im = f64::from(sample.im);
        *sample = Complex::new((re * cos - im * sin) as f32, (re * sin + im * cos) as f32);
    }
}
