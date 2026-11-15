const SPREAD_STEADY: f32 = 0.35;

const SPREAD_NOISE: f32 = 0.75;

#[derive(Clone, Debug)]
pub(super) struct Steadiness {
    alpha: f32,
    power: f32,
    power_square: f32,
}

impl Steadiness {
    pub(super) fn new(samples: f32) -> Self {
        Self {
            alpha: samples.max(1.0).recip(),
            power: 0.0,
            power_square: 0.0,
        }
    }

    pub(super) fn push(&mut self, power: f32) {
        self.power += self.alpha * (power - self.power);
        self.power_square += self.alpha * (power * power - self.power_square);
    }

    fn spread(&self) -> f32 {
        let mean = self.power * self.power;
        if mean <= f32::MIN_POSITIVE {
            return 1.0;
        }
        (self.power_square - mean).max(0.0) / mean
    }

    pub(super) fn power(&self) -> f32 {
        self.power
    }

    pub(super) fn weight(&self) -> f32 {
        ((SPREAD_NOISE - self.spread()) / (SPREAD_NOISE - SPREAD_STEADY)).clamp(0.0, 1.0)
    }

    pub(super) fn reset(&mut self) {
        self.power = 0.0;
        self.power_square = 0.0;
    }
}
