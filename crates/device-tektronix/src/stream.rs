use std::{
    cell::{Cell, RefCell},
    ops::Deref,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use sdrmm_device::{
    BlockGap, CaptureRadio, CaptureStream, DeviceError, Next, Sample, SampleConverter, StopHandle,
    StreamFailure, lock,
};

use crate::{api::Rsa, ffi, link::Link, model::Chunk};

/// The largest block the API hands out at its default block size.
pub const BLOCK_PAIRS: usize = 131_072;
const WAIT_SLICE: Duration = Duration::from_millis(20);
const BYTES_PER_SAMPLE: usize = 2 * size_of::<f32>();
const SPARE_FRAMES: usize = 4;

/// What the capture thread reads from the control side without a lock: the scale that puts the
/// reference level at full scale, and the rate the stream runs at.
#[derive(Debug, Default)]
pub struct Gauge {
    scale: AtomicU32,
    rate: AtomicU64,
}

impl Gauge {
    pub fn set(&self, scale: f32, rate: f64) {
        self.scale.store(scale.to_bits(), Ordering::Release);
        self.rate.store(rate.to_bits(), Ordering::Release);
    }

    #[must_use]
    pub fn scale(&self) -> f32 {
        f32::from_bits(self.scale.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Acquire))
    }
}

pub struct RxRadio {
    link: Arc<Link>,
    gauge: Arc<Gauge>,
}

impl RxRadio {
    #[must_use]
    pub fn new(link: Arc<Link>, gauge: Arc<Gauge>) -> Self {
        Self { link, gauge }
    }
}

impl CaptureRadio for RxRadio {
    type Stream = RxStream;

    fn arm(&self) -> Result<RxStream, DeviceError> {
        let api = &self.link.api;
        let ticks_per_second = api.timestamp_rate()? as f64;
        self.link.start()?;
        Ok(RxStream::new(
            api.clone(),
            self.gauge.clone(),
            ticks_per_second,
        ))
    }

    fn disarm(&self) {
        self.link.halt();
    }
}

#[derive(Clone, Debug, Default)]
pub struct RxStop(Arc<AtomicBool>);

impl StopHandle for RxStop {
    fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
}

type Spares = Arc<Mutex<Vec<Vec<f32>>>>;

pub struct RxStream {
    api: Arc<dyn Rsa>,
    gauge: Arc<Gauge>,
    ticks_per_second: f64,
    stop: RxStop,
    spares: Spares,
    last_end: Cell<Option<f64>>,
    overdriven: Cell<bool>,
    failure: RefCell<Option<StreamFailure>>,
}

impl RxStream {
    fn new(api: Arc<dyn Rsa>, gauge: Arc<Gauge>, ticks_per_second: f64) -> Self {
        Self {
            api,
            gauge,
            ticks_per_second,
            stop: RxStop::default(),
            spares: Spares::default(),
            last_end: Cell::new(None),
            overdriven: Cell::new(false),
            failure: RefCell::new(None),
        }
    }

    fn fail(&self, error: &DeviceError) -> Next<Frame> {
        *self.failure.borrow_mut() = Some(StreamFailure {
            reason: error.to_string(),
            gone: matches!(error, DeviceError::Disconnected(_)),
        });
        Next::Ended
    }

    fn spare(&self) -> Vec<f32> {
        lock(&self.spares)
            .pop()
            .unwrap_or_else(|| vec![0.0; BLOCK_PAIRS * 2])
    }

    fn note_overdrive(&self, status: u32) {
        let overdriven = status & ffi::STATUS_OVERRANGE != 0;
        if overdriven && !self.overdriven.get() {
            tracing::warn!("the analyser input is overdriven; lower the gain");
        }
        self.overdriven.set(overdriven);
    }

    fn ticks_per_sample(&self) -> f64 {
        self.ticks_per_second / self.gauge.rate()
    }
}

impl CaptureStream for RxStream {
    type Block = Frame;
    type Stop = RxStop;

    fn stop_handle(&self) -> RxStop {
        self.stop.clone()
    }

    fn next_block(&self, timeout: Duration) -> Next<Frame> {
        let deadline = Instant::now() + timeout;
        while !self.stop.0.load(Ordering::Acquire) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            match self.api.wait(left.min(WAIT_SLICE)) {
                Ok(true) => {}
                Ok(false) => continue,
                Err(error) => return self.fail(&error),
            }
            let mut samples = self.spare();
            match self.api.read(&mut samples) {
                Ok(Some(chunk)) => {
                    self.note_overdrive(chunk.status);
                    return Next::Block(Frame {
                        samples,
                        chunk,
                        spares: self.spares.clone(),
                    });
                }
                Ok(None) => lock(&self.spares).push(samples),
                Err(error) => return self.fail(&error),
            }
        }
        Next::Idle
    }

    fn dropped(&self) -> u64 {
        0
    }

    fn block_gap(&self, block: &Frame, _bytes_per_sample: u64) -> Option<BlockGap> {
        let ticks = self.ticks_per_sample();
        let chunk = block.chunk;
        let estimated = gap_samples(self.last_end.get(), chunk.timestamp, ticks);
        self.last_end
            .set(Some(chunk.timestamp as f64 + chunk.pairs as f64 * ticks));
        if estimated == 0 && chunk.status & ffi::STATUS_LOSS != 0 {
            tracing::warn!("the analyser flagged lost samples its timestamps do not show");
        }
        Some(BlockGap {
            exact: 0,
            estimated,
        })
    }

    fn failure(&self) -> StreamFailure {
        self.failure.borrow().clone().unwrap_or(StreamFailure {
            reason: "the analyser stream ended".to_string(),
            gone: false,
        })
    }
}

/// The samples missing between the end of the previous block and the start of this one, from
/// the timestamp the API puts on each block's first sample.
#[must_use]
pub fn gap_samples(previous_end: Option<f64>, timestamp: u64, ticks_per_sample: f64) -> u64 {
    let Some(previous_end) = previous_end else {
        return 0;
    };
    if !(ticks_per_sample.is_finite() && ticks_per_sample > 0.0) {
        return 0;
    }
    let gap = timestamp as f64 - previous_end;
    if gap <= ticks_per_sample / 2.0 {
        return 0;
    }
    (gap / ticks_per_sample).round() as u64
}

/// One block's IQ pairs in a buffer that goes back to its stream when the block is dropped.
pub struct Frame {
    samples: Vec<f32>,
    chunk: Chunk,
    spares: Spares,
}

impl Deref for Frame {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        let floats = &self.samples[..(self.chunk.pairs * 2).min(self.samples.len())];
        unsafe { std::slice::from_raw_parts(floats.as_ptr().cast::<u8>(), size_of_val(floats)) }
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        let mut spares = lock(&self.spares);
        if spares.len() < SPARE_FRAMES {
            spares.push(std::mem::take(&mut self.samples));
        }
    }
}

pub struct FloatConverter {
    gauge: Arc<Gauge>,
    out: Vec<Sample>,
}

impl FloatConverter {
    #[must_use]
    pub fn new(gauge: Arc<Gauge>) -> Self {
        Self {
            gauge,
            out: Vec::with_capacity(BLOCK_PAIRS),
        }
    }
}

impl SampleConverter for FloatConverter {
    fn convert(&mut self, bytes: &[u8]) -> &[Sample] {
        let scale = self.gauge.scale();
        self.out.clear();
        let (pairs, _) = bytes.as_chunks::<BYTES_PER_SAMPLE>();
        self.out.extend(pairs.iter().map(|pair| {
            let [i0, i1, i2, i3, q0, q1, q2, q3] = *pair;
            Sample::new(
                f32::from_ne_bytes([i0, i1, i2, i3]) * scale,
                f32::from_ne_bytes([q0, q1, q2, q3]) * scale,
            )
        }));
        &self.out
    }

    fn reset(&mut self) {}

    fn bytes_per_sample(&self) -> u64 {
        BYTES_PER_SAMPLE as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TICKS: f64 = 112e6 / 3.5e6;

    #[test]
    fn back_to_back_blocks_have_no_gap() {
        assert_eq!(gap_samples(None, 5_000, TICKS), 0);
        assert_eq!(gap_samples(Some(64_000.0), 64_000, TICKS), 0);
    }

    #[test]
    fn a_hole_in_the_timestamps_counts_the_samples_it_lost() {
        let end = 2_000.0 * TICKS;
        let resumed = (2_000.0 * TICKS + 13_213.0 * TICKS) as u64;
        assert_eq!(gap_samples(Some(end), resumed, TICKS), 13_213);
    }

    #[test]
    fn a_rate_that_is_not_known_yet_books_nothing() {
        assert_eq!(gap_samples(Some(0.0), 1_000_000, f64::INFINITY), 0);
    }

    #[test]
    fn floats_are_scaled_by_the_current_gauge() {
        let gauge = Arc::new(Gauge::default());
        gauge.set(2.0, 1e6);
        let mut converter = FloatConverter::new(gauge.clone());
        let bytes: Vec<u8> = [0.25f32, -0.5]
            .iter()
            .flat_map(|value| value.to_ne_bytes())
            .collect();
        assert_eq!(converter.convert(&bytes), &[Sample::new(0.5, -1.0)]);
        gauge.set(4.0, 1e6);
        assert_eq!(converter.convert(&bytes), &[Sample::new(1.0, -2.0)]);
    }

    #[test]
    fn a_frame_shows_only_the_pairs_its_block_filled() {
        let frame = Frame {
            samples: vec![1.0; 16],
            chunk: Chunk {
                pairs: 3,
                timestamp: 0,
                status: 0,
            },
            spares: Spares::default(),
        };
        assert_eq!(frame.len(), 3 * BYTES_PER_SAMPLE);
        let spares = frame.spares.clone();
        drop(frame);
        assert_eq!(lock(&spares).len(), 1);
    }
}
