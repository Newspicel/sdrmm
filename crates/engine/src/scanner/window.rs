use std::sync::Mutex;

use super::USABLE_SPAN_FRACTION;

/// What a stepping scan needs to know about the radio it moves.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ScanLane {
    pub(crate) rate: f64,
    /// The part of each window clear of the anti-alias filter's roll-off.
    pub(crate) clean_hz: f64,
    /// Samples a retune can leave in the radio's buffers, taken at the old tuning.
    pub(crate) in_flight: u64,
}

impl ScanLane {
    pub(crate) fn new(rate: f64, clean_fraction: Option<f64>, in_flight: u64) -> Self {
        let fraction = clean_fraction
            .filter(|f| f.is_finite() && *f > 0.0 && *f <= 1.0)
            .unwrap_or(USABLE_SPAN_FRACTION);
        Self {
            rate,
            clean_hz: rate * fraction,
            in_flight,
        }
    }
}

/// The index of the last target a window opening at `targets[from]` holds whole inside
/// `clean_hz`. Opening each window at the first target not yet read tiles a sorted plan into
/// the fewest windows.
pub(super) fn tile_end(targets: &[f64], from: usize, clean_hz: f64, bw_hz: f64) -> usize {
    let Some(&first) = targets.get(from) else {
        return from;
    };
    let reach = first + (clean_hz - bw_hz).max(0.0);
    let past = from + targets[from..].partition_point(|&hz| hz <= reach);
    past.saturating_sub(1).max(from)
}

/// The stretch of targets a stepping scan reads at once. Auto tuning frames it the way it
/// frames a decoder's band.
#[derive(Debug, Default)]
pub(crate) struct Window(Mutex<Option<(f64, f64)>>);

impl Window {
    pub(super) fn set(&self, low: f64, high: f64) {
        *self.lock() = Some((low, high));
    }

    pub(super) fn clear(&self) {
        *self.lock() = None;
    }

    pub(crate) fn edges(&self) -> Option<(f64, f64)> {
        *self.lock()
    }

    pub(super) fn holds(&self, hz: f64) -> bool {
        self.edges()
            .is_some_and(|(low, high)| (low..=high).contains(&hz))
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(f64, f64)>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raster(start: f64, step: f64, n: usize) -> Vec<f64> {
        (0..n).map(|i| start + step * i as f64).collect()
    }

    fn tiles(targets: &[f64], clean_hz: f64, bw_hz: f64) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        let mut from = 0;
        while from < targets.len() {
            let end = tile_end(targets, from, clean_hz, bw_hz);
            out.push((from, end));
            from = end + 1;
        }
        out
    }

    #[test]
    fn five_megahertz_of_airband_takes_three_rtl_windows() {
        let lane = ScanLane::new(2_400_000.0, None, 0);
        let airband = raster(118_000_000.0, 8_333.333, 601);
        let windows = tiles(&airband, lane.clean_hz, 8_333.333);
        assert_eq!(windows.len(), 3, "{windows:?}");
        for &(from, end) in &windows {
            assert!(airband[end] - airband[from] + 8_333.333 <= lane.clean_hz + 1e-6);
        }
    }

    #[test]
    fn every_target_lands_in_exactly_one_window() {
        let targets = [100e6, 100.01e6, 100.5e6, 103e6, 103.001e6, 110e6];
        let windows = tiles(&targets, 1e6, 12_500.0);
        assert_eq!(windows, vec![(0, 2), (3, 4), (5, 5)]);
    }

    #[test]
    fn a_band_wider_than_the_clean_span_still_gets_a_window_of_its_own() {
        assert_eq!(tile_end(&[100e6, 100.001e6], 0, 5_000.0, 10_000.0), 0);
        assert_eq!(tile_end(&[100e6], 3, 1e6, 10_000.0), 3);
    }

    #[test]
    fn a_radio_that_names_its_clean_span_is_taken_at_its_word() {
        assert_eq!(ScanLane::new(10e6, Some(0.9), 0).clean_hz, 9e6);
        assert_eq!(ScanLane::new(10e6, Some(f64::NAN), 0).clean_hz, 8e6);
        assert_eq!(ScanLane::new(10e6, Some(1.5), 0).clean_hz, 8e6);
    }

    #[test]
    fn a_window_holds_only_its_own_stretch() {
        let window = Window::default();
        assert!(!window.holds(100e6));
        window.set(100e6, 101e6);
        assert!(window.holds(100.5e6));
        assert!(!window.holds(101.5e6));
        window.clear();
        assert_eq!(window.edges(), None);
    }
}
