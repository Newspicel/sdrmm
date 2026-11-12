use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use sdrmm_device::{DeviceError, Direction, DuplexState, Sample, TxStream, lock};

use crate::{api::DevHandle, caps::tx_scale, ffi, link::Link, model::Outgoing};

/// How far ahead of the stream clock a fresh burst is scheduled, so the first packets arrive
/// before they are due.
const LEAD: Duration = Duration::from_millis(200);
/// How much is queued ahead of the stream clock before a write waits.
const QUEUE_AHEAD: Duration = Duration::from_millis(100);
const PACKET_SAMPLES: usize = 16_384;
const DRAIN_LIMIT: Duration = Duration::from_secs(2);
const WAIT_STEP: Duration = Duration::from_millis(5);

/// Where the transmitter sits, shared with the control side so a retune reaches the next packet.
#[derive(Debug, Default)]
pub struct Tune {
    center: AtomicU64,
    rate: AtomicU64,
}

impl Tune {
    pub fn set(&self, center_hz: f64, rate: f64) {
        self.center.store(center_hz.to_bits(), Ordering::Release);
        self.rate.store(rate.to_bits(), Ordering::Release);
    }

    #[must_use]
    pub fn center(&self) -> f64 {
        f64::from_bits(self.center.load(Ordering::Acquire))
    }

    #[must_use]
    pub fn rate(&self) -> f64 {
        f64::from_bits(self.rate.load(Ordering::Acquire))
    }
}

pub struct AaroniaTx {
    link: Arc<Link>,
    dev: DevHandle,
    duplex: Arc<Mutex<DuplexState>>,
    tune: Arc<Tune>,
    next: Option<f64>,
    queued_until: f64,
    iq: Vec<f32>,
    active: bool,
}

impl AaroniaTx {
    #[must_use]
    pub fn new(
        link: Arc<Link>,
        dev: DevHandle,
        duplex: Arc<Mutex<DuplexState>>,
        tune: Arc<Tune>,
    ) -> Self {
        Self {
            link,
            dev,
            duplex,
            tune,
            next: None,
            queued_until: 0.0,
            iq: Vec::with_capacity(2 * PACKET_SAMPLES),
            active: true,
        }
    }

    fn now(&self) -> Result<f64, DeviceError> {
        self.link.api.stream_time(self.dev)
    }

    /// Waits until the queue holds no more than [`QUEUE_AHEAD`] before `start`, or reports that
    /// `timeout` ran out first.
    fn wait_for_room(&self, start: f64, timeout: Duration) -> Result<bool, DeviceError> {
        let deadline = Instant::now() + timeout;
        loop {
            let ahead = start - self.now()? - QUEUE_AHEAD.as_secs_f64();
            if ahead <= 0.0 {
                return Ok(true);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(false);
            }
            std::thread::sleep(Duration::from_secs_f64(ahead).min(left));
        }
    }

    fn send(&mut self, chunk: &[Sample], flags: u64, start: f64) -> Result<f64, DeviceError> {
        let rate = self.tune.rate();
        let scale = tx_scale();
        self.iq.clear();
        self.iq.extend(
            chunk
                .iter()
                .flat_map(|sample| [sample.re * scale, sample.im * scale]),
        );
        let end = start + chunk.len() as f64 / rate;
        self.link.api.send(
            self.dev,
            &Outgoing {
                flags,
                start_time: start,
                end_time: end,
                start_hz: self.tune.center() - rate / 2.0,
                step_hz: rate,
                iq: &self.iq,
            },
        )?;
        Ok(end)
    }

    fn drain(&self) {
        let end = self.queued_until;
        let deadline = Instant::now() + DRAIN_LIMIT;
        while Instant::now() < deadline {
            match self.now() {
                Ok(now) if now < end => std::thread::sleep(WAIT_STEP),
                _ => return,
            }
        }
    }

    fn finish(&mut self) -> Result<(), DeviceError> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let api = &self.link.api;
        let stopped = api.stop(self.dev);
        let disconnected = api.disconnect(self.dev);
        lock(&self.duplex).release(Direction::Tx);
        stopped.and(disconnected)
    }
}

impl TxStream for AaroniaTx {
    fn write_channels(
        &mut self,
        channels: &[&[Sample]],
        timeout: Duration,
        end_burst: bool,
    ) -> Result<usize, DeviceError> {
        let [samples] = channels else {
            return Err(DeviceError::Unsupported(format!(
                "this device has 1 tx stream, got {} channels",
                channels.len()
            )));
        };
        if !self.active {
            return Err(DeviceError::Io("transmit stream is stopped".to_string()));
        }
        if samples.is_empty() {
            if end_burst {
                self.next = None;
            }
            return Ok(0);
        }
        let now = self.now()?;
        let (mut start, mut flags) = match self.next {
            Some(next) if next >= now => (next, 0),
            _ => (
                now + LEAD.as_secs_f64(),
                ffi::PACKET_STREAM_START | ffi::PACKET_SEGMENT_START,
            ),
        };
        if !self.wait_for_room(start, timeout)? {
            return Ok(0);
        }
        let count = samples.len().div_ceil(PACKET_SAMPLES);
        for (index, chunk) in samples.chunks(PACKET_SAMPLES).enumerate() {
            if end_burst && index + 1 == count {
                flags |= ffi::PACKET_SEGMENT_END | ffi::PACKET_STREAM_END;
            }
            start = self.send(chunk, flags, start)?;
            flags = 0;
        }
        self.queued_until = start;
        self.next = (!end_burst).then_some(start);
        Ok(samples.len())
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        if self.active {
            self.drain();
        }
        self.finish()
    }
}

impl Drop for AaroniaTx {
    fn drop(&mut self) {
        if let Err(error) = self.finish() {
            tracing::warn!(%error, "stopping the SPECTRAN transmitter");
        }
    }
}
