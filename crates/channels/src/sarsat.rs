mod demod;
pub(crate) mod message;

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_dsp::{Decimator, SyncDetector, design_lowpass};
use sdrmm_wire::{
    ChannelDescriptor, ChannelParams, ChannelSettings, DecoderEvent, DecoderFamily,
    MAX_SARSAT_BANDWIDTH_HZ, MIN_SARSAT_BANDWIDTH_HZ, SarsatParams,
};

use crate::{ChannelCtx, ChannelError, ChannelFilter, ChannelOutputs, ChannelRx, check_input_rate};
pub(crate) use demod::{BIT_RATE, PHASE_DEVIATION, PREAMBLE_S};
use demod::{Demod, RATE, Search};
use message::{Bits, LONG_BITS, PDF1_BITS, SHORT_BITS};

pub(crate) const BIT_SYNC_BITS: usize = 15;
pub(crate) const FRAME_SYNC: u32 = 0b0_0010_1111;
pub(crate) const SELF_TEST_SYNC: u32 = 0b0_1101_0000;
pub(crate) const FRAME_SYNC_BITS: u32 = 9;
const SYNC_LEAD_BITS: u32 = 7;
const SYNC_TOLERANCE: u32 = 3;
const CHANNEL_TAPS: usize = 65;
const TRACK_S: f64 = 0.75;
const HISTORY_CHIPS: usize = 512;
const DOUBTFUL_SHARE: usize = 4;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "sarsat".to_owned(),
    name: "406 MHz beacon".to_owned(),
    summary: "COSPAS-SARSAT EPIRB, ELT and PLB bursts".to_owned(),
    family: DecoderFamily::Utility,
    bandwidth_hz: sdrmm_wire::SARSAT_BANDWIDTH_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    decoder_kind: Some("sarsat".to_owned()),
    ..ChannelDescriptor::default()
});

fn doubtful(soft: &[f32]) -> impl Iterator<Item = bool> + '_ {
    let mut magnitudes = [0.0f32; LONG_BITS];
    for (magnitude, value) in magnitudes.iter_mut().zip(soft) {
        *magnitude = value.abs();
    }
    let used = &mut magnitudes[..soft.len()];
    let (_, &mut threshold, _) =
        used.select_nth_unstable_by(soft.len() / DOUBTFUL_SHARE, f32::total_cmp);
    soft.iter().map(move |value| value.abs() < threshold)
}

fn chip_word(sync: u32) -> u64 {
    let ones = (1u64 << SYNC_LEAD_BITS) - 1;
    let bits = ones << FRAME_SYNC_BITS | u64::from(sync);
    let len = SYNC_LEAD_BITS + FRAME_SYNC_BITS;
    (0..len).rev().fold(0, |word, k| {
        let chip = bits >> k & 1;
        word << 2 | chip << 1 | (chip ^ 1)
    })
}

#[derive(Clone, Copy)]
struct Locked {
    polarity: f32,
    self_test: bool,
}

struct Syncs {
    detectors: [(SyncDetector, Locked); 4],
}

impl Syncs {
    fn new() -> Self {
        let bits = 2 * (SYNC_LEAD_BITS + FRAME_SYNC_BITS);
        let mask = (1u64 << bits) - 1;
        let detector = |word: u64| SyncDetector::new(word & mask, bits, SYNC_TOLERANCE);
        let normal = chip_word(FRAME_SYNC);
        let test = chip_word(SELF_TEST_SYNC);
        let locked = |polarity, self_test| Locked {
            polarity,
            self_test,
        };
        Self {
            detectors: [
                (detector(normal), locked(1.0, false)),
                (detector(!normal), locked(-1.0, false)),
                (detector(test), locked(1.0, true)),
                (detector(!test), locked(-1.0, true)),
            ],
        }
    }

    fn push(&mut self, chip: bool) -> Option<Locked> {
        let mut found = None;
        for (detector, locked) in &mut self.detectors {
            if detector.push(chip) && found.is_none() {
                found = Some(*locked);
            }
        }
        found
    }

    fn reset(&mut self) {
        for (detector, _) in &mut self.detectors {
            detector.reset();
        }
    }
}

enum Frame {
    Hunting,
    Collecting {
        locked: Locked,
        start: u64,
        bits: usize,
    },
}

enum State {
    Searching,
    Tracking { until: u64 },
}

pub struct SarsatChannel {
    search: Search,
    demod: Demod,
    syncs: Syncs,
    state: State,
    frame: Frame,
    chips: Vec<f32>,
    history: [f32; HISTORY_CHIPS],
    chip_clock: u64,
    clock: u64,
    rejected: u32,
}

fn params(settings: &ChannelSettings) -> Result<&SarsatParams, ChannelError> {
    match &settings.params {
        ChannelParams::Sarsat(params) => Ok(params),
        other => Err(ChannelError::InvalidSettings(format!(
            "sarsat channel got {} params",
            other.type_id()
        ))),
    }
}

fn check_params(params: &SarsatParams) -> Result<(), ChannelError> {
    if params.bandwidth_hz.is_finite()
        && (MIN_SARSAT_BANDWIDTH_HZ..=MAX_SARSAT_BANDWIDTH_HZ).contains(&params.bandwidth_hz)
    {
        Ok(())
    } else {
        Err(ChannelError::InvalidSettings(format!(
            "sarsat bandwidth must be in [{MIN_SARSAT_BANDWIDTH_HZ}, {MAX_SARSAT_BANDWIDTH_HZ}] \
             Hz, got {}",
            params.bandwidth_hz
        )))
    }
}

pub(crate) fn occupied_band(params: &SarsatParams) -> (f64, f64) {
    let half = params.bandwidth_hz / 2.0;
    (-half, half)
}

pub(crate) fn channel_filter(params: &SarsatParams) -> Result<ChannelFilter, ChannelError> {
    check_params(params)?;
    let half = params.bandwidth_hz / 2.0;
    Ok(ChannelFilter::Symmetric(Decimator::new(
        &design_lowpass(CHANNEL_TAPS, half / RATE),
        1,
    )))
}

impl SarsatChannel {
    fn chip(&mut self, chip: f32, out: &mut ChannelOutputs) {
        self.history[self.chip_clock as usize % HISTORY_CHIPS] = chip;
        self.chip_clock += 1;
        match self.frame {
            Frame::Hunting => {
                if let Some(locked) = self.syncs.push(chip > 0.0) {
                    self.frame = Frame::Collecting {
                        locked,
                        start: self.chip_clock,
                        bits: SHORT_BITS,
                    };
                }
            }
            Frame::Collecting {
                locked,
                start,
                bits,
            } => {
                if self.chip_clock == start + 2 * bits as u64 {
                    self.collected(locked, start, bits, out);
                }
            }
        }
    }

    fn soft(&self, locked: Locked, start: u64) -> [f32; LONG_BITS] {
        std::array::from_fn(|k| {
            let at = |chip: usize| self.history[(start as usize + chip) % HISTORY_CHIPS];
            (at(2 * k) - at(2 * k + 1)) * locked.polarity
        })
    }

    fn frame(&self, locked: Locked, start: u64, count: usize) -> (Bits, Bits) {
        let soft = self.soft(locked, start);
        let used = &soft[..count];
        let word = Bits::from_sent(used.iter().map(|&value| value > 0.0));
        let (first, second) = used.split_at(PDF1_BITS);
        let weak = Bits::from_sent(doubtful(first).chain(doubtful(second)));
        (word, weak)
    }

    fn collected(&mut self, locked: Locked, start: u64, bits: usize, out: &mut ChannelOutputs) {
        let (word, weak) = self.frame(locked, start, bits);
        if bits == SHORT_BITS
            && message::pdf1(word, weak).is_some_and(|(fixed, _)| message::is_long(fixed))
        {
            self.frame = Frame::Collecting {
                locked,
                start,
                bits: LONG_BITS,
            };
            return;
        }
        match message::decode(word, weak, locked.self_test) {
            Some(decoded) => out.events.push(DecoderEvent::Sarsat(decoded)),
            None => self.rejected = self.rejected.saturating_add(1),
        }
        self.stop();
    }

    fn start(&mut self, carrier_hz: f64) {
        self.demod.start(carrier_hz);
        self.syncs.reset();
        self.frame = Frame::Hunting;
        self.state = State::Tracking {
            until: self.clock + (TRACK_S * RATE) as u64,
        };
    }

    fn stop(&mut self) {
        self.frame = Frame::Hunting;
        self.state = State::Searching;
    }

    fn track(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.search.feed(iq);
        let mut chips = std::mem::take(&mut self.chips);
        self.demod.chips(iq, &mut chips);
        for &chip in &chips {
            self.chip(chip, out);
            if matches!(self.state, State::Searching) {
                break;
            }
        }
        self.chips = chips;
        self.clock += iq.len() as u64;
        if let State::Tracking { until } = self.state
            && self.clock >= until
            && matches!(self.frame, Frame::Hunting)
        {
            self.stop();
        }
    }

    fn hunt(&mut self, iq: &[Complex<f32>]) -> usize {
        for (k, &sample) in iq.iter().enumerate() {
            self.clock += 1;
            if let Some(carrier_hz) = self.search.push(sample) {
                self.start(carrier_hz);
                return k + 1;
            }
        }
        iq.len()
    }

    fn reset(&mut self) {
        self.search.reset();
        self.stop();
    }
}

impl ChannelRx for SarsatChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        check_params(params)?;
        message::prepare();
        Ok(Self {
            search: Search::new(params.bandwidth_hz),
            demod: Demod::new(),
            syncs: Syncs::new(),
            state: State::Searching,
            frame: Frame::Hunting,
            chips: Vec::new(),
            history: [0.0; HISTORY_CHIPS],
            chip_clock: 0,
            clock: 0,
            rejected: 0,
        })
    }

    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        check_params(params)?;
        self.search.set_bandwidth(params.bandwidth_hz);
        Ok(())
    }

    fn retuned(&mut self) {
        self.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let mut rest = iq;
        while !rest.is_empty() {
            match self.state {
                State::Searching => {
                    let used = self.hunt(rest);
                    rest = &rest[used..];
                }
                State::Tracking { .. } => {
                    self.track(rest, out);
                    rest = &[];
                }
            }
        }
    }

    fn rejected(&self) -> u32 {
        self.rejected
    }
}

#[cfg(test)]
mod tests;
