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

pub(crate) const BUFFER_S: f64 = 0.25;
const BACKLOG_S: f64 = 2.0;

static LISTENERS: LazyLock<Mutex<Vec<Weak<Antenna>>>> = LazyLock::new(|| Mutex::new(Vec::new()));
static NEXT_CARRIER: AtomicU64 = AtomicU64::new(1);
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// `at` on the bench's shared clock, which both sides of a half-duplex turn read.
pub(crate) fn ns_at(at: Instant) -> u64 {
    u64::try_from(at.saturating_duration_since(*EPOCH).as_nanos()).unwrap_or(u64::MAX)
}

fn ns_for(samples: usize, rate: f64) -> u64 {
    (samples as f64 / rate * 1e9).round() as u64
}

struct Heard {
    carrier: u64,
    center_hz: f64,
    rate: f64,
    samples: VecDeque<Sample>,
    /// When the first queued sample is on the air, on the bench's shared clock.
    airs_at_ns: u64,
    ended: bool,
    nco: Nco,
}

impl Heard {
    fn new(emission: &Emission<'_>) -> Self {
        Self {
            carrier: emission.carrier,
            center_hz: emission.drive.center_hz,
            rate: emission.drive.rate,
            samples: VecDeque::new(),
            airs_at_ns: emission.airs_at_ns,
            ended: false,
            nco: Nco::new(0.0, emission.drive.rate as f32),
        }
    }

    /// Where the queued samples start in a block of `len` due at `due_ns`, and how many it takes.
    /// Samples that should already be on the air start the block: heard late, never lost.
    fn place(&self, due_ns: u64, len: usize) -> Option<(usize, usize)> {
        let ahead_s = self.airs_at_ns.saturating_sub(due_ns) as f64 * 1e-9;
        let offset = (ahead_s * self.rate).round() as usize;
        (offset < len && !self.samples.is_empty())
            .then(|| (offset, (len - offset).min(self.samples.len())))
    }

    fn queue(&mut self, emission: &Emission<'_>) {
        if self.samples.is_empty() {
            self.airs_at_ns = emission.airs_at_ns;
        } else {
            let queued_until = self.airs_at_ns + ns_for(self.samples.len(), self.rate);
            let gap_s = emission.airs_at_ns.saturating_sub(queued_until) as f64 * 1e-9;
            let silence = (gap_s * self.rate).round() as usize;
            self.samples
                .extend(std::iter::repeat_n(Complex::new(0.0, 0.0), silence));
        }
        self.center_hz = emission.drive.center_hz;
        self.rate = emission.drive.rate;
        self.ended = emission.ended;
        self.samples.extend(emission.samples.iter().copied());
        let backlog = (BACKLOG_S * self.rate) as usize;
        if self.samples.len() > backlog {
            let excess = self.samples.len() - backlog;
            self.samples.drain(..excess);
            self.airs_at_ns += ns_for(excess, self.rate);
        }
    }
}

/// One virtual receive lane's ear on the bench's shared ether: what any virtual radio transmits,
/// every lane running at the same rate hears at the right offset and at the time it went out,
/// so a receiver catching up after a stall hears a burst whole rather than faster than it aired.
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

    /// Mixes in what was on the air while `block`, due at `due_ns` on the shared clock, was.
    pub(crate) fn add_to(&self, block: &mut [Sample], center_hz: f64, rate: f64, due_ns: u64) {
        let mut heard = lock(&self.heard);
        let mut scratch = lock(&self.scratch);
        let (raw, mixed) = &mut *scratch;
        for carrier in heard.iter_mut() {
            let Some((offset, take)) = carrier.place(due_ns, block.len()) else {
                continue;
            };
            raw.clear();
            raw.extend(carrier.samples.drain(..take));
            carrier.airs_at_ns = carrier.airs_at_ns.max(due_ns) + ns_for(take, carrier.rate);
            let offset_hz = carrier.center_hz - center_hz;
            if carrier.rate != rate || offset_hz.abs() >= rate / 2.0 {
                continue;
            }
            carrier.nco.set_turns(offset_hz / rate);
            mixed.resize(raw.len(), Complex::new(0.0, 0.0));
            carrier.nco.mix_into(raw, mixed);
            for (out, sample) in block[offset..].iter_mut().zip(mixed.iter()) {
                *out += sample;
            }
        }
        heard.retain(|carrier| !(carrier.ended && carrier.samples.is_empty()));
    }

    fn hear(&self, emission: &Emission<'_>) {
        let mut heard = lock(&self.heard);
        let index = heard
            .iter()
            .position(|entry| entry.carrier == emission.carrier)
            .unwrap_or_else(|| {
                heard.push(Heard::new(emission));
                heard.len() - 1
            });
        heard[index].queue(emission);
    }
}

struct Emission<'a> {
    carrier: u64,
    drive: Drive,
    samples: &'a [Sample],
    /// When the first sample goes on the air, on the bench's shared clock.
    airs_at_ns: u64,
    ended: bool,
}

fn emit(emission: &Emission<'_>, deaf: &[Arc<Antenna>]) {
    let listeners: Vec<Arc<Antenna>> = lock(&LISTENERS).iter().filter_map(Weak::upgrade).collect();
    for listener in listeners {
        if deaf.iter().any(|own| Arc::ptr_eq(own, &listener)) {
            continue;
        }
        listener.hear(emission);
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
                airs_at_ns: ns_at(self.airs_until),
                ended: true,
            };
            emit(&ending, &self.deaf);
        }
        release();
    }

    /// Holds a half-duplex receiver off until `until_ns`. A write first holds it off indefinitely,
    /// so no block due after the burst starts slips through before the burst's end is known.
    fn hold_receiver(&self, until_ns: u64) {
        if let Some(quiet_until) = &self.quiet_until {
            quiet_until.store(until_ns, Ordering::Release);
        }
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
        self.hold_receiver(u64::MAX);
        let accepted = len.min(self.room(drive.rate, timeout));
        let airs_at_ns = ns_at(self.airs_until);
        self.airs_until += Duration::from_secs_f64(accepted as f64 / drive.rate);
        self.hold_receiver(ns_at(self.airs_until));
        let ended = end_burst && accepted == len;
        for (carrier, samples) in self.carriers.iter().zip(channels) {
            self.scaled.clear();
            self.scaled
                .extend(samples[..accepted].iter().map(|sample| sample * drive.gain));
            let emission = Emission {
                carrier: *carrier,
                drive,
                samples: &self.scaled,
                airs_at_ns,
                ended,
            };
            emit(&emission, &self.deaf);
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

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f64 = 1_000.0;
    const SECOND_NS: u64 = 1_000_000_000;
    const BLOCK: usize = 50;

    fn burst(airs_at_ns: u64, samples: &[Sample]) -> Antenna {
        let antenna = Antenna::default();
        antenna.hear(&Emission {
            carrier: 1,
            drive: Drive {
                center_hz: 0.0,
                rate: RATE,
                gain: 1.0,
            },
            samples,
            airs_at_ns,
            ended: true,
        });
        antenna
    }

    fn block(antenna: &Antenna, due_ns: u64) -> Vec<f32> {
        let mut block = vec![Complex::new(0.0, 0.0); BLOCK];
        antenna.add_to(&mut block, 0.0, RATE, due_ns);
        block.iter().map(|sample| sample.re).collect()
    }

    #[test]
    fn a_receiver_catching_up_hears_a_burst_at_the_pace_it_aired() {
        let samples: Vec<Sample> = (1..=100).map(|n| Complex::new(n as f32, 0.0)).collect();
        let antenna = burst(SECOND_NS, &samples);

        assert!(
            block(&antenna, SECOND_NS - 50_000_000)
                .iter()
                .all(|v| *v == 0.0)
        );
        let straddling = block(&antenna, SECOND_NS - 20_000_000);
        assert!(straddling[..20].iter().all(|v| *v == 0.0));
        assert_eq!(straddling[20], 1.0);
        assert_eq!(straddling[49], 30.0);
        let next = block(&antenna, SECOND_NS + 30_000_000);
        assert_eq!(next[0], 31.0);
        assert_eq!(next[49], 80.0);
    }

    #[test]
    fn a_burst_already_due_starts_the_next_block_whole() {
        let samples = vec![Complex::new(1.0, 0.0); 80];
        let antenna = burst(SECOND_NS, &samples);

        let late = block(&antenna, 2 * SECOND_NS);
        assert!(late.iter().all(|v| *v == 1.0));
        let rest = block(&antenna, 2 * SECOND_NS + 50_000_000);
        assert!(rest[..30].iter().all(|v| *v == 1.0));
        assert!(rest[30..].iter().all(|v| *v == 0.0));
    }
}
