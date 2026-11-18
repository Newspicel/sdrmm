use super::one_pole_coeff;

/// Microseconds to seconds.
const SECONDS_PER_US: f64 = 1e-6;

#[derive(Clone, Debug)]
pub struct Deemphasis {
    state: f32,
    coeff: f32,
}

impl Deemphasis {
    #[must_use]
    pub fn new(rate: f64, tau_us: f32) -> Self {
        assert!(rate > 0.0 && tau_us > 0.0, "rate and tau must be positive");
        Self {
            state: 0.0,
            coeff: one_pole_coeff(rate, f64::from(tau_us) * SECONDS_PER_US),
        }
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.state.is_finite() {
            self.state = 0.0;
        }
        for s in samples {
            self.state += self.coeff * (*s - self.state);
            *s = self.state;
        }
    }
}

/// The gain [`Deemphasis`] gives a tone at `hz`.
#[must_use]
pub fn deemphasis_gain(rate: f64, tau_us: f32, hz: f64) -> f32 {
    let pole = f64::from(one_pole_coeff(rate, f64::from(tau_us) * SECONDS_PER_US));
    let kept = 1.0 - pole;
    let turn = std::f64::consts::TAU * hz / rate;
    (pole / (1.0 - kept * turn.cos()).hypot(kept * turn.sin())) as f32
}

/// The inverse of [`Deemphasis`], lifting highs at 6 dB per octave, scaled to unity gain at
/// `reference_hz`. Non-finite input is read as silence.
#[derive(Clone, Debug)]
pub struct Preemphasis {
    kept: f32,
    scale: f32,
    previous: f32,
}

impl Preemphasis {
    #[must_use]
    pub fn new(rate: f64, tau_us: f32, reference_hz: f64) -> Self {
        let pole = one_pole_coeff(rate, f64::from(tau_us) * SECONDS_PER_US);
        Self {
            kept: 1.0 - pole,
            scale: deemphasis_gain(rate, tau_us, reference_hz) / pole,
            previous: 0.0,
        }
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.previous.is_finite() {
            self.previous = 0.0;
        }
        for s in samples {
            let sample = if s.is_finite() { *s } else { 0.0 };
            *s = self.scale * (sample - self.kept * self.previous);
            self.previous = sample;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_1_SQRT_2;

    use super::*;

    fn tone_gain(rate: f64, hz: f64, mut filter: impl FnMut(&mut [f32])) -> f32 {
        let mut audio: Vec<f32> = (0..48_000)
            .map(|n| (std::f64::consts::TAU * hz * f64::from(n) / rate).sin() as f32)
            .collect();
        filter(&mut audio);
        let tail = &audio[24_000..];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32 * 2.0).sqrt()
    }

    #[test]
    fn preemphasis_is_flat_at_its_reference_and_undoes_deemphasis() {
        let rate = 48_000.0;
        let mut lift = Preemphasis::new(rate, 750.0, 1_000.0);
        let at_reference = tone_gain(rate, 1_000.0, |audio| lift.process(audio));
        assert!((at_reference - 1.0).abs() < 0.01, "{at_reference}");
        for hz in [300.0, 1_000.0, 3_000.0] {
            let mut lift = Preemphasis::new(rate, 750.0, 1_000.0);
            let mut fall = Deemphasis::new(rate, 750.0);
            let fall_gain = deemphasis_gain(rate, 750.0, 1_000.0);
            let paired = tone_gain(rate, hz, |audio| {
                lift.process(audio);
                fall.process(audio);
            }) / fall_gain;
            assert!((paired - 1.0).abs() < 0.02, "{hz} Hz: {paired}");
        }
    }

    #[test]
    fn a_non_finite_sample_is_read_as_silence() {
        let mut lift = Preemphasis::new(48_000.0, 750.0, 1_000.0);
        let mut audio = [f32::NAN, 0.5, f32::INFINITY, 0.0];
        lift.process(&mut audio);
        assert!(audio.iter().all(|s| s.is_finite()));
    }
    use crate::testutil::{real_tone, rms_r};

    fn deemphasis_gain(rate: f64, tau_us: f32, freq_hz: f64) -> f32 {
        let mut filter = Deemphasis::new(rate, tau_us);
        let n = (rate * 0.2) as usize;
        let mut x = real_tone(freq_hz / rate, n);
        filter.process(&mut x);
        rms_r(&x[n / 2..]) / FRAC_1_SQRT_2
    }

    #[test]
    fn deemphasis_minus_3_db_point_matches_tau() {
        let (rate, tau_us) = (48_000.0, 50.0f32);
        let expected = 1.0 / (std::f64::consts::TAU * 50e-6);
        let (mut lo, mut hi) = (1_000.0f64, 10_000.0f64);
        for _ in 0..25 {
            let mid = f64::midpoint(lo, hi);
            if deemphasis_gain(rate, tau_us, mid) > FRAC_1_SQRT_2 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let found = f64::midpoint(lo, hi);
        let rel_err = (found - expected).abs() / expected;
        assert!(
            rel_err < 0.05,
            "-3 dB at {found} Hz, expected {expected} Hz"
        );
    }

    #[test]
    fn deemphasis_recovers_after_non_finite_sample() {
        let mut filter = Deemphasis::new(48_000.0, 50.0);
        let mut poisoned = real_tone(1_000.0 / 48_000.0, 480);
        poisoned[100] = f32::NAN;
        filter.process(&mut poisoned);

        let mut tone = real_tone(1_000.0 / 48_000.0, 9_600);
        filter.process(&mut tone);
        assert!(tone.iter().all(|v| v.is_finite()), "state still poisoned");
        let gain = rms_r(&tone[4_800..]) / FRAC_1_SQRT_2;
        assert!((0.9..1.0).contains(&gain), "post-recovery gain {gain}");
    }
}
