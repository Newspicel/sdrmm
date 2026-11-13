use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, OnceLock,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, SystemTime},
};

use num_complex::Complex;
use sdrmm_recorder::{SigmfError, SigmfWriter, meta_path};
use sdrmm_wire::PositionFix;

use crate::{EngineError, runtime::DSP_BLOCK};

const REC_QUEUE_SECONDS: f64 = 0.5;
const REC_QUEUE_MIN_BLOCKS: usize = 64;
const REC_QUEUE_MAX_BLOCKS: usize = 1 << 11;

pub(crate) fn queue_depth(sample_rate: f64) -> usize {
    ((sample_rate * REC_QUEUE_SECONDS / DSP_BLOCK as f64) as usize)
        .clamp(REC_QUEUE_MIN_BLOCKS, REC_QUEUE_MAX_BLOCKS)
}

#[derive(Clone, Debug)]
pub struct FinalizedRecording {
    pub stem: PathBuf,
    pub stream: u32,
    pub started_at: String,
    pub samples: u64,
    pub bytes: u64,
    pub overruns: u64,
    pub error: Option<String>,
}

#[derive(Debug, Default)]
pub(crate) struct RecordingShared {
    samples: AtomicU64,
    bytes: AtomicU64,
    error: OnceLock<String>,
    publication_failed: AtomicBool,
    runs: AtomicU32,
}

#[derive(Clone, Debug)]
pub(crate) struct RecordingPosition {
    tx: mpsc::SyncSender<RecMessage>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PositionUpdateError {
    Full,
    Disconnected,
}

impl RecordingPosition {
    pub(crate) fn update(&self, fix: Option<PositionFix>) -> Result<(), PositionUpdateError> {
        self.tx
            .try_send(RecMessage::Position(fix.map(Box::new)))
            .map_err(|error| match error {
                mpsc::TrySendError::Full(_) => PositionUpdateError::Full,
                mpsc::TrySendError::Disconnected(_) => PositionUpdateError::Disconnected,
            })
    }

    /// A fresh tap into the same file for a restarted runtime. Its blocks carry a new run, so the
    /// writer opens a capture at the seam instead of joining two streams silently.
    pub(crate) fn resume(&self, shared: &Arc<RecordingShared>) -> RecorderTap {
        RecorderTap {
            tx: self.tx.clone(),
            shared: shared.clone(),
            run: shared.runs.fetch_add(1, Ordering::Relaxed) + 1,
        }
    }
}

impl RecordingShared {
    pub(crate) fn samples(&self) -> u64 {
        self.samples.load(Ordering::Relaxed)
    }

    pub(crate) fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    pub(crate) fn error(&self) -> Option<String> {
        self.error.get().cloned().or_else(|| {
            self.publication_failed
                .load(Ordering::Relaxed)
                .then(|| "recording publication queue overflow or worker stopped".to_owned())
        })
    }

    pub(crate) fn fail(&self, message: String) {
        let _ = self.error.set(message);
    }
}

#[derive(Debug)]
pub(crate) struct RecBlock {
    start_sample: u64,
    center_hz: f64,
    run: u32,
    captured: Option<SystemTime>,
    samples: Arc<[Complex<f32>]>,
}

#[derive(Debug)]
pub(crate) enum RecMessage {
    Block(RecBlock),
    Position(Option<Box<PositionFix>>),
}

#[derive(Clone)]
pub(crate) struct RecorderTap {
    tx: mpsc::SyncSender<RecMessage>,
    shared: Arc<RecordingShared>,
    run: u32,
}

impl RecorderTap {
    pub(crate) fn feeds(&self, shared: &Arc<RecordingShared>) -> bool {
        Arc::ptr_eq(&self.shared, shared)
    }

    pub(crate) fn healthy(&self) -> bool {
        self.shared.error.get().is_none() && !self.shared.publication_failed.load(Ordering::Relaxed)
    }

    pub(crate) fn publication_failed(&self) {
        self.shared
            .publication_failed
            .store(true, Ordering::Relaxed);
    }

    #[must_use]
    pub(crate) fn push(&self, slice: &[Complex<f32>], start_sample: u64, center_hz: f64) -> bool {
        self.enqueue(slice, start_sample, center_hz, None, false)
    }

    #[must_use]
    pub(crate) fn push_waiting(
        &self,
        slice: &[Complex<f32>],
        start_sample: u64,
        center_hz: f64,
        captured: Option<SystemTime>,
    ) -> bool {
        self.enqueue(slice, start_sample, center_hz, captured, true)
    }

    fn enqueue(
        &self,
        slice: &[Complex<f32>],
        start_sample: u64,
        center_hz: f64,
        captured: Option<SystemTime>,
        wait: bool,
    ) -> bool {
        if slice.is_empty() {
            return true;
        }
        let block = RecBlock {
            start_sample,
            center_hz,
            run: self.run,
            captured,
            samples: Arc::from(slice),
        };
        let message = RecMessage::Block(block);
        let result = if wait {
            self.tx
                .send(message)
                .map_err(|error| mpsc::TrySendError::Disconnected(error.0))
        } else {
            self.tx.try_send(message)
        };
        match result {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(_)) => {
                self.shared.fail("recording queue overflow".to_string());
                false
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.shared.fail("recording writer stopped".to_string());
                false
            }
        }
    }
}

pub(crate) fn create_tap(
    sample_rate: f64,
) -> (
    RecorderTap,
    RecordingPosition,
    mpsc::Receiver<RecMessage>,
    Arc<RecordingShared>,
) {
    let (tx, rx) = mpsc::sync_channel(queue_depth(sample_rate));
    let shared = Arc::new(RecordingShared::default());
    let position = RecordingPosition { tx: tx.clone() };
    (
        RecorderTap {
            tx,
            shared: shared.clone(),
            run: 0,
        },
        position,
        rx,
        shared,
    )
}

pub(crate) fn spawn_writer(
    writer: SigmfWriter,
    messages: mpsc::Receiver<RecMessage>,
    shared: Arc<RecordingShared>,
) -> Result<JoinHandle<()>, EngineError> {
    std::thread::Builder::new()
        .name("sdrmm-rec".to_string())
        .spawn(move || write_loop(writer, &messages, &shared))
        .map_err(|e| EngineError::RecordingIo(format!("spawn recording writer thread: {e}")))
}

fn write_loop(
    mut writer: SigmfWriter,
    messages: &mpsc::Receiver<RecMessage>,
    shared: &RecordingShared,
) {
    let mut center = writer.meta().captures.last().and_then(|c| c.frequency);
    let rate = writer.meta().global.sample_rate.unwrap_or(0.0);
    let mut seen: Option<Seen> = None;
    while let Ok(message) = messages.recv() {
        let block = match message {
            RecMessage::Block(block) => block,
            RecMessage::Position(fix) => {
                writer.set_position(fix.as_deref());
                continue;
            }
        };
        if let Some(gap) = seen.and_then(|seen| seen.gap_before(&block, rate)) {
            tracing::debug!(missing = gap.missing, "recording spans a capture loss");
            writer.skip(gap.missing, &stamp(gap.at));
        }
        seen = Some(Seen::after(&block, rate));
        if center != Some(block.center_hz) {
            writer.add_capture(block.center_hz);
            center = Some(block.center_hz);
        }
        if let Err(e) = writer.write_block(&block.samples) {
            shared.fail(format!("recording write failed: {e}"));
            break;
        }
        shared
            .samples
            .store(writer.samples_written(), Ordering::Relaxed);
        shared
            .bytes
            .store(writer.bytes_written(), Ordering::Relaxed);
    }
    if let Err(e) = writer.finalize() {
        shared.fail(format!("recording finalize failed: {e}"));
    }
}

/// Where the stream stood after the last block the writer took.
#[derive(Clone, Copy)]
struct Seen {
    run: u32,
    next: u64,
    next_at: Option<SystemTime>,
}

struct Gap {
    missing: u64,
    at: SystemTime,
}

impl Seen {
    fn after(block: &RecBlock, rate: f64) -> Self {
        let len = block.samples.len() as u64;
        Self {
            run: block.run,
            next: block.start_sample + len,
            next_at: block.captured.and_then(|at| {
                at.checked_add(Duration::try_from_secs_f64(len as f64 / rate).ok()?)
            }),
        }
    }

    /// The samples lost before `block`. A restarted runtime counts from a new origin, so across
    /// runs the loss is estimated from the wall clock.
    fn gap_before(&self, block: &RecBlock, rate: f64) -> Option<Gap> {
        let at = block.captured.unwrap_or_else(SystemTime::now);
        if block.run == self.run {
            return (block.start_sample != self.next).then(|| Gap {
                missing: block.start_sample.saturating_sub(self.next),
                at,
            });
        }
        let missing = self
            .next_at
            .and_then(|expected| at.duration_since(expected).ok())
            .map_or(0, |lost| (lost.as_secs_f64() * rate).round() as u64);
        Some(Gap { missing, at })
    }
}

fn stamp(at: SystemTime) -> String {
    jiff::Timestamp::try_from(at)
        .unwrap_or_else(|_| jiff::Timestamp::now())
        .to_string()
}

pub(crate) fn create_writer(
    dir: &Path,
    prefix: &str,
    stream: u32,
    started_at: jiff::Timestamp,
    sample_rate: f64,
    center_hz: f64,
    hw: &str,
) -> Result<(SigmfWriter, String), EngineError> {
    let base = format!("{prefix}_{}", started_at.strftime("%Y%m%dT%H%M%SZ"));
    let mut name = base.clone();
    let mut n = 1u32;
    loop {
        let stem = dir.join(&name);
        if !meta_path(&stem).exists() {
            match SigmfWriter::create(&stem, sample_rate, center_hz, hw) {
                Ok(mut writer) => {
                    writer.set_rx_stream(stream);
                    return Ok((writer, name));
                }
                Err(SigmfError::StemTaken(_)) => {}
                Err(e) => {
                    return Err(EngineError::RecordingIo(format!(
                        "create {}: {e}",
                        stem.display()
                    )));
                }
            }
        }
        n += 1;
        name = format!("{base}-{n}");
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_recorder::{BYTES_PER_SAMPLE, SigmfReader, data_path};
    use tempfile::TempDir;

    use super::*;

    fn block(n: usize) -> Vec<Complex<f32>> {
        (0..n)
            .map(|i| Complex::new(i as f32, -(i as f32)))
            .collect()
    }

    #[test]
    fn writer_appends_capture_segments_and_finalizes_on_tap_drop() {
        let dir = TempDir::new().unwrap();
        let stem = dir.path().join("rec");
        let writer = SigmfWriter::create(&stem, 48_000.0, 100_000_000.0, "hw").unwrap();
        let (tap, position, messages, shared) = create_tap(48_000.0);
        let handle = spawn_writer(writer, messages, shared.clone()).unwrap();

        let samples = block(16);
        assert!(tap.push(&samples, 0, 100_000_000.0));
        assert!(tap.push(&samples, 16, 101_000_000.0));
        assert!(tap.push(&[], 32, 101_000_000.0), "empty slices are skipped");
        drop(tap);
        drop(position);
        handle.join().unwrap();

        assert_eq!(shared.samples(), 32);
        assert_eq!(shared.bytes(), 32 * sdrmm_recorder::BYTES_PER_SAMPLE);
        assert_eq!(shared.error(), None);
        let reader = SigmfReader::open(&stem).unwrap();
        assert_eq!(reader.total_samples(), 32);
        let captures = &reader.meta().captures;
        assert_eq!(captures.len(), 2);
        assert_eq!(captures[1].sample_start, 16);
        assert_eq!(captures[1].frequency, Some(101_000_000.0));
    }

    #[test]
    fn a_capture_loss_opens_a_segment_at_its_global_index() {
        let dir = TempDir::new().unwrap();
        let stem = dir.path().join("lossy");
        let writer = SigmfWriter::create(&stem, 48_000.0, 100_000_000.0, "hw").unwrap();
        let (tap, position, messages, shared) = create_tap(48_000.0);
        let handle = spawn_writer(writer, messages, shared.clone()).unwrap();
        let resumed = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        assert!(tap.push(&block(16), 0, 100_000_000.0));
        assert!(tap.push_waiting(&block(16), 40, 100_000_000.0, Some(resumed)));
        assert!(tap.push(&block(16), 56, 100_000_000.0));
        drop(tap);
        drop(position);
        handle.join().unwrap();

        let reader = SigmfReader::open(&stem).unwrap();
        assert_eq!(reader.total_samples(), 48);
        let captures = &reader.meta().captures;
        assert_eq!(captures.len(), 2, "{captures:?}");
        assert_eq!(captures[1].sample_start, 16);
        assert_eq!(captures[1].global_index, Some(40));
        assert_eq!(captures[1].frequency, Some(100_000_000.0));
        assert_eq!(
            captures[1].datetime.as_deref(),
            Some(stamp(resumed).as_str())
        );
    }

    #[test]
    fn a_resumed_tap_opens_a_segment_and_estimates_the_loss_from_the_clock() {
        let dir = TempDir::new().unwrap();
        let stem = dir.path().join("resumed");
        let writer = SigmfWriter::create(&stem, 1_000.0, 100_000_000.0, "hw").unwrap();
        let (tap, position, messages, shared) = create_tap(1_000.0);
        let handle = spawn_writer(writer, messages, shared.clone()).unwrap();
        let origin = SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000);

        assert!(tap.push_waiting(&block(100), 500, 100_000_000.0, Some(origin)));
        let restarted = position.resume(&shared);
        drop(tap);
        let later = origin + Duration::from_millis(600);
        assert!(restarted.push_waiting(&block(100), 100, 101_000_000.0, Some(later)));
        drop(restarted);
        drop(position);
        handle.join().unwrap();

        let reader = SigmfReader::open(&stem).unwrap();
        assert_eq!(reader.total_samples(), 200);
        let captures = &reader.meta().captures;
        assert_eq!(captures.len(), 2, "{captures:?}");
        assert_eq!(captures[1].sample_start, 100);
        assert_eq!(captures[1].global_index, Some(600));
        assert_eq!(captures[1].frequency, Some(101_000_000.0));
        assert_eq!(captures[1].datetime.as_deref(), Some(stamp(later).as_str()));
        assert_eq!(shared.error(), None);
    }

    #[test]
    fn writer_geotags_the_sample_where_a_live_fix_arrives() {
        let dir = TempDir::new().unwrap();
        let stem = dir.path().join("mobile");
        let writer = SigmfWriter::create(&stem, 48_000.0, 100_000_000.0, "hw").unwrap();
        let (tap, position, messages, shared) = create_tap(48_000.0);
        let handle = spawn_writer(writer, messages, shared).unwrap();
        position
            .update(Some(PositionFix {
                latitude: 52.52,
                longitude: 13.405,
                altitude_m: Some(40.0),
                accuracy_m: Some(3.0),
                speed_mps: None,
                track_deg: None,
                time: "2026-08-14T12:00:00Z".to_owned(),
                attitude: sdrmm_wire::Attitude::default(),
            }))
            .unwrap();
        assert!(tap.push(&block(16), 0, 100_000_000.0));
        drop(tap);
        drop(position);
        handle.join().unwrap();

        let reader = SigmfReader::open(&stem).unwrap();
        let capture = &reader.meta().captures[0];
        assert_eq!(
            capture.geolocation.as_ref().unwrap().coordinates,
            vec![13.405, 52.52, 40.0]
        );
    }

    #[test]
    fn queued_iq_before_a_fix_is_not_backdated() {
        let dir = TempDir::new().unwrap();
        let stem = dir.path().join("ordered-position");
        let writer = SigmfWriter::create(&stem, 48_000.0, 100_000_000.0, "hw").unwrap();
        let (tap, position, messages, shared) = create_tap(48_000.0);
        let handle = spawn_writer(writer, messages, shared).unwrap();
        assert!(tap.push(&block(16), 0, 100_000_000.0));
        position
            .update(Some(PositionFix {
                latitude: 52.52,
                longitude: 13.405,
                altitude_m: None,
                accuracy_m: None,
                speed_mps: None,
                track_deg: None,
                time: "2026-08-14T12:00:01Z".to_owned(),
                attitude: sdrmm_wire::Attitude::default(),
            }))
            .unwrap();
        assert!(tap.push(&block(16), 16, 100_000_000.0));
        drop(tap);
        drop(position);
        handle.join().unwrap();

        let reader = SigmfReader::open(&stem).unwrap();
        assert_eq!(reader.meta().captures.len(), 2);
        assert_eq!(reader.meta().captures[0].geolocation, None);
        assert_eq!(reader.meta().captures[1].sample_start, 16);
        assert_eq!(
            reader.meta().captures[1]
                .geolocation
                .as_ref()
                .unwrap()
                .coordinates,
            vec![13.405, 52.52]
        );
    }

    #[test]
    fn full_queue_surfaces_overflow_instead_of_dropping() {
        let (tap, position, _messages, shared) = create_tap(48_000.0);
        let samples = block(4);
        for i in 0..REC_QUEUE_MIN_BLOCKS as u64 {
            assert!(tap.push(&samples, i * 4, 1_000_000.0));
        }
        assert_eq!(position.update(None), Err(PositionUpdateError::Full));
        assert!(!tap.push(&samples, REC_QUEUE_MIN_BLOCKS as u64 * 4, 1_000_000.0));
        assert!(shared.error().unwrap().contains("overflow"));
    }

    #[test]
    fn a_stalled_writer_absorbs_half_a_second_of_iq_before_it_overflows() {
        let rate = 2_400_000.0;
        let (tap, position, _messages, shared) = create_tap(rate);
        drop(position);
        let samples = block(DSP_BLOCK);
        let mut absorbed = 0u64;
        while tap.push(&samples, absorbed, 1_000_000.0) {
            absorbed += DSP_BLOCK as u64;
        }
        assert_eq!(absorbed, (queue_depth(rate) * DSP_BLOCK) as u64);
        assert!(
            absorbed as f64 / rate > 0.49,
            "a writer that never drains took only {} s of IQ",
            absorbed as f64 / rate
        );
        assert!(shared.error().unwrap().contains("overflow"));
    }

    #[test]
    fn every_rate_queues_more_slack_than_the_capture_ring_holds() {
        for rate in [48_000.0, 250_000.0, 2_400_000.0, 10_000_000.0, 20_000_000.0] {
            let seconds = (queue_depth(rate) * DSP_BLOCK) as f64 / rate;
            assert!(
                seconds >= crate::runtime::capture::RING_SECONDS,
                "{rate} S/s left only {seconds} s of queue"
            );
        }
        assert_eq!(
            queue_depth(2_400_000.0),
            (2_400_000.0 * REC_QUEUE_SECONDS / DSP_BLOCK as f64) as usize
        );
    }

    #[test]
    fn a_rate_that_is_not_a_number_still_sizes_a_queue() {
        assert_eq!(queue_depth(48_000.0), REC_QUEUE_MIN_BLOCKS);
        assert_eq!(queue_depth(0.0), REC_QUEUE_MIN_BLOCKS);
        assert_eq!(queue_depth(f64::NAN), REC_QUEUE_MIN_BLOCKS);
        assert_eq!(queue_depth(-1.0), REC_QUEUE_MIN_BLOCKS);
        assert_eq!(queue_depth(f64::INFINITY), REC_QUEUE_MAX_BLOCKS);
    }

    #[test]
    fn the_deepest_queue_stays_inside_its_memory_budget() {
        let bytes = REC_QUEUE_MAX_BLOCKS * DSP_BLOCK * BYTES_PER_SAMPLE as usize;
        assert!(bytes <= 32 << 20, "{bytes} bytes of queued IQ");
    }

    #[test]
    fn dead_writer_surfaces_instead_of_dropping() {
        let (tap, position, messages, shared) = create_tap(48_000.0);
        drop(position);
        drop(messages);
        assert!(!tap.push(&block(4), 0, 1_000_000.0));
        assert_eq!(shared.error().as_deref(), Some("recording writer stopped"));
    }

    #[test]
    fn create_writer_advances_the_suffix_on_claimed_stems() {
        let dir = TempDir::new().unwrap();
        let ts = jiff::Timestamp::UNIX_EPOCH;
        let (first, name) =
            create_writer(dir.path(), "rec_3", 0, ts, 48_000.0, 1_000_000.0, "hw").unwrap();
        assert_eq!(name, "rec_3_19700101T000000Z");
        let base_stem = first.stem().to_path_buf();

        let (second, name) =
            create_writer(dir.path(), "rec_3", 0, ts, 48_000.0, 1_000_000.0, "hw").unwrap();
        assert_eq!(name, "rec_3_19700101T000000Z-2");

        first.finalize().unwrap();
        drop(second);
        let (_third, name) =
            create_writer(dir.path(), "rec_3", 0, ts, 48_000.0, 1_000_000.0, "hw").unwrap();
        assert_eq!(name, "rec_3_19700101T000000Z-3");

        std::fs::remove_file(data_path(&base_stem)).unwrap();
        let (_fourth, name) =
            create_writer(dir.path(), "rec_3", 0, ts, 48_000.0, 1_000_000.0, "hw").unwrap();
        assert_eq!(name, "rec_3_19700101T000000Z-4");
    }
}
