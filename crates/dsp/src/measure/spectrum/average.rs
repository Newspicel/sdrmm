use num_complex::Complex;

use super::POWER_EPSILON;
use crate::math::fastmath::{fast_db_to_power, fast_power_db};

pub struct PowerAverage {
    sum: Vec<f32>,
    count: u32,
}

impl PowerAverage {
    #[must_use]
    pub fn new(size: usize) -> Self {
        Self {
            sum: vec![0.0; size],
            count: 0,
        }
    }

    #[must_use]
    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn reset(&mut self) {
        self.sum.fill(0.0);
        self.count = 0;
    }

    pub fn add(&mut self, db: &[f32]) {
        assert_eq!(db.len(), self.sum.len(), "average length mismatch");
        for (slot, &level) in self.sum.iter_mut().zip(db) {
            *slot += fast_db_to_power(level);
        }
        self.count += 1;
    }

    pub fn take_db(&mut self, out: &mut [f32]) {
        assert_eq!(out.len(), self.sum.len(), "average length mismatch");
        let scale = 1.0 / self.count.max(1) as f32;
        for (slot, &power) in out.iter_mut().zip(&self.sum) {
            *slot = fast_power_db(power * scale + POWER_EPSILON);
        }
        self.reset();
    }

    pub(super) fn len(&self) -> usize {
        self.sum.len()
    }

    /// Adds one transform, its two halves laid out in display order.
    pub(super) fn add_power(
        &mut self,
        low_bins: &[Complex<f32>],
        high_bins: &[Complex<f32>],
        scale: f32,
    ) {
        let (low, high) = self.sum.split_at_mut(low_bins.len());
        for (slot, bin) in low
            .iter_mut()
            .chain(high)
            .zip(low_bins.iter().chain(high_bins))
        {
            *slot += bin.norm_sqr() * scale;
        }
        self.count += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::{super::floor::tests::exponential_power_db, *};

    fn spread(db: &[f32]) -> f32 {
        let mean = db.iter().sum::<f32>() / db.len() as f32;
        (db.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / db.len() as f32).sqrt()
    }

    #[test]
    fn averaging_power_calms_noise_and_keeps_its_mean() {
        let mut average = PowerAverage::new(4_096);
        for seed in 1..=16 {
            average.add(&exponential_power_db(4_096, -80.0, seed * 0x9E37));
        }
        assert_eq!(average.count(), 16);
        let mut out = vec![0.0; 4_096];
        average.take_db(&mut out);
        assert_eq!(average.count(), 0);
        let raw = spread(&exponential_power_db(4_096, -80.0, 7));
        let calm = spread(&out);
        assert!(calm < raw / 3.0, "spread {calm} dB, raw {raw} dB");
        let mean = out.iter().sum::<f32>() / out.len() as f32;
        assert!((mean - -80.0).abs() < 1.0, "mean read as {mean}");
    }

    #[test]
    fn averaging_a_steady_tone_leaves_it_in_place() {
        let mut average = PowerAverage::new(3);
        average.add(&[-10.0, -60.0, -90.0]);
        average.add(&[-10.0, -60.0, -90.0]);
        let mut out = [0.0; 3];
        average.take_db(&mut out);
        for (got, want) in out.iter().zip([-10.0, -60.0, -90.0]) {
            assert!((got - want).abs() < 1e-3, "{got} vs {want}");
        }
    }
}
