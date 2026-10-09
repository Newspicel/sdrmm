use std::ops::Range;

use sdrmm_dsp::fastmath::fast_db_to_power;
use sdrmm_wire::{AIRTIME_REPORT_MS, AirtimeSpan};

use crate::{ChannelError, ChannelFilter};

pub(crate) const BIN_HZ: f64 = 312_500.0;
pub(crate) const EDGE_HZ: f64 = 1_500_000.0;
pub(crate) const FLOOR_RISE_DB: f32 = 0.5;
pub(crate) const REPORT_FRAMES: u64 = (AIRTIME_REPORT_MS as u64) * (BIN_HZ as u64) / 1_000;
pub(crate) const WARMUP_FRAMES: u64 = REPORT_FRAMES / 8;

#[must_use]
pub(crate) fn fft_len(span: AirtimeSpan) -> usize {
    (span.sample_rate_hz() / BIN_HZ).round() as usize
}

#[must_use]
pub(crate) fn frame_s() -> f64 {
    1.0 / BIN_HZ
}

#[must_use]
pub(crate) fn bin_offset_hz(bin: f64, len: usize) -> f64 {
    (bin - (len / 2) as f64) * BIN_HZ
}

#[must_use]
pub(crate) fn bins_between(low_hz: f64, high_hz: f64, len: usize) -> Range<usize> {
    let centre = (len / 2) as f64;
    let first = (low_hz / BIN_HZ + centre).ceil().max(0.0) as usize;
    let last = (high_hz / BIN_HZ + centre).floor().min(len as f64 - 1.0) as usize;
    first..(last + 1).max(first)
}

#[must_use]
pub(crate) fn usable_half_hz(span: AirtimeSpan) -> f64 {
    span.sample_rate_hz() / 2.0 - EDGE_HZ
}

#[must_use]
pub(crate) fn threshold(floor_db: Option<f32>, margin_db: f32) -> f32 {
    floor_db.map_or(f32::INFINITY, |floor| fast_db_to_power(floor + margin_db))
}

#[must_use]
pub(crate) fn gated(power: &[f32]) -> bool {
    power.iter().all(|&bin| bin <= 0.0)
}

#[must_use]
pub(crate) fn occupied_band(span: AirtimeSpan) -> (f64, f64) {
    let half = usable_half_hz(span);
    (-half, half)
}

#[must_use]
pub(crate) fn channel_filter() -> ChannelFilter {
    ChannelFilter::Passthrough
}

pub(crate) fn check_margin(margin_db: f32) -> Result<(), ChannelError> {
    if margin_db.is_finite()
        && (sdrmm_wire::MIN_AIRTIME_MARGIN_DB..=sdrmm_wire::MAX_AIRTIME_MARGIN_DB)
            .contains(&margin_db)
    {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "margin must be {}..={} dB, got {margin_db}",
            sdrmm_wire::MIN_AIRTIME_MARGIN_DB,
            sdrmm_wire::MAX_AIRTIME_MARGIN_DB
        )))
    }
}

#[cfg(test)]
pub(crate) mod scene;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_keep_a_312_5_khz_bin() {
        assert_eq!(fft_len(AirtimeSpan::Mhz20), 64);
        assert_eq!(fft_len(AirtimeSpan::Mhz80), 256);
        assert_eq!(REPORT_FRAMES, 312_500);
        assert_eq!(bins_between(-1e6, 1e6, 64), 29..36);
        assert_eq!(bin_offset_hz(32.0, 64), 0.0);
        assert_eq!(bins_between(-20e6, 20e6, 64), 0..64);
    }
}
