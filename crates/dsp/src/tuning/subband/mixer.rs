use num_complex::Complex;

use super::{HALF_BANDS, MIXER_BLOCK, MIXER_PERIOD, MIXER_STEP};

/// A mixer onto one subband's centre, whose carrier repeats every `MIXER_PERIOD` samples and
/// so comes from a table.
#[derive(Clone, Debug)]
pub(super) struct PeriodicMixer {
    carrier: [Complex<f32>; MIXER_PERIOD + MIXER_BLOCK - 1],
    phase: usize,
}

impl PeriodicMixer {
    pub(super) fn new(band: usize) -> Self {
        let frequency =
            -(band as f64 - HALF_BANDS as f64) * MIXER_STEP as f64 / MIXER_PERIOD as f64;
        let carrier = std::array::from_fn(|index| {
            let angle = std::f64::consts::TAU * frequency * (index % MIXER_PERIOD) as f64;
            let (sin, cos) = angle.sin_cos();
            Complex::new(cos as f32, sin as f32)
        });
        Self { carrier, phase: 0 }
    }

    pub(super) fn reset(&mut self) {
        self.phase = 0;
    }

    pub(super) fn mix_into(&mut self, input: &[Complex<f32>], output: &mut [Complex<f32>]) {
        for (input, output) in input
            .chunks(MIXER_BLOCK)
            .zip(output.chunks_mut(MIXER_BLOCK))
        {
            let carrier = &self.carrier[self.phase..self.phase + input.len()];
            for ((input, output), carrier) in input.iter().zip(output).zip(carrier) {
                *output = *input * *carrier;
            }
            self.phase = (self.phase + input.len()) % MIXER_PERIOD;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{testutil::complex_tone, tuning::subband::SUBBANDS, tuning::subband::SubbandPlan};

    #[test]
    fn periodic_mixers_match_the_grid_without_losing_phase() {
        let input = complex_tone(0.017, 4099);
        let mut output = vec![Complex::new(0.0, 0.0); input.len()];
        let plan = SubbandPlan::new(20_000_000.0).unwrap();
        for band in 0..SUBBANDS {
            let mut mixer = PeriodicMixer::new(band);
            let mut at = 0;
            for size in [0, 1, 24, 25, 26, 255, 256, 257, 4099]
                .into_iter()
                .cycle()
                .take(90)
            {
                mixer.mix_into(&input[..size], &mut output[..size]);
                for index in 0..size {
                    let angle = -std::f64::consts::TAU * plan.center(band) / plan.input_rate
                        * (at + index) as f64;
                    let (sin, cos) = angle.sin_cos();
                    let expected = input[index] * Complex::new(cos as f32, sin as f32);
                    assert!((output[index] - expected).norm() < 2e-6);
                }
                at += size;
            }
        }
    }
}
