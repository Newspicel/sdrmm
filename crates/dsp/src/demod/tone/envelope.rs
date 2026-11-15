use crate::filter::iir::one_pole_coeff;

#[derive(Clone, Debug)]
pub struct Envelope {
    value: f32,
    attack: f32,
    release: f32,
}

impl Envelope {
    #[must_use]
    pub fn new(sample_rate: f64, attack_s: f64, release_s: f64) -> Self {
        assert!(sample_rate > 0.0, "sample rate must be positive");
        assert!(
            attack_s > 0.0 && release_s > 0.0,
            "time constants must be positive"
        );
        Self {
            value: 0.0,
            attack: one_pole_coeff(sample_rate, attack_s),
            release: one_pole_coeff(sample_rate, release_s),
        }
    }

    pub fn push(&mut self, magnitude: f32) -> f32 {
        if magnitude.is_finite() {
            let coeff = if magnitude > self.value {
                self.attack
            } else {
                self.release
            };
            self.value += coeff * (magnitude - self.value);
        }
        self.value
    }

    #[must_use]
    pub fn value(&self) -> f32 {
        self.value
    }

    pub fn reset(&mut self) {
        self.value = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 48_000.0;

    #[test]
    fn envelope_attack_and_release_hit_63_percent_at_their_time_constants() {
        let (attack, release) = (0.010, 0.100);
        let mut env = Envelope::new(RATE, attack, release);
        for _ in 0..(attack * RATE) as usize {
            env.push(1.0);
        }
        let risen = env.value();
        assert!((0.569..0.696).contains(&risen), "attack reached {risen}");

        let mut env = Envelope::new(RATE, attack, release);
        for _ in 0..(4.0 * attack * RATE) as usize {
            env.push(1.0);
        }
        let start = env.value();
        for _ in 0..(release * RATE) as usize {
            env.push(0.0);
        }
        let fallen = (start - env.value()) / start;
        assert!((0.569..0.696).contains(&fallen), "release fell {fallen}");
    }

    #[test]
    fn envelope_holds_through_a_non_finite_sample() {
        let mut env = Envelope::new(RATE, 0.001, 0.001);
        for _ in 0..480 {
            env.push(1.0);
        }
        let before = env.value();
        assert_eq!(env.push(f32::NAN), before);
        assert!(env.push(1.0).is_finite(), "state poisoned by NaN");
    }
}
