use std::sync::Arc;

pub(super) mod average;
pub(super) mod history;

#[derive(Clone, Debug)]
pub struct SpectrumSnapshot {
    pub seq: u32,
    pub timestamp: u64,
    pub center_hz: f64,
    pub span_hz: f32,
    pub db: Arc<[f32]>,
}

const LO_GUARD_BINS: usize = 4;
const MIN_SPECTRUM_SIZE: usize = 4_096;
const MAX_SPECTRUM_SIZE: usize = 262_144;
const TARGET_BIN_HZ: f64 = 1_000.0;

pub(crate) fn spectrum_size(sample_rate: f64) -> usize {
    let bins =
        (sample_rate / TARGET_BIN_HZ).clamp(MIN_SPECTRUM_SIZE as f64, MAX_SPECTRUM_SIZE as f64);
    (bins.ceil() as usize)
        .next_power_of_two()
        .clamp(MIN_SPECTRUM_SIZE, MAX_SPECTRUM_SIZE)
}

impl SpectrumSnapshot {
    #[must_use]
    pub fn lo_guard(&self) -> Option<std::ops::RangeInclusive<usize>> {
        let n = self.db.len();
        if n == 0 || !self.span_hz.is_finite() || self.span_hz <= 0.0 {
            return None;
        }
        let bin = n as f64 / 2.0;
        let lo = (bin - LO_GUARD_BINS as f64).ceil();
        let hi = (bin + LO_GUARD_BINS as f64).floor();
        if hi < 0.0 || lo > (n - 1) as f64 {
            return None;
        }
        Some((lo.max(0.0) as usize)..=(hi.max(0.0) as usize).min(n - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot_at(center_hz: f64, bins: usize) -> SpectrumSnapshot {
        SpectrumSnapshot {
            seq: 0,
            timestamp: 0,
            center_hz,
            span_hz: 1_024_000.0,
            db: Arc::from(vec![-100.0f32; bins].as_slice()),
        }
    }

    #[test]
    fn the_lo_guard_covers_the_centre_bins() {
        assert_eq!(snapshot_at(100e6, 1_024).lo_guard(), Some(508..=516));
    }

    #[test]
    fn the_spectrum_grows_with_the_rate_to_keep_bins_near_a_kilohertz() {
        for (rate, size) in [
            (48_000.0, 4_096),
            (2_400_000.0, 4_096),
            (10_000_000.0, 16_384),
            (20_000_000.0, 32_768),
            (61_440_000.0, 65_536),
            (245_760_000.0, 262_144),
            (1e12, 262_144),
        ] {
            assert_eq!(spectrum_size(rate), size, "{rate}");
        }
        for rate in [10_000_000.0, 20_000_000.0, 61_440_000.0, 245_760_000.0] {
            assert!(rate / spectrum_size(rate) as f64 <= TARGET_BIN_HZ, "{rate}");
        }
    }

    #[test]
    fn a_rate_that_is_not_a_rate_gets_the_smallest_spectrum() {
        for rate in [f64::NAN, -1.0, 0.0, f64::NEG_INFINITY] {
            assert_eq!(spectrum_size(rate), MIN_SPECTRUM_SIZE, "{rate}");
        }
    }

    #[test]
    fn an_empty_snapshot_guards_nothing() {
        assert_eq!(snapshot_at(100e6, 0).lo_guard(), None);
    }
}
