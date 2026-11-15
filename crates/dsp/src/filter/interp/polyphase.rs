use num_complex::Complex;

#[derive(Clone, Debug)]
pub struct RealInterpolator {
    phases: Vec<Vec<f32>>,
    history: Vec<f32>,
}

impl RealInterpolator {
    #[must_use]
    pub fn new(taps: &[f32], factor: usize) -> Self {
        let phases = split_phases(taps, factor);
        Self {
            history: vec![0.0; phases[0].len()],
            phases,
        }
    }

    pub fn reset(&mut self) {
        self.history.fill(0.0);
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        out.clear();
        let len = self.history.len();
        for &sample in input {
            self.history.copy_within(1.., 0);
            self.history[len - 1] = sample;
            for phase in &self.phases {
                out.push(
                    phase
                        .iter()
                        .zip(self.history.iter().rev())
                        .map(|(h, x)| h * x)
                        .sum(),
                );
            }
        }
    }
}

/// The complex counterpart of [`RealInterpolator`]: zero-stuffs by `factor` and filters the images.
#[derive(Clone, Debug)]
pub struct Interpolator {
    phases: Vec<Vec<f32>>,
    history: Vec<Complex<f32>>,
}

impl Interpolator {
    #[must_use]
    pub fn new(taps: &[f32], factor: usize) -> Self {
        let phases = split_phases(taps, factor);
        Self {
            history: vec![Complex::new(0.0, 0.0); phases[0].len()],
            phases,
        }
    }

    pub fn reset(&mut self) {
        self.history.fill(Complex::new(0.0, 0.0));
    }

    pub fn process(&mut self, input: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        out.clear();
        out.reserve(input.len() * self.phases.len());
        let len = self.history.len();
        for &sample in input {
            self.history.copy_within(1.., 0);
            self.history[len - 1] = sample;
            for phase in &self.phases {
                let mut sum = Complex::new(0.0, 0.0);
                for (h, x) in phase.iter().zip(self.history.iter().rev()) {
                    sum += x * *h;
                }
                out.push(sum);
            }
        }
    }
}

/// The `factor` phases of `taps`, zero-padded to one length and scaled by `factor` so the
/// zero-stuffed stream keeps its level.
fn split_phases(taps: &[f32], factor: usize) -> Vec<Vec<f32>> {
    assert!(factor >= 1, "factor must be >= 1");
    assert!(taps.len() >= factor, "need at least one tap per phase");
    let per_phase = taps.len().div_ceil(factor);
    let gain = factor as f32;
    (0..factor)
        .map(|phase| {
            (0..per_phase)
                .map(|k| taps.get(phase + k * factor).copied().unwrap_or(0.0) * gain)
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        design_lowpass,
        filter::fir::design_lowpass_kaiser,
        testutil::{complex_tone, real_tone, rms_r},
    };

    #[test]
    fn a_real_interpolator_keeps_an_in_band_tone_and_its_level() {
        let taps = design_lowpass(96, 0.15);
        let mut up = RealInterpolator::new(&taps, 3);
        let input = real_tone(0.05, 4_000);
        let mut out = Vec::new();
        up.process(&input, &mut out);
        assert_eq!(out.len(), 3 * input.len());
        let level = rms_r(&out[600..]);
        assert!((0.68..0.73).contains(&level), "interpolated rms {level}");
    }

    #[test]
    fn a_real_interpolator_suppresses_the_images_it_makes() {
        let taps = design_lowpass(96, 0.15);
        let mut up = RealInterpolator::new(&taps, 3);
        let input = real_tone(0.1, 6_000);
        let mut out = Vec::new();
        up.process(&input, &mut out);
        let settled = &out[600..600 + 4_096];
        let image = settled
            .iter()
            .enumerate()
            .map(|(n, &x)| {
                let p = std::f64::consts::TAU * (1.0 / 3.0 - 0.1 / 3.0) * n as f64;
                Complex::new(x * p.cos() as f32, -x * p.sin() as f32)
            })
            .sum::<Complex<f32>>()
            .norm()
            / settled.len() as f32;
        assert!(image < 1e-3, "image leaked at {image}");
    }

    #[test]
    fn a_complex_interpolator_keeps_the_tone_and_stops_its_image() {
        let taps = design_lowpass_kaiser(63, 0.25, 80.0);
        let mut up = Interpolator::new(&taps, 2);
        let input = complex_tone(0.05, 6_000);
        let mut out = Vec::new();
        up.process(&input, &mut out);
        assert_eq!(out.len(), 2 * input.len());
        let settled = &out[200..200 + 4_096];
        let level = |turns: f64| {
            settled
                .iter()
                .enumerate()
                .map(|(n, x)| {
                    let p = -std::f64::consts::TAU * turns * n as f64;
                    x * Complex::new(p.cos() as f32, p.sin() as f32)
                })
                .sum::<Complex<f32>>()
                .norm()
                / settled.len() as f32
        };
        assert!(
            (level(0.025) - 1.0).abs() < 1e-2,
            "tone at {}",
            level(0.025)
        );
        assert!(level(0.525) < 1e-3, "image at {}", level(0.525));
    }

    #[test]
    fn a_complex_interpolator_gives_the_same_result_in_ragged_blocks() {
        let taps = design_lowpass_kaiser(31, 0.25, 80.0);
        let input = complex_tone(0.07, 3_000);
        let mut whole = Interpolator::new(&taps, 2);
        let mut expected = Vec::new();
        whole.process(&input, &mut expected);
        let mut ragged = Interpolator::new(&taps, 2);
        let mut got = Vec::new();
        let mut block = Vec::new();
        for chunk in input.chunks(97) {
            ragged.process(chunk, &mut block);
            got.extend_from_slice(&block);
        }
        assert_eq!(expected, got);
    }

    #[test]
    fn a_real_interpolator_gives_the_same_result_in_ragged_blocks() {
        let taps = design_lowpass(96, 0.15);
        let input = real_tone(0.07, 3_000);
        let mut whole = RealInterpolator::new(&taps, 3);
        let mut expected = Vec::new();
        whole.process(&input, &mut expected);
        let mut ragged = RealInterpolator::new(&taps, 3);
        let mut got = Vec::new();
        let mut block = Vec::new();
        for chunk in input.chunks(97) {
            ragged.process(chunk, &mut block);
            got.extend_from_slice(&block);
        }
        assert_eq!(expected, got);
    }
}
