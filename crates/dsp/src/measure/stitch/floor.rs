use num_complex::Complex;

use super::{STITCH_FFT, sorted_window::SortedWindow};
use crate::math::fastmath::fast_power_db;

pub(super) const WARMUP_FRAMES: u64 = 16;
pub(super) const POWER_FLOOR_DB: f32 = -300.0;

const LEVEL_SMOOTHING: f32 = 0.125;
const FLOOR_STEP: f32 = 0.02;
const FLOOR_FALL_DB: f32 = 3.0;
const FLOOR_RISE_DB: f32 = 1.0;
const MEDIAN_BINS: usize = 33;
const POWER_FLOOR: f32 = 1e-30;

/// One lane's per-bin level and noise floor, the floor following slowly so a burst barely
/// lifts it, then median-smoothed across bins so a carrier does not stand in its own floor.
#[expect(
    clippy::struct_field_names,
    reason = "floor_db is the noise floor in dB"
)]
pub(super) struct Floor {
    level: Vec<f32>,
    level_db: Vec<f32>,
    floor_db: Vec<f32>,
    pub(super) smooth_db: Vec<f32>,
    pub(super) spur: Vec<bool>,
}

impl Floor {
    pub(super) fn new() -> Self {
        Self {
            level: vec![0.0; STITCH_FFT],
            level_db: vec![POWER_FLOOR_DB; STITCH_FFT],
            floor_db: vec![POWER_FLOOR_DB; STITCH_FFT],
            smooth_db: vec![POWER_FLOOR_DB; STITCH_FFT],
            spur: vec![false; STITCH_FFT],
        }
    }

    pub(super) fn track(&mut self, spectrum: &[Complex<f32>], frame: u64) {
        if frame < WARMUP_FRAMES {
            self.warm(spectrum, frame);
            return;
        }
        for (level, bin) in self.level.iter_mut().zip(spectrum) {
            *level += (bin.norm_sqr() - *level) * LEVEL_SMOOTHING;
        }
        for (level_db, level) in self.level_db.iter_mut().zip(&self.level) {
            *level_db = floored_power_db(*level);
        }
        for (floor_db, level_db) in self.floor_db.iter_mut().zip(&self.level_db) {
            *floor_db += FLOOR_STEP * (*level_db - *floor_db).clamp(-FLOOR_FALL_DB, FLOOR_RISE_DB);
        }
    }

    /// Fills `smooth_db` with the floor median-smoothed across bins; `ordered` is scratch.
    pub(super) fn smooth(&mut self, ordered: &mut [f32]) {
        let half = STITCH_FFT / 2;
        for (index, slot) in ordered.iter_mut().enumerate() {
            *slot = self.floor_db[(index + half) % STITCH_FFT];
        }
        let reach = MEDIAN_BINS / 2;
        let mut window = SortedWindow::<MEDIAN_BINS>::new();
        for value in ordered.iter().take(reach) {
            window.insert(*value);
        }
        for index in 0..ordered.len() {
            if let Some(outgoing) = index.checked_sub(reach + 1) {
                window.remove(ordered[outgoing]);
            }
            if let Some(incoming) = ordered.get(index + reach) {
                window.insert(*incoming);
            }
            self.smooth_db[(index + half) % STITCH_FFT] = window.median();
        }
    }

    pub(super) fn above_db(&self, bin: usize) -> f32 {
        self.level_db[bin] - self.smooth_db[bin]
    }

    fn warm(&mut self, spectrum: &[Complex<f32>], frame: u64) {
        let weight = 1.0 / (frame + 1) as f32;
        for (level, bin) in self.level.iter_mut().zip(spectrum) {
            *level += (bin.norm_sqr() - *level) * weight;
        }
        if frame + 1 < WARMUP_FRAMES {
            return;
        }
        let bins = self
            .level
            .iter()
            .zip(&mut self.level_db)
            .zip(&mut self.floor_db);
        for ((level, level_db), floor_db) in bins {
            *level_db = floored_power_db(*level);
            *floor_db = *level_db;
        }
    }
}

fn floored_power_db(power: f32) -> f32 {
    fast_power_db(power.max(POWER_FLOOR))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::db::Decibels;

    #[test]
    fn power_db_matches_log10() {
        for exponent in -29..30 {
            for step in 0..50 {
                let power = 10f32.powf(exponent as f32 + step as f32 / 50.0);
                let exact = power.power_to_db();
                assert!((floored_power_db(power) - exact).abs() < 1e-3, "{power}");
            }
        }
        assert!((floored_power_db(0.0) - POWER_FLOOR_DB).abs() < 1e-3);
        assert!((floored_power_db(f32::NAN) - POWER_FLOOR_DB).abs() < 1e-3);
    }
}
