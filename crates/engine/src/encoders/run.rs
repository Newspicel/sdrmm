//! One encoder's clock: it turns queued audio and messages into baseband at its own rate, paced to
//! real time, and hands every block to the sinks on its output.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use num_complex::Complex;
use sdrmm_decoder::encoder::{Modulator, TxPayload};

use crate::{network_export::NetworkExportTap, recording::RecorderTap, transmit::AirTap};

type Sample = Complex<f32>;

const TICK: Duration = Duration::from_millis(5);
const BLOCK_S: f64 = 0.02;
/// How far live audio runs ahead of real time: its latency, so kept short.
pub(crate) const LEAD_S: f64 = 0.06;
/// A message is known whole up front, so it runs further ahead and rides out a scheduler stall
/// this long without a hole in the burst.
pub(crate) const MESSAGE_LEAD_S: f64 = 0.2;
/// Audio held until this much has gathered, so arrival jitter leaves no hole.
pub(crate) const PREFILL_S: f64 = 0.06;
/// How long late audio is bridged with silence before the burst ends.
pub(crate) const HANG: Duration = Duration::from_millis(120);
const MAX_HELD_AUDIO_S: f64 = 5.0;
/// A stall longer than this is skipped rather than caught up, and counted.
const MAX_BEHIND_S: f64 = 1.0;

pub(crate) enum Command {
    Payload(TxPayload),
    Retune(Box<dyn Modulator>),
    Attach(Sink),
    Detach(SinkKind),
}

/// Where an encoder's baseband goes. A transmitter takes only the keyed stretches; a recorder
/// and a network export take the stream whole, silence between bursts included.
pub(crate) enum Sink {
    Air(AirTap),
    Record { tap: RecorderTap, at: u64 },
    Export(NetworkExportTap),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SinkKind {
    Record,
    Export,
}

impl Sink {
    const fn kind(&self) -> Option<SinkKind> {
        match self {
            Self::Air(_) => None,
            Self::Record { .. } => Some(SinkKind::Record),
            Self::Export(_) => Some(SinkKind::Export),
        }
    }

    /// False once the sink is gone for good.
    fn push(&mut self, block: &[Sample], keyed: usize) -> bool {
        match self {
            Self::Air(tap) => tap.push(block, keyed),
            Self::Record { tap, at } => {
                let pushed = tap.push(block, *at, 0.0);
                *at += block.len() as u64;
                pushed || tap.healthy()
            }
            Self::Export(tap) => {
                let _shown_in_status = tap.push(block);
                true
            }
        }
    }
}

/// What a snapshot reads without stopping the encoder.
#[derive(Default)]
pub(crate) struct EncoderShared {
    pub(crate) produced: AtomicU64,
    pub(crate) skipped: AtomicU64,
    pub(crate) error: arc_swap::ArcSwapOption<String>,
}

pub(crate) struct Run {
    tx: Box<dyn Modulator>,
    rate: f64,
    sinks: Vec<Sink>,
    shared: Arc<EncoderShared>,
    started: Instant,
    produced: u64,
    block: Vec<Sample>,
    /// The last block ended keyed, so the burst runs a lead ahead of real time.
    on_air: bool,
    held_audio: Vec<f32>,
    flowing: bool,
    voiced: i64,
    heard: Instant,
}

impl Run {
    pub(crate) fn new(tx: Box<dyn Modulator>, rate: f64, shared: Arc<EncoderShared>) -> Self {
        Self {
            tx,
            rate,
            sinks: Vec::new(),
            shared,
            started: Instant::now(),
            produced: 0,
            block: Vec::new(),
            on_air: false,
            held_audio: Vec::new(),
            flowing: false,
            voiced: 0,
            heard: Instant::now(),
        }
    }

    pub(crate) fn run(mut self, commands: &mpsc::Receiver<Command>, stop: &AtomicBool) {
        while !stop.load(Ordering::Acquire) {
            loop {
                match commands.try_recv() {
                    Ok(command) => self.obey(command),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => return,
                }
            }
            if !self.step() {
                std::thread::sleep(TICK);
            }
        }
    }

    fn obey(&mut self, command: Command) {
        match command {
            Command::Payload(TxPayload::Audio(pcm)) => self.hold_audio(&pcm),
            Command::Payload(frame) => self.submit(frame),
            Command::Retune(tx) => self.tx = tx,
            Command::Attach(sink) => {
                let kind = sink.kind();
                self.sinks
                    .retain(|held| kind.is_none() || held.kind() != kind);
                self.sinks.push(sink);
            }
            Command::Detach(kind) => self.sinks.retain(|held| held.kind() != Some(kind)),
        }
    }

    fn submit(&mut self, payload: TxPayload) {
        match self.tx.submit(payload) {
            Ok(()) => self.shared.error.store(None),
            Err(error) => self.shared.error.store(Some(Arc::new(error.to_string()))),
        }
    }

    fn hold_audio(&mut self, pcm: &[f32]) {
        let limit = (MAX_HELD_AUDIO_S * self.rate) as usize;
        if self.held_audio.len() + pcm.len() > limit {
            self.shared
                .skipped
                .fetch_add(pcm.len() as u64, Ordering::Relaxed);
            return;
        }
        self.held_audio.extend_from_slice(pcm);
        self.heard = Instant::now();
    }

    /// Hands held audio to the modulator once enough has gathered to ride out arrival jitter, or
    /// the speaker has already stopped.
    fn release_audio(&mut self) {
        if self.held_audio.is_empty() {
            return;
        }
        let prefill = (PREFILL_S * self.rate) as usize;
        if self.flowing || self.held_audio.len() >= prefill || self.heard.elapsed() > HANG {
            self.flowing = true;
            let pcm = std::mem::take(&mut self.held_audio);
            self.voiced += pcm.len() as i64;
            self.submit(TxPayload::Audio(pcm));
        }
    }

    /// Covers audio that is late by less than the hang time with silence, so a speaker's
    /// jitter leaves a short quiet patch instead of ending the burst.
    fn bridge(&mut self, want: usize) {
        if !self.flowing || self.heard.elapsed() > HANG {
            return;
        }
        let short = want as i64 - self.voiced;
        if short > 0 {
            self.voiced += short;
            self.submit(TxPayload::Audio(vec![0.0; short as usize]));
        }
    }

    /// How many samples are due now, skipping past a stall too long to catch up. Silence runs
    /// at real time; a burst runs its lead ahead, so it starts with that much in hand.
    fn due(&mut self) -> usize {
        let lead = match (self.on_air, self.flowing) {
            (false, _) => 0.0,
            (true, true) => LEAD_S,
            (true, false) => MESSAGE_LEAD_S,
        };
        let due = ((self.started.elapsed().as_secs_f64() + lead) * self.rate) as u64;
        let behind = due.saturating_sub(self.produced);
        if behind as f64 > MAX_BEHIND_S * self.rate {
            let skipped = behind - (lead * self.rate) as u64;
            self.produced += skipped;
            self.shared.skipped.fetch_add(skipped, Ordering::Relaxed);
        }
        let block = ((BLOCK_S * self.rate) as usize).max(1);
        (due.saturating_sub(self.produced) as usize).min(4 * block)
    }

    /// Makes what is due and hands it on. True while it is still catching up.
    fn step(&mut self) -> bool {
        self.release_audio();
        let want = self.due();
        if want == 0 {
            return false;
        }
        self.bridge(want);
        self.block.resize(want, Sample::new(0.0, 0.0));
        let keyed = self.tx.generate(&mut self.block);
        self.block[keyed..].fill(Sample::new(0.0, 0.0));
        if self.flowing {
            self.voiced -= keyed as i64;
        }
        if keyed < want {
            self.flowing = false;
            self.voiced = 0;
        }
        let keying = keyed == want && !self.on_air;
        self.on_air = keyed == want;
        let block = &self.block;
        self.sinks.retain_mut(|sink| sink.push(block, keyed));
        self.produced += want as u64;
        self.shared.produced.store(self.produced, Ordering::Relaxed);
        let full = ((BLOCK_S * self.rate) as usize).max(1) * 4;
        want == full || keying
    }
}

#[cfg(test)]
mod tests;
