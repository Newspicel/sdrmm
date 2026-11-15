use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};

use tokio::sync::broadcast;

#[derive(Clone, Debug)]
pub struct SpectrumSnapshot {
    pub seq: u32,
    pub timestamp: u64,
    pub center_hz: f64,
    pub span_hz: f32,
    pub db: Arc<[f32]>,
}

const TAP_SLOTS: usize = 8;
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

/// Who reads a lane's spectrum, which sets how much of it the DSP thread works out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Watch {
    Nobody,
    /// Only readers keeping statistics, which need every sample once but no overlap.
    Background,
    Viewer,
}

/// A lane's spectrum broadcast. Readers that only keep statistics subscribe as background, so
/// the display's overlapped transforms run only while someone looks.
#[derive(Clone, Debug)]
pub struct SpectrumTap {
    tx: broadcast::Sender<SpectrumSnapshot>,
    background: Arc<AtomicUsize>,
}

impl Default for SpectrumTap {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(TAP_SLOTS).0,
            background: Arc::default(),
        }
    }
}

impl SpectrumTap {
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SpectrumSnapshot> {
        self.tx.subscribe()
    }

    #[must_use]
    pub fn subscribe_background(&self) -> BackgroundSpectrum {
        self.background.fetch_add(1, Ordering::Relaxed);
        BackgroundSpectrum {
            rx: self.tx.subscribe(),
            background: self.background.clone(),
        }
    }

    pub(crate) fn send(&self, snapshot: SpectrumSnapshot) {
        let _ = self.tx.send(snapshot);
    }

    /// Takes the broadcast's lock, so it belongs on the publishing thread, never the DSP one.
    pub(crate) fn watch(&self) -> Watch {
        let readers = self.tx.receiver_count();
        if readers > self.background.load(Ordering::Relaxed) {
            Watch::Viewer
        } else if readers > 0 {
            Watch::Background
        } else {
            Watch::Nobody
        }
    }
}

/// A statistics reader of a lane's spectrum. It does not keep the display's overlap running.
pub struct BackgroundSpectrum {
    rx: broadcast::Receiver<SpectrumSnapshot>,
    background: Arc<AtomicUsize>,
}

impl BackgroundSpectrum {
    pub fn blocking_recv(&mut self) -> Result<SpectrumSnapshot, broadcast::error::RecvError> {
        self.rx.blocking_recv()
    }
}

impl Drop for BackgroundSpectrum {
    fn drop(&mut self) {
        self.background.fetch_sub(1, Ordering::Relaxed);
    }
}

/// The last [`Watch`] the publishing thread saw, read lock-free by the DSP thread.
#[derive(Clone, Debug)]
pub(crate) struct Watching(Arc<AtomicU8>);

impl Default for Watching {
    fn default() -> Self {
        Self(Arc::new(AtomicU8::new(Watch::Viewer as u8)))
    }
}

impl Watching {
    pub(crate) fn get(&self) -> Watch {
        match self.0.load(Ordering::Relaxed) {
            value if value == Watch::Nobody as u8 => Watch::Nobody,
            value if value == Watch::Background as u8 => Watch::Background,
            _ => Watch::Viewer,
        }
    }

    pub(crate) fn set(&self, watch: Watch) {
        self.0.store(watch as u8, Ordering::Relaxed);
    }
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

    #[test]
    fn the_watch_follows_who_subscribes() {
        let tap = SpectrumTap::default();
        assert_eq!(tap.watch(), Watch::Nobody);
        let statistics = tap.subscribe_background();
        assert_eq!(tap.watch(), Watch::Background);
        let viewer = tap.subscribe();
        assert_eq!(tap.watch(), Watch::Viewer);
        drop(viewer);
        assert_eq!(tap.watch(), Watch::Background);
        drop(statistics);
        assert_eq!(tap.watch(), Watch::Nobody);
    }

    #[test]
    fn the_dsp_thread_reads_the_watch_it_was_given() {
        let watching = Watching::default();
        assert_eq!(watching.get(), Watch::Viewer);
        for watch in [Watch::Nobody, Watch::Background, Watch::Viewer] {
            watching.set(watch);
            assert_eq!(watching.clone().get(), watch);
        }
    }

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
