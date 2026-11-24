use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};

use sdrmm_wire::SpectrumOverlap;
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
    Viewer(SpectrumOverlap),
    /// Viewers among them a scan stepping the radio, which wants frames as often as they come
    /// cheap so it can leave each window as soon as it has read it.
    Scan(SpectrumOverlap),
}

const OVERLAPS: u8 = SpectrumOverlap::ALL.len() as u8;

impl Watch {
    const fn code(self) -> u8 {
        match self {
            Self::Nobody => 0,
            Self::Background => 1,
            Self::Viewer(overlap) => 2 + overlap as u8,
            Self::Scan(overlap) => 2 + OVERLAPS + overlap as u8,
        }
    }

    fn from_code(code: u8) -> Self {
        let overlap = |at: u8| {
            SpectrumOverlap::ALL
                .get(usize::from(at))
                .copied()
                .unwrap_or_default()
        };
        match code {
            0 => Self::Nobody,
            1 => Self::Background,
            _ if code >= 2 + OVERLAPS => Self::Scan(overlap(code - 2 - OVERLAPS)),
            _ => Self::Viewer(overlap(code - 2)),
        }
    }
}

/// A lane's spectrum broadcast. Readers that only keep statistics subscribe as background, so
/// the display's overlapped transforms run only while someone looks.
#[derive(Clone, Debug)]
pub struct SpectrumTap {
    tx: broadcast::Sender<SpectrumSnapshot>,
    background: Arc<AtomicUsize>,
    claims: Arc<OverlapClaims>,
    scans: Arc<AtomicUsize>,
}

impl Default for SpectrumTap {
    fn default() -> Self {
        Self {
            tx: broadcast::channel(TAP_SLOTS).0,
            background: Arc::default(),
            claims: Arc::default(),
            scans: Arc::default(),
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

    /// Asks for `overlap` while the claim lives. The lane runs the most any claim asks.
    #[must_use]
    pub fn claim(&self, overlap: SpectrumOverlap) -> OverlapClaim {
        self.claims.count(overlap).fetch_add(1, Ordering::Relaxed);
        OverlapClaim {
            claims: self.claims.clone(),
            overlap,
        }
    }

    /// Asks for scan pace while the claim lives.
    #[must_use]
    pub fn claim_scan_pace(&self) -> ScanPaceClaim {
        self.scans.fetch_add(1, Ordering::Relaxed);
        ScanPaceClaim(self.scans.clone())
    }

    pub(crate) fn send(&self, snapshot: SpectrumSnapshot) {
        let _ = self.tx.send(snapshot);
    }

    /// Takes the broadcast's lock, so it belongs on the publishing thread, never the DSP one.
    pub(crate) fn watch(&self) -> Watch {
        let readers = self.tx.receiver_count();
        if readers > self.background.load(Ordering::Relaxed) {
            let overlap = self.claims.wanted();
            if self.scans.load(Ordering::Relaxed) > 0 {
                Watch::Scan(overlap)
            } else {
                Watch::Viewer(overlap)
            }
        } else if readers > 0 {
            Watch::Background
        } else {
            Watch::Nobody
        }
    }
}

#[derive(Debug, Default)]
struct OverlapClaims([AtomicUsize; SpectrumOverlap::ALL.len()]);

impl OverlapClaims {
    const fn count(&self, overlap: SpectrumOverlap) -> &AtomicUsize {
        &self.0[overlap as usize]
    }

    /// The most any claim asks, or [`SpectrumOverlap::Auto`] for viewers that claim nothing.
    fn wanted(&self) -> SpectrumOverlap {
        SpectrumOverlap::ALL
            .into_iter()
            .rev()
            .find(|overlap| self.count(*overlap).load(Ordering::Relaxed) > 0)
            .unwrap_or_default()
    }
}

/// A viewer's ask for how far its lane overlaps transforms, dropped when the viewer leaves.
#[derive(Debug)]
pub struct OverlapClaim {
    claims: Arc<OverlapClaims>,
    overlap: SpectrumOverlap,
}

impl Drop for OverlapClaim {
    fn drop(&mut self) {
        self.claims
            .count(self.overlap)
            .fetch_sub(1, Ordering::Relaxed);
    }
}

/// A scan's ask for frames at scan pace, dropped when the scan stops stepping.
#[derive(Debug)]
pub struct ScanPaceClaim(Arc<AtomicUsize>);

impl Drop for ScanPaceClaim {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
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
        Self(Arc::new(AtomicU8::new(
            Watch::Viewer(SpectrumOverlap::Auto).code(),
        )))
    }
}

impl Watching {
    pub(crate) fn get(&self) -> Watch {
        Watch::from_code(self.0.load(Ordering::Relaxed))
    }

    pub(crate) fn set(&self, watch: Watch) {
        self.0.store(watch.code(), Ordering::Relaxed);
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
        assert_eq!(tap.watch(), Watch::Viewer(SpectrumOverlap::Auto));
        drop(viewer);
        assert_eq!(tap.watch(), Watch::Background);
        drop(statistics);
        assert_eq!(tap.watch(), Watch::Nobody);
    }

    #[test]
    fn the_dsp_thread_reads_the_watch_it_was_given() {
        let watching = Watching::default();
        assert_eq!(watching.get(), Watch::Viewer(SpectrumOverlap::Auto));
        let viewers = SpectrumOverlap::ALL.map(Watch::Viewer);
        let scans = SpectrumOverlap::ALL.map(Watch::Scan);
        for watch in [Watch::Nobody, Watch::Background]
            .into_iter()
            .chain(viewers)
            .chain(scans)
        {
            watching.set(watch);
            assert_eq!(watching.clone().get(), watch);
        }
    }

    #[test]
    fn a_lane_overlaps_as_far_as_its_most_demanding_viewer() {
        let tap = SpectrumTap::default();
        let _viewer = tap.subscribe();
        let off = tap.claim(SpectrumOverlap::Off);
        assert_eq!(tap.watch(), Watch::Viewer(SpectrumOverlap::Off));
        let full = tap.claim(SpectrumOverlap::Full);
        let auto = tap.claim(SpectrumOverlap::Auto);
        assert_eq!(tap.watch(), Watch::Viewer(SpectrumOverlap::Full));
        drop(full);
        assert_eq!(tap.watch(), Watch::Viewer(SpectrumOverlap::Auto));
        drop(auto);
        assert_eq!(tap.watch(), Watch::Viewer(SpectrumOverlap::Off));
        drop(off);
        assert_eq!(tap.watch(), Watch::Viewer(SpectrumOverlap::Auto));
    }

    #[test]
    fn a_stepping_scan_paces_the_lane_only_while_it_claims() {
        let tap = SpectrumTap::default();
        let pace = tap.claim_scan_pace();
        assert_eq!(tap.watch(), Watch::Nobody, "a claim alone reads nothing");
        let _viewer = tap.subscribe();
        let _full = tap.claim(SpectrumOverlap::Full);
        assert_eq!(tap.watch(), Watch::Scan(SpectrumOverlap::Full));
        drop(pace);
        assert_eq!(tap.watch(), Watch::Viewer(SpectrumOverlap::Full));
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
