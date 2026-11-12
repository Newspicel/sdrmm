use num_complex::Complex;

use crate::{
    Ddc, DdcError, Interpolator, Nco,
    ddc::STOPBAND_DB,
    fir::{design_lowpass_kaiser, kaiser_taps},
};

const MIN_STAGE_TAPS: usize = 11;
const MAX_BASE_RATIO: f64 = 4.0;

/// Brings a baseband stream up to a radio's rate and places it `offset_hz` from the radio's
/// centre: the rate change first, the shift last, so the offset may exceed the input band.
#[derive(Clone, Debug)]
pub struct Duc {
    base: Ddc,
    doublers: Vec<Interpolator>,
    nco: Nco,
    output_rate: f64,
    work_in: Vec<Complex<f32>>,
    work_out: Vec<Complex<f32>>,
}

impl Duc {
    pub fn new(input_rate: f64, output_rate: f64, offset_hz: f64) -> Result<Self, DdcError> {
        let valid = |rate: f64| rate.is_finite() && rate > 0.0;
        if !valid(input_rate) || !valid(output_rate) {
            return Err(DdcError::InvalidRates {
                input: input_rate,
                output: output_rate,
            });
        }
        let mut doublings = 0u32;
        while output_rate / input_rate / f64::from(1u32 << doublings) >= MAX_BASE_RATIO {
            doublings += 1;
        }
        let base_rate = output_rate / f64::from(1u32 << doublings);
        let doublers = (0..doublings)
            .map(|stage| doubler(input_rate, base_rate * f64::from(1u32 << stage)))
            .collect();
        let mut duc = Self {
            base: Ddc::new(input_rate, base_rate, 0.0)?,
            doublers,
            nco: Nco::new(0.0, output_rate as f32),
            output_rate,
            work_in: Vec::new(),
            work_out: Vec::new(),
        };
        duc.set_offset(offset_hz);
        Ok(duc)
    }

    pub fn set_offset(&mut self, offset_hz: f64) {
        self.nco.set_turns(offset_hz / self.output_rate);
    }

    pub fn reset(&mut self) {
        self.base.reset();
        for doubler in &mut self.doublers {
            doubler.reset();
        }
        self.nco.reset();
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        self.base.process(input, &mut self.work_in);
        for doubler in &mut self.doublers {
            doubler.process(&self.work_in, &mut self.work_out);
            std::mem::swap(&mut self.work_in, &mut self.work_out);
        }
        out.clear();
        if self.nco.is_identity() {
            out.extend_from_slice(&self.work_in);
        } else {
            out.resize(self.work_in.len(), Complex::new(0.0, 0.0));
            self.nco.mix_into(&self.work_in, out);
        }
    }
}

/// A stage by two whose passband is the input band and whose stopband starts where that band's
/// first image does.
fn doubler(band_rate: f64, stage_rate: f64) -> Interpolator {
    let pass = 0.5 * band_rate / (2.0 * stage_rate);
    let transition = 0.5 - 2.0 * pass;
    let taps = kaiser_taps(transition, STOPBAND_DB).max(MIN_STAGE_TAPS);
    Interpolator::new(&design_lowpass_kaiser(taps, 0.25, STOPBAND_DB), 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::rms_c;

    fn tone(freq_hz: f64, rate: f64, len: usize) -> Vec<Complex<f32>> {
        (0..len)
            .map(|n| {
                let phase = std::f64::consts::TAU * freq_hz * n as f64 / rate;
                Complex::new(phase.cos() as f32, phase.sin() as f32)
            })
            .collect()
    }

    fn level_db(samples: &[Complex<f32>], freq_hz: f64, rate: f64) -> f64 {
        let sum: Complex<f64> = samples
            .iter()
            .enumerate()
            .map(|(n, s)| {
                let phase = -std::f64::consts::TAU * freq_hz * n as f64 / rate;
                Complex::new(f64::from(s.re), f64::from(s.im)) * Complex::from_polar(1.0, phase)
            })
            .sum();
        20.0 * (sum.norm() / samples.len() as f64).log10()
    }

    #[test]
    fn a_baseband_tone_lands_at_the_offset_at_full_level() {
        for (input_rate, output_rate, offset) in [
            (48_000.0, 2_400_000.0, 600_000.0),
            (48_000.0, 20_000_000.0, -7_300_000.0),
            (48_000.0, 48_000.0, 0.0),
            (240_000.0, 2_000_000.0, 125_000.0),
            (2_400_000.0, 2_000_000.0, 0.0),
        ] {
            let mut duc = Duc::new(input_rate, output_rate, offset).expect("rates");
            let seconds = 0.05;
            let input = tone(1_000.0, input_rate, (input_rate * seconds) as usize);
            let mut out = Vec::new();
            duc.process(&input, &mut out);
            let ideal = (output_rate * seconds) as i64;
            assert!(
                (out.len() as i64 - ideal).abs() <= 64,
                "{input_rate} to {output_rate}: {} samples, wanted {ideal}",
                out.len()
            );
            let settled = &out[out.len() / 2..];
            let db = level_db(settled, offset + 1_000.0, output_rate);
            assert!(
                db.abs() < 0.1,
                "{input_rate} to {output_rate}: tone {db} dB"
            );
        }
    }

    #[test]
    fn the_images_of_the_input_band_stay_far_down() {
        let (input_rate, output_rate) = (48_000.0, 3_072_000.0);
        let mut duc = Duc::new(input_rate, output_rate, 0.0).expect("rates");
        let input = tone(15_000.0, input_rate, 9_600);
        let mut out = Vec::new();
        duc.process(&input, &mut out);
        let settled = &out[out.len() / 2..];
        for image in [
            15_000.0 - 48_000.0,
            15_000.0 + 48_000.0,
            15_000.0 + 96_000.0,
            15_000.0 - 192_000.0,
            15_000.0 + 768_000.0,
            15_000.0 - 1_536_000.0,
        ] {
            let db = level_db(settled, image, output_rate);
            assert!(db < -80.0, "image at {image} Hz is {db} dB");
        }
    }

    #[test]
    fn ragged_blocks_match_one_pass_and_reset_starts_over() {
        let input = tone(2_500.0, 48_000.0, 4_800);
        let mut whole = Duc::new(48_000.0, 1_920_000.0, 300_000.0).expect("rates");
        let mut ragged = whole.clone();
        let mut expected = Vec::new();
        whole.process(&input, &mut expected);
        let mut got = Vec::new();
        let mut block = Vec::new();
        for chunk in input.chunks(197) {
            ragged.process(chunk, &mut block);
            got.extend_from_slice(&block);
        }
        assert_eq!(expected.len(), got.len());
        for (index, (a, b)) in expected.iter().zip(&got).enumerate() {
            assert!((a - b).norm() < 1e-4, "sample {index}: {a} vs {b}");
        }
        ragged.reset();
        ragged.process(&input, &mut got);
        assert_eq!(expected, got);
    }

    #[test]
    fn silence_in_is_silence_out() {
        let mut duc = Duc::new(48_000.0, 2_400_000.0, 100_000.0).expect("rates");
        let mut out = Vec::new();
        duc.process(&vec![Complex::new(0.0, 0.0); 480], &mut out);
        assert_eq!(rms_c(&out), 0.0);
    }

    #[test]
    fn rates_that_are_not_positive_and_finite_are_refused() {
        for (input, output) in [(0.0, 48_000.0), (48_000.0, f64::NAN), (-1.0, 1.0)] {
            assert!(Duc::new(input, output, 0.0).is_err(), "{input} to {output}");
        }
    }
}
