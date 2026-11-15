/// Keeps the normalised step finite when the reference taps are silent.
const POWER_FLOOR: f32 = 1e-6;

#[derive(Clone, Debug)]
pub struct AutoNotch {
    weights: Vec<f32>,
    history: Vec<f32>,
    head: usize,
    span: usize,
    mu: f32,
}

impl AutoNotch {
    const DELAY_S: f64 = 2e-3;
    const TAPS_S: f64 = 4e-3;
    const STEP_HZ: f64 = 24.0;

    #[must_use]
    pub fn new(rate: f64) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let taps = ((rate * Self::TAPS_S).round() as usize).max(1);
        let delay = ((rate * Self::DELAY_S).round() as usize).max(1);
        let span = taps + delay;
        Self {
            weights: vec![0.0; taps],
            history: vec![0.0; 2 * span],
            head: 0,
            span,
            mu: (Self::STEP_HZ / rate) as f32,
        }
    }

    pub fn reset(&mut self) {
        self.weights.fill(0.0);
        self.history.fill(0.0);
        self.head = 0;
    }

    pub fn process(&mut self, samples: &mut [f32]) {
        if !self.weights.iter().all(|w| w.is_finite()) {
            self.reset();
        }
        let taps = self.weights.len();
        for s in samples {
            let x = if s.is_finite() { *s } else { 0.0 };
            let reference = &self.history[self.head + 1..=self.head + taps];
            let (predicted, power) = reference
                .iter()
                .zip(&self.weights)
                .fold((0.0, 0.0), |(p, e), (&r, &w)| (p + w * r, e + r * r));
            let error = x - predicted;
            let step = self.mu * error / (power + POWER_FLOOR);
            for (w, &r) in self.weights.iter_mut().zip(reference) {
                *w += step * r;
            }
            self.history[self.head] = x;
            self.history[self.head + self.span] = x;
            self.head = (self.head + 1) % self.span;
            *s = error;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        audio::noise::testing::{RATE, tone_amplitude},
        testutil::{XorShift32, real_tone, rms_r},
    };

    fn tone_in_noise(tone_hz: f64, len: usize) -> Vec<f32> {
        let mut rng = XorShift32(0x1B3F_9C05);
        real_tone(tone_hz / RATE, len)
            .iter()
            .map(|t| t * 0.5 + rng.next_f32() * 0.2)
            .collect()
    }

    #[test]
    fn auto_notch_removes_an_unannounced_carrier() {
        let input = tone_in_noise(1_500.0, 96_000);
        let mut output = input.clone();
        AutoNotch::new(RATE).process(&mut output);

        let settled = 48_000;
        let before = tone_amplitude(&input[settled..], 1_500.0);
        let after = tone_amplitude(&output[settled..], 1_500.0);
        assert!(after < 0.2 * before, "tone {before} -> {after}");
    }

    #[test]
    fn auto_notch_keeps_the_noise_the_carrier_was_in() {
        let mut rng = XorShift32(0x77AA_0913);
        let input: Vec<f32> = (0..96_000).map(|_| rng.next_f32() * 0.2).collect();
        let mut output = input.clone();
        AutoNotch::new(RATE).process(&mut output);
        let settled = 48_000;
        let ratio = rms_r(&output[settled..]) / rms_r(&input[settled..]);
        assert!((0.8..1.2).contains(&ratio), "noise gain {ratio}");
    }

    fn gliding_voice(len: usize) -> Vec<f32> {
        let mut phase = 0.0f64;
        (0..len)
            .map(|n| {
                let t = n as f64 / RATE;
                let pitch = 130.0 + 30.0 * (std::f64::consts::TAU * 1.3 * t).sin();
                phase += std::f64::consts::TAU * pitch / RATE;
                let syllable = (std::f64::consts::PI * 4.0 * t).sin().abs();
                let voice: f64 = (1..=20)
                    .map(|k| {
                        let f = pitch * f64::from(k);
                        let formant = (-((f - 600.0) / 300.0).powi(2)).exp()
                            + 0.6 * (-((f - 1_400.0) / 400.0).powi(2)).exp();
                        formant * (phase * f64::from(k)).sin()
                    })
                    .sum();
                (0.2 * syllable * voice) as f32
            })
            .collect()
    }

    #[test]
    fn auto_notch_leaves_a_voice_alone() {
        let input = gliding_voice(144_000);
        let mut output = input.clone();
        AutoNotch::new(RATE).process(&mut output);
        let settled = 48_000;
        let ratio = rms_r(&output[settled..]) / rms_r(&input[settled..]);
        assert!(ratio > 0.8, "voice gain {ratio}");
    }

    #[test]
    fn auto_notch_recovers_after_a_non_finite_sample() {
        let mut notch = AutoNotch::new(RATE);
        let mut poisoned = vec![f32::NAN; 256];
        notch.process(&mut poisoned);
        assert!(poisoned.iter().all(|s| s.is_finite()));
        let mut clean = tone_in_noise(1_000.0, 48_000);
        notch.process(&mut clean);
        assert!(clean.iter().all(|s| s.is_finite()), "state still poisoned");
    }
}
