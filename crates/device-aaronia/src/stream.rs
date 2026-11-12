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

use crate::{
    api::{DevHandle, Rtsa},
    ffi,
    link::Link,
    model::{Mode, PacketView},
};

const PACKET_POLL: Duration = Duration::from_millis(2);
const RATE_GRACE: Duration = Duration::from_secs(1);
const RATE_TOLERANCE: f64 = 1e-3;
const BYTES_PER_SAMPLE: usize = 2 * size_of::<f32>();

/// What the capture thread reads from the control side without a lock: the scale that puts the
/// reference level at full scale, and the rate the packets should arrive at.
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
    armed: Mutex<Option<DevHandle>>,
}

impl RxRadio {
    #[must_use]
    pub fn new(link: Arc<Link>, gauge: Arc<Gauge>) -> Self {
        Self {
            link,
            gauge,
            armed: Mutex::new(None),
        }
    }
}

impl CaptureRadio for RxRadio {
    type Stream = RxStream;

    fn arm(&self) -> Result<RxStream, DeviceError> {
        let api = &self.link.api;
        let dev = self.link.handle_in(Mode::Receiver)?;
        api.connect(dev)?;
        if let Err(error) = api.start(dev) {
            if let Err(disconnect) = api.disconnect(dev) {
                tracing::warn!(%disconnect, "disconnecting a SPECTRAN that would not start");
            }
            return Err(error);
        }
        *lock(&self.armed) = Some(dev);
        Ok(RxStream::new(api.clone(), dev, self.gauge.clone()))
    }

    fn disarm(&self) {
        let Some(dev) = lock(&self.armed).take() else {
            return;
        };
        let api = &self.link.api;
        if let Err(error) = api.stop(dev) {
            tracing::warn!(%error, "stopping the SPECTRAN stream");
        }
        if let Err(error) = api.disconnect(dev) {
            tracing::warn!(%error, "disconnecting the SPECTRAN");
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RxStop(Arc<AtomicBool>);

impl StopHandle for RxStop {
    fn stop(&self) {
        self.0.store(true, Ordering::Release);
    }
}

/// Watches the rate the packets carry against the rate asked for. A mismatch that outlasts the
/// packets already in flight when the rate changed is a stream the engine would mistime.
#[derive(Clone, Copy, Debug, Default)]
pub struct RateWatch {
    since: Option<Instant>,
}

impl RateWatch {
    pub fn observe(&mut self, actual: f64, expected: f64, now: Instant) -> Result<(), String> {
        if expected <= 0.0 || ((actual - expected) / expected).abs() <= RATE_TOLERANCE {
            self.since = None;
            return Ok(());
        }
        let since = *self.since.get_or_insert(now);
        if now.duration_since(since) < RATE_GRACE {
            return Ok(());
        }
        Err(format!(
            "the SPECTRAN streams {actual} Hz, not the {expected} Hz asked for"
        ))
    }
}

pub struct RxStream {
    api: Arc<dyn Rtsa>,
    dev: DevHandle,
    gauge: Arc<Gauge>,
    stop: RxStop,
    last_end: Cell<Option<f64>>,
    watch: Cell<RateWatch>,
    clipping: Cell<bool>,
    failure: RefCell<Option<StreamFailure>>,
}

impl RxStream {
    fn new(api: Arc<dyn Rtsa>, dev: DevHandle, gauge: Arc<Gauge>) -> Self {
        Self {
            api,
            dev,
            gauge,
            stop: RxStop::default(),
            last_end: Cell::new(None),
            watch: Cell::new(RateWatch::default()),
            clipping: Cell::new(false),
            failure: RefCell::new(None),
        }
    }

    fn fail(&self, reason: String, gone: bool) -> Next<PacketBlock> {
        *self.failure.borrow_mut() = Some(StreamFailure { reason, gone });
        Next::Ended
    }

    fn release(&self) {
        if let Err(error) = self.api.consume(self.dev) {
            tracing::warn!(%error, "releasing a SPECTRAN packet");
        }
    }

    fn note_clipping(&self, flags: u64) {
        let clipping = flags & ffi::PACKET_WARN_OVERFLOW != 0;
        if clipping && !self.clipping.get() {
            tracing::warn!("the SPECTRAN input is overdriven; raise the reference level");
        }
        self.clipping.set(clipping);
    }

    fn accept(&self, view: PacketView) -> Option<Next<PacketBlock>> {
        if view.num <= 0 {
            self.release();
            return None;
        }
        if view.size != 2 || view.stride != 2 || view.data.is_null() {
            self.release();
            return Some(self.fail(
                format!(
                    "the SPECTRAN sent packets of {} values at a stride of {}, not IQ pairs",
                    view.size, view.stride
                ),
                false,
            ));
        }
        let mut watch = self.watch.get();
        let checked = watch.observe(view.step_hz, self.gauge.rate(), Instant::now());
        self.watch.set(watch);
        if let Err(reason) = checked {
            self.release();
            return Some(self.fail(reason, false));
        }
        self.note_clipping(view.flags);
        Some(Next::Block(PacketBlock {
            api: self.api.clone(),
            dev: self.dev,
            view,
        }))
    }
}

impl CaptureStream for RxStream {
    type Block = PacketBlock;
    type Stop = RxStop;

    fn stop_handle(&self) -> RxStop {
        self.stop.clone()
    }

    fn next_block(&self, timeout: Duration) -> Next<PacketBlock> {
        let deadline = Instant::now() + timeout;
        while !self.stop.0.load(Ordering::Acquire) {
            match self.api.packet(self.dev) {
                Err(error) => {
                    let gone = matches!(error, DeviceError::Disconnected(_));
                    return self.fail(error.to_string(), gone);
                }
                Ok(Some(view)) => {
                    if let Some(next) = self.accept(view) {
                        return next;
                    }
                }
                Ok(None) => {
                    if Instant::now() >= deadline {
                        return Next::Idle;
                    }
                    std::thread::sleep(PACKET_POLL);
                }
            }
        }
        Next::Idle
    }

    fn dropped(&self) -> u64 {
        0
    }

    fn block_gap(&self, block: &PacketBlock, _bytes_per_sample: u64) -> Option<BlockGap> {
        let estimated = gap_samples(self.last_end.get(), &block.view);
        self.last_end.set(Some(block.view.end_time));
        if estimated == 0 && block.view.flags & ffi::PACKET_WARN_DROPPED != 0 {
            tracing::warn!("the SPECTRAN flagged a dropped packet its timestamps do not show");
        }
        Some(BlockGap {
            exact: 0,
            estimated,
        })
    }

    fn failure(&self) -> StreamFailure {
        self.failure.borrow().clone().unwrap_or(StreamFailure {
            reason: "the SPECTRAN stream ended".to_string(),
            gone: false,
        })
    }
}

/// The samples missing between the end of the previous packet and the start of this one, from
/// the timestamps the API puts on both.
#[must_use]
pub fn gap_samples(previous_end: Option<f64>, view: &PacketView) -> u64 {
    let Some(previous_end) = previous_end else {
        return 0;
    };
    let mut per_sample = (view.end_time - view.start_time) / view.num.max(1) as f64;
    if !(per_sample.is_finite() && per_sample > 0.0) && view.step_hz > 0.0 {
        per_sample = 1.0 / view.step_hz;
    }
    if !(per_sample.is_finite() && per_sample > 0.0) {
        return 0;
    }
    let gap = view.start_time - previous_end;
    if gap <= per_sample / 2.0 {
        return 0;
    }
    (gap / per_sample).round() as u64
}

/// One packet's IQ pairs, lent by the API until this block is dropped and the packet consumed.
pub struct PacketBlock {
    api: Arc<dyn Rtsa>,
    dev: DevHandle,
    view: PacketView,
}

impl Deref for PacketBlock {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        let len = self.view.num as usize * BYTES_PER_SAMPLE;
        unsafe { std::slice::from_raw_parts(self.view.data.cast::<u8>(), len) }
    }
}

impl Drop for PacketBlock {
    fn drop(&mut self) {
        if let Err(error) = self.api.consume(self.dev) {
            tracing::warn!(%error, "releasing a SPECTRAN packet");
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
            out: Vec::new(),
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

    fn view(start: f64, end: f64, num: i64) -> PacketView {
        PacketView {
            flags: 0,
            start_time: start,
            end_time: end,
            step_hz: num as f64 / (end - start),
            num,
            size: 2,
            stride: 2,
            data: std::ptr::null(),
        }
    }

    #[test]
    fn back_to_back_packets_have_no_gap() {
        assert_eq!(gap_samples(Some(1.0), &view(1.0, 1.001, 1000)), 0);
        assert_eq!(gap_samples(None, &view(5.0, 5.001, 1000)), 0);
    }

    #[test]
    fn a_hole_in_the_timestamps_counts_the_samples_it_lost() {
        assert_eq!(gap_samples(Some(1.0), &view(1.002, 1.003, 1000)), 2000);
    }

    #[test]
    fn a_rate_that_settles_within_the_grace_is_accepted() {
        let mut watch = RateWatch::default();
        let start = Instant::now();
        assert!(watch.observe(1e6, 2e6, start).is_ok());
        assert!(watch.observe(1e6, 2e6, start + RATE_GRACE / 2).is_ok());
        assert!(watch.observe(2e6, 2e6, start + RATE_GRACE).is_ok());
        assert!(watch.observe(1e6, 2e6, start + RATE_GRACE * 2).is_ok());
    }

    #[test]
    fn a_rate_that_stays_wrong_fails_the_stream() {
        let mut watch = RateWatch::default();
        let start = Instant::now();
        assert!(watch.observe(1.5e6, 2e6, start).is_ok());
        let error = watch
            .observe(1.5e6, 2e6, start + RATE_GRACE)
            .expect_err("still wrong");
        assert!(error.contains("1500000"));
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
}
