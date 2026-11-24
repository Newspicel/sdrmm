use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use arc_swap::ArcSwapOption;
use num_complex::Complex;
use rtrb::{Consumer, Producer, RingBuffer};
use sdrmm_device::TxStream;
use sdrmm_dsp::{Duc, EmissionMeter};
use sdrmm_wire::{TransmitPhase, TransmitSettings, TransmitStatus};

type Sample = Complex<f32>;

const TICK: Duration = Duration::from_millis(5);
const BLOCK_S: f64 = 0.02;
const PREFILL_S: f64 = 0.06;
const TAIL_S: f64 = 0.005;
const FEED_RING_S: f64 = 0.5;
const FEED_RING_MIN: usize = 1 << 16;
const HANG: Duration = Duration::from_millis(120);
const WRITE_TIMEOUT: Duration = Duration::from_millis(100);
const STALL: Duration = Duration::from_secs(3);
const SILENT_DB: f32 = -120.0;
const MEASURE_BLOCKS: usize = 16;

/// Where a stream of samples sits: its centre and its rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Band {
    pub(crate) center_hz: f64,
    pub(crate) sample_rate: f64,
}

pub(crate) enum TxCommand {
    Radio(Band),
    Feed(Band),
    Settings(TransmitSettings),
}

/// What the worker reports, read by snapshots without stopping it.
#[derive(Default)]
pub(crate) struct TxShared {
    on_air: AtomicBool,
    samples: AtomicU64,
    bursts: AtomicU64,
    underruns: AtomicU64,
    peak: AtomicU32,
    occupied: AtomicU64,
    channel: AtomicU64,
    frequency: AtomicU64,
    sample_rate: AtomicU64,
    error: ArcSwapOption<String>,
    stopped: AtomicBool,
}

impl TxShared {
    pub(crate) fn status(&self, node: &str, settings: &TransmitSettings) -> TransmitStatus {
        let peak = f32::from_bits(self.peak.load(Ordering::Relaxed));
        TransmitStatus {
            node: node.to_owned(),
            phase: if self.on_air.load(Ordering::Acquire) {
                TransmitPhase::OnAir
            } else {
                TransmitPhase::Idle
            },
            frequency_hz: f64::from_bits(self.frequency.load(Ordering::Relaxed)),
            sample_rate: f64::from_bits(self.sample_rate.load(Ordering::Relaxed)),
            settings: settings.clone(),
            samples: self.samples.load(Ordering::Relaxed),
            bursts: self.bursts.load(Ordering::Relaxed),
            underruns: self.underruns.load(Ordering::Relaxed),
            peak_db: (peak > 0.0).then(|| (20.0 * peak.log10()).max(SILENT_DB)),
            occupied_hz: width(&self.occupied),
            channel_hz: width(&self.channel),
            error: self.error.load().as_deref().cloned(),
        }
    }

    /// The width the modulator's mode is meant to stay inside.
    pub(crate) fn declare_channel(&self, width_hz: f64) {
        self.channel.store(width_hz.to_bits(), Ordering::Relaxed);
    }

    pub(crate) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn fail(&self, reason: String) {
        self.error.store(Some(Arc::new(reason)));
    }
}

fn width(bits: &AtomicU64) -> Option<f64> {
    Some(f64::from_bits(bits.load(Ordering::Relaxed))).filter(|hz| *hz > 0.0)
}

/// The lane side of a feed: the DSP thread pushes here and never waits.
pub(crate) struct TxFeedTap {
    samples: Producer<Sample>,
    shared: Arc<TxShared>,
}

impl TxFeedTap {
    /// False once the transmitter is gone, so the lane lets go of the tap.
    #[must_use]
    pub(crate) fn push(&mut self, samples: &[Sample]) -> bool {
        if self.samples.is_abandoned() {
            return false;
        }
        if self.samples.push_entire_slice(samples).is_err() {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
        true
    }
}

pub(crate) fn feed(band: Band, shared: &Arc<TxShared>) -> (TxFeedTap, Source) {
    let capacity = ((band.sample_rate * FEED_RING_S) as usize).max(FEED_RING_MIN);
    let (producer, consumer) = RingBuffer::new(capacity);
    let tap = TxFeedTap {
        samples: producer,
        shared: shared.clone(),
    };
    (
        tap,
        Source::Feed {
            ring: consumer,
            band,
        },
    )
}

/// The encoder side of a transmitter: the encoder pushes each keyed stretch here, already
/// paced to real time, and says when its burst is over.
pub(crate) struct AirTap {
    samples: Producer<Sample>,
    keyed: Arc<AtomicBool>,
    shared: Arc<TxShared>,
}

impl AirTap {
    /// Hands over the first `keyed` samples of `block`; fewer than all of them ends the burst.
    /// False once the transmitter is gone.
    #[must_use]
    pub(crate) fn push(&mut self, block: &[Sample], keyed: usize) -> bool {
        if self.samples.is_abandoned() {
            return false;
        }
        if keyed > 0 {
            if self.samples.push_entire_slice(&block[..keyed]).is_err() {
                self.shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
            self.keyed.store(true, Ordering::Release);
        }
        if keyed < block.len() {
            self.keyed.store(false, Ordering::Release);
        }
        true
    }
}

/// A transmitter fed by an encoder running at `rate`.
pub(crate) fn encoded(rate: f64, shared: &Arc<TxShared>) -> (AirTap, Source) {
    let capacity = ((rate * FEED_RING_S) as usize).max(FEED_RING_MIN);
    let (producer, consumer) = RingBuffer::new(capacity);
    let keyed = Arc::new(AtomicBool::new(false));
    let tap = AirTap {
        samples: producer,
        keyed: keyed.clone(),
        shared: shared.clone(),
    };
    (
        tap,
        Source::Encoded {
            ring: consumer,
            rate,
            keyed,
        },
    )
}

/// An air tap and a reader of what reached it: the samples keyed so far and whether the
/// encoder still keys.
#[cfg(test)]
pub(crate) fn encoded_for_tests(
    rate: f64,
    shared: &Arc<TxShared>,
) -> (AirTap, impl FnMut() -> (usize, bool)) {
    let (tap, source) = encoded(rate, shared);
    let Source::Encoded {
        mut ring, keyed, ..
    } = source
    else {
        unreachable!("an encoded source");
    };
    let mut seen = 0;
    let read = move || {
        seen += ring.slots();
        if let Ok(chunk) = ring.read_chunk(ring.slots()) {
            chunk.commit_all();
        }
        (seen, keyed.load(Ordering::Acquire))
    };
    (tap, read)
}

pub(crate) enum Source {
    Encoded {
        ring: Consumer<Sample>,
        rate: f64,
        keyed: Arc<AtomicBool>,
    },
    Feed {
        ring: Consumer<Sample>,
        band: Band,
    },
}

impl Source {
    fn rate(&self) -> f64 {
        match self {
            Self::Encoded { rate, .. } => *rate,
            Self::Feed { band, .. } => band.sample_rate,
        }
    }

    fn center_hz(&self) -> Option<f64> {
        match self {
            Self::Encoded { .. } => None,
            Self::Feed { band, .. } => Some(band.center_hz),
        }
    }

    /// Whether the burst on the air is over: the encoder unkeyed and everything it sent is out,
    /// or a lane left nothing waiting for the hang time since the radio last took its samples.
    fn over(&self, fed: Instant) -> bool {
        match self {
            Self::Encoded { ring, keyed, .. } => {
                !keyed.load(Ordering::Acquire) && ring.slots() == 0
            }
            Self::Feed { ring, .. } => ring.slots() == 0 && fed.elapsed() > HANG,
        }
    }

    fn fill(&mut self, out: &mut [Sample], on_air: bool) -> usize {
        match self {
            Self::Encoded { ring, .. } => read(ring, out),
            Self::Feed { ring, band } => {
                let prefill = (PREFILL_S * band.sample_rate) as usize;
                if !on_air && ring.slots() < prefill {
                    return 0;
                }
                read(ring, out)
            }
        }
    }
}

fn read(ring: &mut Consumer<Sample>, out: &mut [Sample]) -> usize {
    let take = ring.slots().min(out.len());
    let Ok(chunk) = ring.read_chunk(take) else {
        return 0;
    };
    let (first, second) = chunk.as_slices();
    out[..first.len()].copy_from_slice(first);
    out[first.len()..take].copy_from_slice(second);
    chunk.commit_all();
    take
}

struct Burst {
    started: Instant,
    produced: u64,
    fed: Instant,
    peak: f32,
}

pub(crate) struct Worker {
    stream: Box<dyn TxStream>,
    source: Source,
    radio: Band,
    settings: TransmitSettings,
    shared: Arc<TxShared>,
    duc: Option<Duc>,
    gain: f32,
    burst: Option<Burst>,
    baseband: Vec<Sample>,
    out: Vec<Sample>,
    meter: EmissionMeter,
}

impl Worker {
    pub(crate) fn new(
        stream: Box<dyn TxStream>,
        source: Source,
        radio: Band,
        settings: TransmitSettings,
        shared: Arc<TxShared>,
    ) -> Self {
        let mut worker = Self {
            stream,
            source,
            radio,
            settings,
            shared,
            duc: None,
            gain: 1.0,
            burst: None,
            baseband: Vec::new(),
            out: Vec::new(),
            meter: EmissionMeter::new(1.0),
        };
        worker.plan();
        worker
    }

    pub(crate) fn run(mut self, commands: &mpsc::Receiver<TxCommand>, stop: &AtomicBool) {
        while !stop.load(Ordering::Acquire) {
            if !self.drain(commands) {
                break;
            }
            match self.step() {
                Ok(true) => {}
                Ok(false) => std::thread::sleep(TICK),
                Err(reason) => {
                    tracing::warn!(%reason, "transmitter stopped");
                    self.shared.fail(reason);
                    break;
                }
            }
        }
        self.close();
    }

    fn close(&mut self) {
        if self.burst.is_some()
            && let Err(reason) = self.end_burst()
        {
            tracing::debug!(%reason, "the last burst did not end cleanly");
        }
        if let Err(error) = self.stream.stop() {
            tracing::debug!(%error, "transmit stream stop failed");
        }
        self.shared.on_air.store(false, Ordering::Release);
        self.shared.stopped.store(true, Ordering::Release);
    }

    fn drain(&mut self, commands: &mpsc::Receiver<TxCommand>) -> bool {
        loop {
            match commands.try_recv() {
                Ok(command) => self.obey(command),
                Err(mpsc::TryRecvError::Empty) => return true,
                Err(mpsc::TryRecvError::Disconnected) => return false,
            }
        }
    }

    fn obey(&mut self, command: TxCommand) {
        match command {
            TxCommand::Radio(radio) => {
                self.radio = radio;
                self.replan();
            }
            TxCommand::Feed(moved) => {
                if let Source::Feed { band, .. } = &mut self.source {
                    *band = moved;
                }
                self.replan();
            }
            TxCommand::Settings(settings) => {
                let moved = settings.frequency_hz != self.settings.frequency_hz;
                self.settings = settings;
                if moved {
                    self.replan();
                } else {
                    self.gain = gain_of(&self.settings);
                }
            }
        }
    }

    fn replan(&mut self) {
        if self.burst.is_some()
            && let Err(reason) = self.end_burst()
        {
            self.shared.fail(reason);
        }
        self.plan();
    }

    /// Places the source in the radio's band, or leaves the transmitter mute with the reason
    /// when it does not fit.
    fn plan(&mut self) {
        let source_rate = self.source.rate();
        let frequency = self
            .settings
            .frequency_hz
            .or_else(|| self.source.center_hz())
            .unwrap_or(self.radio.center_hz);
        let offset = frequency - self.radio.center_hz;
        let room = (self.radio.sample_rate - source_rate).max(0.0) / 2.0;
        self.gain = gain_of(&self.settings);
        self.shared
            .frequency
            .store(frequency.to_bits(), Ordering::Relaxed);
        self.shared
            .sample_rate
            .store(self.radio.sample_rate.to_bits(), Ordering::Relaxed);
        self.duc = None;
        self.meter = EmissionMeter::new(source_rate);
        if offset.abs() > room + 1.0 {
            self.shared.fail(format!(
                "{frequency} Hz is {offset:+.0} Hz from the radio's centre; it reaches {room:.0} Hz"
            ));
            return;
        }
        match Duc::new(source_rate, self.radio.sample_rate, offset) {
            Ok(duc) => {
                self.duc = Some(duc);
                self.shared.error.store(None);
            }
            Err(error) => self.shared.fail(error.to_string()),
        }
    }

    #[expect(
        clippy::large_stack_arrays,
        reason = "stack scratch keeps the TX loop allocation free"
    )]
    fn step(&mut self) -> Result<bool, String> {
        if self.duc.is_none() {
            let mut scrap = [Sample::new(0.0, 0.0); 4_096];
            return Ok(self.source.fill(&mut scrap, true) > 0);
        }
        let block = ((BLOCK_S * self.source.rate()) as usize).max(1);
        let want = if self.burst.is_some() {
            4 * block
        } else {
            block
        };
        self.baseband.resize(want, Sample::new(0.0, 0.0));
        let got = self.source.fill(&mut self.baseband, self.burst.is_some());
        if got > 0 {
            self.send(got)?;
        }
        let Some((started, fed)) = self.burst.as_ref().map(|burst| (burst.started, burst.fed))
        else {
            return Ok(false);
        };
        if self.source.over(fed) {
            self.end_burst()?;
            return Ok(true);
        }
        let limit = Duration::from_secs(u64::from(self.settings.timeout_s));
        if started.elapsed() > limit {
            self.end_burst()?;
            return Err(format!(
                "on the air for {} s; disarmed",
                self.settings.timeout_s
            ));
        }
        Ok(got > 0)
    }

    fn send(&mut self, got: usize) -> Result<(), String> {
        let now = Instant::now();
        let burst = self.burst.get_or_insert_with(|| {
            self.shared.on_air.store(true, Ordering::Release);
            self.shared.bursts.fetch_add(1, Ordering::Relaxed);
            Burst {
                started: now,
                produced: 0,
                fed: now,
                peak: 0.0,
            }
        });
        burst.produced += got as u64;
        for sample in &mut self.baseband[..got] {
            *sample *= self.gain;
            burst.peak = burst.peak.max(sample.norm());
        }
        self.shared
            .peak
            .store(burst.peak.to_bits(), Ordering::Relaxed);
        self.meter.push(&self.baseband[..got]);
        if self.meter.blocks() >= MEASURE_BLOCKS {
            self.measure();
        }
        if let Some(duc) = &mut self.duc {
            duc.process(&self.baseband[..got], &mut self.out);
        }
        let written = write_all(self.stream.as_mut(), &self.shared, &self.out, false);
        if let Some(burst) = &mut self.burst {
            burst.fed = Instant::now();
        }
        written
    }

    /// Runs the converter's tail out so the key-down ramp reaches the air, then tells the radio
    /// the burst is over.
    /// Publishes how wide the signal has been since the last measurement.
    fn measure(&mut self) {
        if let Some(occupied) = self.meter.occupied_hz() {
            self.shared
                .occupied
                .store(occupied.to_bits(), Ordering::Relaxed);
        }
        self.meter.reset();
    }

    fn end_burst(&mut self) -> Result<(), String> {
        self.measure();
        self.burst = None;
        let tail = (TAIL_S * self.source.rate()) as usize;
        self.baseband.clear();
        self.baseband.resize(tail, Sample::new(0.0, 0.0));
        self.out.clear();
        if let Some(duc) = &mut self.duc {
            duc.process(&self.baseband, &mut self.out);
            duc.reset();
        }
        let written = write_all(self.stream.as_mut(), &self.shared, &self.out, true);
        self.shared.on_air.store(false, Ordering::Release);
        written
    }
}

fn gain_of(settings: &TransmitSettings) -> f32 {
    10f64.powf(settings.level_db / 20.0) as f32
}

fn write_all(
    stream: &mut dyn TxStream,
    shared: &TxShared,
    samples: &[Sample],
    end_burst: bool,
) -> Result<(), String> {
    let mut sent = 0;
    let mut waited = Instant::now();
    loop {
        let accepted = stream
            .write(&samples[sent..], WRITE_TIMEOUT, end_burst)
            .map_err(|error| error.to_string())?;
        sent += accepted;
        shared.samples.fetch_add(accepted as u64, Ordering::Relaxed);
        if sent == samples.len() {
            return Ok(());
        }
        if accepted > 0 {
            waited = Instant::now();
        } else if waited.elapsed() > STALL {
            return Err("the radio stopped taking samples".to_owned());
        }
    }
}

#[cfg(test)]
mod tests;
