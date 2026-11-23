use num_complex::Complex;

use crate::filter::iir::one_pole_coeff;

#[derive(Clone, Debug)]
pub struct NoiseBlanker {
    delay: Vec<Complex<f32>>,
    write: usize,
    average: f32,
    coeff: f32,
    threshold: f32,
    remaining: usize,
    window: usize,
    blanked: u64,
}

impl NoiseBlanker {
    const HALF_WINDOW_S: f64 = 40e-6;
    const AVERAGE_TAU_S: f64 = 0.05;

    #[must_use]
    pub fn new(rate: f64, threshold: f32) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let half = ((rate * Self::HALF_WINDOW_S).round() as usize).max(1);
        Self {
            delay: vec![Complex::new(0.0, 0.0); half],
            write: 0,
            average: 0.0,
            coeff: one_pole_coeff(rate, Self::AVERAGE_TAU_S),
            threshold: threshold.max(1.0),
            remaining: 0,
            window: 2 * half + 1,
            blanked: 0,
        }
    }

    pub fn set_threshold(&mut self, threshold: f32) {
        self.threshold = threshold.max(1.0);
    }

    #[must_use]
    pub fn blanked_samples(&self) -> u64 {
        self.blanked
    }

    pub fn reset(&mut self) {
        self.delay.fill(Complex::new(0.0, 0.0));
        self.write = 0;
        self.average = 0.0;
        self.remaining = 0;
    }

    fn track(&mut self, magnitude: f32) {
        if self.average <= 0.0 {
            self.average = magnitude;
        } else {
            self.average += self.coeff * (magnitude - self.average);
        }
    }

    fn is_impulse(&self, magnitude: f32) -> bool {
        magnitude > self.threshold * self.average.max(f32::MIN_POSITIVE)
    }

    /// A burst still loud at the end of its window is a level step (a retune, a stronger
    /// station), not an impulse, so the average jumps to it instead of blanking on.
    pub fn process(&mut self, iq: &mut [Complex<f32>]) {
        if !self.average.is_finite() {
            self.reset();
        }
        for sample in iq {
            let input = if sample.re.is_finite() && sample.im.is_finite() {
                *sample
            } else {
                Complex::new(0.0, 0.0)
            };
            let delayed = self.delay[self.write];
            self.delay[self.write] = input;
            self.write += 1;
            if self.write == self.delay.len() {
                self.write = 0;
            }

            let magnitude = input.norm();
            if self.remaining == 0 {
                self.track(magnitude);
                if self.is_impulse(magnitude) {
                    self.remaining = self.window;
                }
            } else if self.remaining == 1 && self.is_impulse(magnitude) {
                self.average = magnitude;
            }
            if self.remaining > 0 {
                self.remaining -= 1;
                self.blanked += 1;
                *sample = Complex::new(0.0, 0.0);
            } else {
                *sample = delayed;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::noise::testing::RATE,
        testutil::{XorShift32, rms_c},
    };

    fn noise_iq(seed: u32, amplitude: f32, len: usize) -> Vec<Complex<f32>> {
        let mut rng = XorShift32(seed);
        (0..len)
            .map(|_| Complex::new(rng.next_f32() * amplitude, rng.next_f32() * amplitude))
            .collect()
    }

    fn carrier(len: usize) -> Vec<Complex<f32>> {
        (0..len)
            .map(|n| Complex::from_polar(0.1, (n as f32) * 0.01))
            .collect()
    }

    fn with_impulses(len: usize, period: usize, amplitude: f32) -> Vec<Complex<f32>> {
        let mut iq = carrier(len);
        for n in (period..len).step_by(period) {
            iq[n] = Complex::new(amplitude, 0.0);
        }
        iq
    }

    #[test]
    fn blanker_cuts_impulses_out_and_leaves_the_carrier() {
        let mut blanked = with_impulses(24_000, 500, 5.0);
        NoiseBlanker::new(RATE, 4.0).process(&mut blanked);

        let settled = 4_000;
        let peak = blanked[settled..]
            .iter()
            .map(|s| s.norm())
            .fold(0.0f32, f32::max);
        assert!(peak < 0.2, "impulse survived at {peak}");
        let kept = rms_c(&blanked[settled..]);
        let original = rms_c(&carrier(24_000)[settled..]);
        assert!(
            kept > 0.9 * original,
            "carrier lost: {kept} of {original} left"
        );
        assert!(kept < 1.1 * original);
    }

    #[test]
    fn blanker_leaves_a_clean_channel_alone() {
        let clean = noise_iq(0x51D3_2AA1, 0.2, 48_000);
        let mut through = clean.clone();
        let mut blanker = NoiseBlanker::new(RATE, 6.0);
        blanker.process(&mut through);
        let before = rms_c(&clean[8_000..]);
        let after = rms_c(&through[8_000..]);
        assert!(
            (after / before - 1.0).abs() < 0.05,
            "an undisturbed channel was chewed up: {before} -> {after}"
        );
    }

    #[test]
    fn a_lower_threshold_blanks_more() {
        let iq = with_impulses(48_000, 300, 0.5);
        let counts: Vec<u64> = [2.5f32, 8.0]
            .iter()
            .map(|&t| {
                let mut blanker = NoiseBlanker::new(RATE, t);
                blanker.process(&mut iq.clone());
                blanker.blanked_samples()
            })
            .collect();
        assert!(counts[0] > counts[1], "{counts:?}");
    }

    #[test]
    fn a_level_step_is_followed_not_blanked() {
        let mut iq = carrier(48_000);
        for sample in &mut iq[..4_000] {
            *sample *= 0.01;
        }
        let mut blanker = NoiseBlanker::new(RATE, 5.0);
        blanker.process(&mut iq);
        assert!(
            blanker.blanked_samples() < 200,
            "blanked {} samples after a level step",
            blanker.blanked_samples()
        );
        assert!(rms_c(&iq[8_000..]) > 0.09, "the louder signal stayed muted");
    }

    #[test]
    fn blanker_recovers_after_a_non_finite_sample() {
        let mut blanker = NoiseBlanker::new(RATE, 4.0);
        let mut poisoned = vec![Complex::new(f32::NAN, 0.0); 64];
        poisoned[10] = Complex::new(f32::INFINITY, 0.0);
        blanker.process(&mut poisoned);
        assert!(
            poisoned
                .iter()
                .all(|s| s.re.is_finite() && s.im.is_finite())
        );
        let mut back = carrier(24_000);
        blanker.process(&mut back);
        assert!(rms_c(&back[8_000..]) > 0.09, "channel stayed muted");
    }
}
