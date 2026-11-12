use std::{
    collections::VecDeque,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_device::{DeviceError, Sample, TxStream, lock};
use sdrmm_dsp::Nco;

const PREFILL_S: f64 = 0.03;
const BUFFER_S: f64 = 0.25;
const BACKLOG_S: f64 = 2.0;

static LISTENERS: LazyLock<Mutex<Vec<Weak<Antenna>>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static NEXT_CARRIER: AtomicU64 = AtomicU64::new(1);
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

pub(crate) fn now_ns() -> u64 {
    u64::try_from(EPOCH.elapsed().as_nanos()).unwrap_or(u64::MAX)
}

struct Heard {
    carrier: u64,
    center_hz: f64,
    rate: f64,
    samples: VecDeque<Sample>,
    ended: bool,
    playing: bool,
    nco: Nco,
}

/// One virtual receive lane's ear on the bench's shared ether: what any virtual radio transmits,
/// every lane running at the same rate hears at the right offset.
#[derive(Default)]
pub(crate) struct Antenna {
    heard: Mutex<Vec<Heard>>,
    scratch: Mutex<(Vec<Sample>, Vec<Sample>)>,
}

impl Antenna {
    pub(crate) fn join() -> Arc<Self> {
        let antenna = Arc::new(Self::default());
        let mut listeners = lock(&LISTENERS);
        listeners.retain(|listener| listener.strong_count() > 0);
        listeners.push(Arc::downgrade(&antenna));
        antenna
    }

    pub(crate) fn add_to(&self, block: &mut [Sample], center_hz: f64, rate: f64) {
        let mut heard = lock(&self.heard);
        let mut scratch = lock(&self.scratch);
        let (raw, mixed) = &mut *scratch;
        for carrier in heard.iter_mut() {
            if !carrier.playing {
                let prefill = (PREFILL_S * rate) as usize;
                if carrier.samples.len() < prefill && !carrier.ended {
                    continue;
                }
                carrier.playing = true;
            }
            let take = block.len().min(carrier.samples.len());
            raw.clear();
            raw.extend(carrier.samples.drain(..take));
            if carrier.samples.is_empty() {
                carrier.playing = false;
            }
            let offset_hz = carrier.center_hz - center_hz;
            if carrier.rate != rate || offset_hz.abs() >= rate / 2.0 {
                continue;
            }
            carrier.nco.set_turns(offset_hz / rate);
            mixed.resize(raw.len(), Complex::new(0.0, 0.0));
            carrier.nco.mix_into(raw, mixed);
            for (out, sample) in block.iter_mut().zip(mixed.iter()) {
                *out += sample;
            }
        }
        heard.retain(|carrier| !(carrier.ended && carrier.samples.is_empty()));
    }

    fn hear(&self, carrier: u64, center_hz: f64, rate: f64, samples: &[Sample], ended: bool) {
        let mut heard = lock(&self.heard);
        let index = heard
            .iter()
            .position(|entry| entry.carrier == carrier)
            .unwrap_or_else(|| {
                heard.push(Heard {
                    carrier,
                    center_hz,
                    rate,
                    samples: VecDeque::new(),
                    ended: false,
                    playing: false,
                    nco: Nco::new(0.0, rate as f32),
                });
                heard.len() - 1
            });
        let entry = &mut heard[index];
        entry.center_hz = center_hz;
        entry.rate = rate;
        entry.ended = ended;
        entry.samples.extend(samples.iter().copied());
        let backlog = (BACKLOG_S * rate) as usize;
        if entry.samples.len() > backlog {
            let excess = entry.samples.len() - backlog;
            entry.samples.drain(..excess);
        }
    }
}

struct Emission<'a> {
    carrier: u64,
    drive: Drive,
    samples: &'a [Sample],
    ended: bool,
}

fn emit(emission: &Emission<'_>, deaf: &[Arc<Antenna>]) {
    let listeners: Vec<Arc<Antenna>> = lock(&LISTENERS).iter().filter_map(Weak::upgrade).collect();
    for listener in listeners {
        if deaf.iter().any(|own| Arc::ptr_eq(own, &listener)) {
            continue;
        }
        listener.hear(
            emission.carrier,
            emission.drive.center_hz,
            emission.drive.rate,
            emission.samples,
            emission.ended,
        );
    }
}

/// Where a virtual transmitter sits and how hard it drives, read at every write.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Drive {
    pub center_hz: f64,
    pub rate: f64,
    pub gain: f32,
}

/// A virtual transmitter: it radiates into the bench's ether no faster than real time, like a
/// radio whose buffer fills.
pub(crate) struct VirtualTx {
    carriers: Vec<u64>,
    drive: Box<dyn Fn() -> Drive + Send>,
    release: Option<Box<dyn FnOnce() + Send>>,
    quiet_until: Option<Arc<AtomicU64>>,
    deaf: Vec<Arc<Antenna>>,
    airs_until: Instant,
    scaled: Vec<Sample>,
}

/// What a half-duplex radio gives up while it transmits: its receiver holds off until
/// `quiet_until`, and its own antennas never hear the burst.
pub(crate) struct Turns {
    pub quiet_until: Arc<AtomicU64>,
    pub antennas: Vec<Arc<Antenna>>,
}

impl VirtualTx {
    pub(crate) fn new(
        channels: usize,
        drive: impl Fn() -> Drive + Send + 'static,
        turns: Option<Turns>,
        release: impl FnOnce() + Send + 'static,
    ) -> Self {
        let (quiet_until, deaf) = match turns {
            Some(turns) => (Some(turns.quiet_until), turns.antennas),
            None => (None, Vec::new()),
        };
        Self {
            carriers: (0..channels)
                .map(|_| NEXT_CARRIER.fetch_add(1, Ordering::Relaxed))
                .collect(),
            drive: Box::new(drive),
            release: Some(Box::new(release)),
            quiet_until,
            deaf,
            airs_until: Instant::now(),
            scaled: Vec::new(),
        }
    }

    fn finish(&mut self) {
        let Some(release) = self.release.take() else {
            return;
        };
        let drive = (self.drive)();
        for carrier in &self.carriers {
            let ending = Emission {
                carrier: *carrier,
                drive,
                samples: &[],
                ended: true,
            };
            emit(&ending, &self.deaf);
        }
        release();
    }

    fn room(&mut self, rate: f64, timeout: Duration) -> usize {
        let buffer = Duration::from_secs_f64(BUFFER_S);
        let now = Instant::now();
        if self.airs_until < now {
            self.airs_until = now;
        }
        let full_for = self.airs_until.saturating_duration_since(now + buffer);
        if !full_for.is_zero() {
            std::thread::sleep(full_for.min(timeout));
        }
        let horizon = Instant::now() + buffer;
        (horizon
            .saturating_duration_since(self.airs_until)
            .as_secs_f64()
            * rate) as usize
    }
}

impl TxStream for VirtualTx {
    fn write_channels(
        &mut self,
        channels: &[&[Sample]],
        timeout: Duration,
        end_burst: bool,
    ) -> Result<usize, DeviceError> {
        if channels.len() != self.carriers.len() {
            return Err(DeviceError::Unsupported(format!(
                "this stream carries {} tx channels, got {}",
                self.carriers.len(),
                channels.len()
            )));
        }
        let len = channels.first().map_or(0, |samples| samples.len());
        if channels.iter().any(|samples| samples.len() != len) {
            return Err(DeviceError::Unsupported(
                "tx channels must carry the same number of samples".to_string(),
            ));
        }
        let drive = (self.drive)();
        let accepted = len.min(self.room(drive.rate, timeout));
        let ended = end_burst && accepted == len;
        for (carrier, samples) in self.carriers.iter().zip(channels) {
            self.scaled.clear();
            self.scaled
                .extend(samples[..accepted].iter().map(|sample| sample * drive.gain));
            let emission = Emission {
                carrier: *carrier,
                drive,
                samples: &self.scaled,
                ended,
            };
            emit(&emission, &self.deaf);
        }
        self.airs_until += Duration::from_secs_f64(accepted as f64 / drive.rate);
        if let Some(quiet_until) = &self.quiet_until {
            let airs_in = self.airs_until.saturating_duration_since(Instant::now());
            let until = now_ns().saturating_add(airs_in.as_nanos() as u64);
            quiet_until.store(until, Ordering::Release);
        }
        Ok(accepted)
    }

    fn stop(&mut self) -> Result<(), DeviceError> {
        self.finish();
        Ok(())
    }
}

impl Drop for VirtualTx {
    fn drop(&mut self) {
        self.finish();
    }
}
