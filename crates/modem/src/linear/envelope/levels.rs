use sdrmm_dsp::one_pole_coeff;

use super::EnvelopeTiming;

/// Fits the envelope's pedestal and gain to the table's amplitude spread, seeded from a block
/// mean and then tracked with a one-pole average.
#[derive(Clone, Debug)]
pub(super) struct LevelTracker {
    mean: f32,
    mean_square: f32,
    alpha: f32,
    warmup: usize,
    seen: usize,
    seed_sum: f32,
    seed_square: f32,
    table_mean: f32,
    table_sd: f32,
}

impl LevelTracker {
    pub(super) fn new(timing: EnvelopeTiming, table_mean: f32, table_sd: f32) -> Self {
        Self {
            mean: 0.0,
            mean_square: 0.0,
            alpha: one_pole_coeff(1.0, timing.mean_symbols),
            warmup: timing.warmup_symbols.max(2),
            seen: 0,
            seed_sum: 0.0,
            seed_square: 0.0,
            table_mean,
            table_sd,
        }
    }

    pub(super) fn push(&mut self, envelope: f32) -> Option<f32> {
        if !envelope.is_finite() {
            return None;
        }
        if self.seen < self.warmup {
            self.seed_sum += envelope;
            self.seed_square += envelope * envelope;
            self.seen += 1;
            if self.seen == self.warmup {
                let n = self.warmup as f32;
                self.mean = self.seed_sum / n;
                self.mean_square = self.seed_square / n;
            }
            return None;
        }
        self.mean += self.alpha * (envelope - self.mean);
        self.mean_square += self.alpha * (envelope * envelope - self.mean_square);
        let gain = self.gain();
        if gain <= f32::MIN_POSITIVE {
            return Some(self.table_mean);
        }
        let pedestal = self.mean - gain * self.table_mean;
        Some((envelope - pedestal) / gain)
    }

    pub(super) fn snr(&self) -> f32 {
        let pedestal = self.mean - self.gain() * self.table_mean;
        self.mean / pedestal.max(f32::MIN_POSITIVE)
    }

    pub(super) fn reset(&mut self) {
        self.mean = 0.0;
        self.mean_square = 0.0;
        self.seen = 0;
        self.seed_sum = 0.0;
        self.seed_square = 0.0;
    }

    fn gain(&self) -> f32 {
        let variance = (self.mean_square - self.mean * self.mean).max(0.0);
        variance.sqrt() / self.table_sd
    }
}
