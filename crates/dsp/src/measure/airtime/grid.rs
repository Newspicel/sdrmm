use std::ops::Range;

use crate::math::fastmath::fast_db_to_power;

/// The width of every bin an airtime survey measures in.
pub const BIN_HZ: f64 = 312_500.0;

/// What a survey leaves out at either edge of its span, where the anti-alias filter rolls off.
pub const EDGE_HZ: f64 = 1_500_000.0;

/// How far a quiet floor may rise before it counts as raised.
pub const FLOOR_RISE_DB: f32 = 0.5;

/// Frames of one bin width in `ms`.
#[must_use]
pub const fn frames_in(ms: u32) -> u64 {
    ms as u64 * BIN_HZ as u64 / 1_000
}

/// Frames a survey waits for its floors before reporting nothing heard: an eighth of a report.
#[must_use]
pub const fn warmup_frames(report_ms: u32) -> u64 {
    frames_in(report_ms) / 8
}

#[must_use]
pub fn fft_len(sample_rate_hz: f64) -> usize {
    (sample_rate_hz / BIN_HZ).round() as usize
}

#[must_use]
pub fn frame_s() -> f64 {
    1.0 / BIN_HZ
}

#[must_use]
pub fn bin_offset_hz(bin: f64, len: usize) -> f64 {
    (bin - (len / 2) as f64) * BIN_HZ
}

#[must_use]
pub fn bins_between(low_hz: f64, high_hz: f64, len: usize) -> Range<usize> {
    let centre = (len / 2) as f64;
    let first = (low_hz / BIN_HZ + centre).ceil().max(0.0) as usize;
    let last = (high_hz / BIN_HZ + centre).floor().min(len as f64 - 1.0) as usize;
    first..(last + 1).max(first)
}

#[must_use]
pub fn usable_half_hz(sample_rate_hz: f64) -> f64 {
    sample_rate_hz / 2.0 - EDGE_HZ
}

/// The offsets a survey at `sample_rate_hz` measures between.
#[must_use]
pub fn occupied_band(sample_rate_hz: f64) -> (f64, f64) {
    let half = usable_half_hz(sample_rate_hz);
    (-half, half)
}

/// The power a bin must pass to count as busy, `margin_db` over its floor; none until the floor
/// settles.
#[must_use]
pub fn threshold(floor_db: Option<f32>, margin_db: f32) -> f32 {
    floor_db.map_or(f32::INFINITY, |floor| fast_db_to_power(floor + margin_db))
}

/// A frame the front end silenced.
#[must_use]
pub fn gated(power: &[f32]) -> bool {
    power.iter().all(|&bin| bin <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_keep_a_312_5_khz_bin() {
        assert_eq!(fft_len(20e6), 64);
        assert_eq!(fft_len(80e6), 256);
        assert_eq!(frames_in(1_000), 312_500);
        assert_eq!(bins_between(-1e6, 1e6, 64), 29..36);
        assert_eq!(bin_offset_hz(32.0, 64), 0.0);
        assert_eq!(bins_between(-20e6, 20e6, 64), 0..64);
    }
}
