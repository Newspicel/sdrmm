use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl Biquad {
    const MIN_Q: f64 = 0.05;
    /// The lowest centre frequency, as a share of the rate.
    const MIN_FREQ: f64 = 1e-4;
    /// The highest centre frequency, as a share of the Nyquist rate.
    const MAX_FREQ: f64 = 0.995;

    fn from_unnormalized(b: [f64; 3], a: [f64; 3]) -> Self {
        let a0 = if a[0].abs() > f64::MIN_POSITIVE {
            a[0]
        } else {
            1.0
        };
        Self {
            b0: (b[0] / a0) as f32,
            b1: (b[1] / a0) as f32,
            b2: (b[2] / a0) as f32,
            a1: (a[1] / a0) as f32,
            a2: (a[2] / a0) as f32,
            z1: 0.0,
            z2: 0.0,
        }
    }

    fn geometry(rate: f64, freq_hz: f64, q: f64) -> (f64, f64, f64) {
        let nyquist = rate / 2.0;
        let freq = freq_hz.clamp(rate * Self::MIN_FREQ, nyquist * Self::MAX_FREQ);
        let w0 = TAU * freq / rate;
        let alpha = w0.sin() / (2.0 * q.max(Self::MIN_Q));
        (w0.cos(), w0.sin(), alpha)
    }

    #[must_use]
    pub fn lowpass(rate: f64, freq_hz: f64, q: f64) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let (cos_w0, _, alpha) = Self::geometry(rate, freq_hz, q);
        Self::from_unnormalized(
            [(1.0 - cos_w0) / 2.0, 1.0 - cos_w0, (1.0 - cos_w0) / 2.0],
            [1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha],
        )
    }

    #[must_use]
    pub fn highpass(rate: f64, freq_hz: f64, q: f64) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let (cos_w0, _, alpha) = Self::geometry(rate, freq_hz, q);
        Self::from_unnormalized(
            [(1.0 + cos_w0) / 2.0, -(1.0 + cos_w0), (1.0 + cos_w0) / 2.0],
            [1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha],
        )
    }

    #[must_use]
    pub fn notch(rate: f64, freq_hz: f64, q: f64) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let (cos_w0, _, alpha) = Self::geometry(rate, freq_hz, q);
        Self::from_unnormalized(
            [1.0, -2.0 * cos_w0, 1.0],
            [1.0 + alpha, -2.0 * cos_w0, 1.0 - alpha],
        )
    }

    pub fn reset(&mut self) {
        self.z1 = 0.0;
        self.z2 = 0.0;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !(self.z1.is_finite() && self.z2.is_finite()) {
            self.reset();
        }
        for s in samples {
            let x = *s;
            let y = self.b0 * x + self.z1;
            self.z1 = self.b1 * x - self.a1 * y + self.z2;
            self.z2 = self.b2 * x - self.a2 * y;
            *s = y;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;

    use super::*;
    use crate::testutil::{real_tone, rms_r};

    fn biquad_gain(mut filter: Biquad, freq_hz: f64) -> f32 {
        let rate = 48_000.0;
        let n = (rate * 0.5) as usize;
        let mut x = real_tone(freq_hz / rate, n);
        filter.process(&mut x);
        rms_r(&x[n / 2..]) / FRAC_1_SQRT_2
    }

    #[test]
    fn biquad_corners_are_3_db_down_and_roll_off_at_12_db_per_octave() {
        let rate = 48_000.0;
        let q = std::f64::consts::FRAC_1_SQRT_2;
        for corner in [300.0f64, 3_000.0] {
            let at = biquad_gain(Biquad::lowpass(rate, corner, q), corner);
            assert!(
                (at - FRAC_1_SQRT_2).abs() < 0.03,
                "lowpass {corner} Hz: {at}"
            );
            let octave = biquad_gain(Biquad::lowpass(rate, corner, q), corner * 2.0);
            assert!((0.2..0.28).contains(&octave), "lowpass octave up: {octave}");

            let at = biquad_gain(Biquad::highpass(rate, corner, q), corner);
            assert!(
                (at - FRAC_1_SQRT_2).abs() < 0.03,
                "highpass {corner} Hz: {at}"
            );
            let octave = biquad_gain(Biquad::highpass(rate, corner, q), corner / 2.0);
            assert!(
                (0.2..0.28).contains(&octave),
                "highpass octave down: {octave}"
            );
        }
    }

    #[test]
    fn biquad_notch_removes_its_tone_and_leaves_its_neighbours() {
        let rate = 48_000.0;
        let notch = || Biquad::notch(rate, 1_000.0, 1_000.0 / 60.0);
        assert!(biquad_gain(notch(), 1_000.0) < 0.02);
        assert!(biquad_gain(notch(), 700.0) > 0.9);
        assert!(biquad_gain(notch(), 1_500.0) > 0.9);
    }

    #[test]
    fn biquad_recovers_after_a_non_finite_sample() {
        let mut filter = Biquad::lowpass(48_000.0, 3_000.0, std::f64::consts::FRAC_1_SQRT_2);
        let mut poisoned = vec![0.5f32; 480];
        poisoned[3] = f32::NAN;
        filter.process(&mut poisoned);
        let mut tone = real_tone(1_000.0 / 48_000.0, 24_000);
        filter.process(&mut tone);
        assert!(tone.iter().all(|v| v.is_finite()), "state still poisoned");
        let gain = rms_r(&tone[12_000..]) / FRAC_1_SQRT_2;
        assert!((0.9..1.05).contains(&gain), "post-recovery gain {gain}");
    }

    #[test]
    fn biquad_stays_stable_at_degenerate_settings() {
        for (freq_hz, q) in [(0.0f64, 0.0f64), (48_000.0, 0.0), (-100.0, -5.0)] {
            let mut filter = Biquad::notch(48_000.0, freq_hz, q);
            let mut x = real_tone(1_000.0 / 48_000.0, 48_000);
            filter.process(&mut x);
            assert!(
                x.iter().all(|v| v.is_finite() && v.abs() < 10.0),
                "{freq_hz} Hz at q {q} diverged"
            );
        }
    }
}
