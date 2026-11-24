use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::registry::Native;
use sdrmm_decoder::{
    AUDIO_RATE, Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_dsp::coding::fec::{Acceptance, Bch, BchScratch, chase};
use sdrmm_dsp::{FmDemod, SyncDetector, hamming_distance};
use sdrmm_modem::analog::voice::{FmVoice, Quieting};
use sdrmm_modem::cpm::{CpmDemod, CpmParams, Mapping, TIMING_BW_BURST};
use sdrmm_modem::pulse::{self, Norm};
use sdrmm_wire::{
    CallGate, ChannelDescriptor, ChannelSettings, Control, DecoderEvent, DecoderFamily,
    DvFrameKind, EventKind, FilterSpec, FrameSync, HzRange, KindSpec, SettingField,
    SignalSignature, SyncPulse,
};

mod message;
mod model;
#[cfg(any(test, feature = "synth"))]
pub mod synth;
mod trunk;

pub use model::*;

const INPUT_RATE_HZ: f64 = 48_000.0;
const BANDWIDTH_HZ: f64 = 25_000.0;
const CHANNEL_TAPS: usize = 129;

/// Dotting, the sync proper, dotting again.
pub(crate) const SYNC: u64 = 0x5555_5712_5555;
pub(crate) const SYNC_BITS: u32 = 48;
const SYNC_TOLERANCE: u32 = 4;
/// A working channel's data opens and closes with dotting, which the control channel never
/// sends this long.
const DOTTING: u64 = 0xAAAA_AAAA;
const DOTTING_BITS: u32 = 32;
const DOTTING_TOLERANCE: u32 = 2;

pub(crate) const MESSAGE_BITS: usize = 40;
pub(crate) const DATA_BITS: usize = 28;
pub(crate) const COPIES: usize = 3;
/// Two messages, each sent three times, the middle copy inverted.
pub(crate) const FRAME_BITS: usize = 2 * COPIES * MESSAGE_BITS;

/// BCH(63,51) over GF(2^6) from x^6 + x + 1, shortened to the 40-bit message.
const BCH_FIELD_BITS: u32 = 6;
const BCH_PRIMITIVE: u32 = 0x43;
const BCH_CORRECTS: usize = 2;
const CHASE_BITS: usize = 4;

/// How long after the site's data a working channel waits before it plays what follows.
const DATA_HOLD_S: f64 = 0.25;
/// How long a working channel's carrier may drop before its call counts as over.
const CARRIER_HANG_S: f64 = 0.5;
/// How long a rate the channel locked onto may go without a frame before both are heard again.
const RELOCK_S: f64 = 5.0;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "edacs".to_owned(),
    name: "EDACS".to_owned(),
    summary: "EDACS trunking control channel and voice".to_owned(),
    family: DecoderFamily::DIGITAL_VOICE,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: INPUT_RATE_HZ,
    has_audio: true,
    emits: vec![EventKind::DV],
    calls: Some(CallGate::Frames),
    ..ChannelDescriptor::default()
});

/// How a baud's sites key their data and voice.
#[derive(Clone, Copy)]
#[expect(clippy::struct_field_names, reason = "the unit belongs in each name")]
pub(crate) struct Air {
    pub(crate) data_deviation_hz: f64,
    pub(crate) voice_deviation_hz: f64,
    half_width_hz: f64,
}

pub(crate) const fn air(baud: EdacsBaud) -> Air {
    match baud {
        EdacsBaud::Auto | EdacsBaud::B9600 => Air {
            data_deviation_hz: 3_000.0,
            voice_deviation_hz: 4_000.0,
            half_width_hz: 10_000.0,
        },
        EdacsBaud::B4800 => Air {
            data_deviation_hz: 1_800.0,
            voice_deviation_hz: 2_500.0,
            half_width_hz: 6_250.0,
        },
    }
}

fn params(settings: &ChannelSettings) -> Result<EdacsParams, ChannelError> {
    sdrmm_decoder::settings_of::<EdacsParams>(settings)
}

fn occupied_band(baud: EdacsBaud) -> (f64, f64) {
    let half = air(baud).half_width_hz;
    (-half, half)
}

fn channel_filter(baud: EdacsBaud) -> FilterSpec {
    FilterSpec::lowpass(CHANNEL_TAPS, air(baud).half_width_hz)
}

fn data_mapping() -> Mapping {
    Mapping::new(vec![1.0, -1.0])
}

fn data_demod(baud: EdacsBaud) -> CpmDemod {
    let sps = INPUT_RATE_HZ / baud.hz();
    let params = CpmParams::from_deviation(
        data_mapping(),
        air(baud).data_deviation_hz,
        baud.hz(),
        pulse::rect(sps, Norm::Area),
        sps,
    );
    CpmDemod::new(&params, &pulse::rect(sps, Norm::Area), TIMING_BW_BURST)
}

pub struct EdacsChannel {
    baud: EdacsBaud,
    addressing: EdacsAddressing,
    receivers: Vec<Receiver>,
    locked: Option<usize>,
    symbols: Vec<f32>,
    mapping: Mapping,
    working: WorkingChannel,
}

/// Listens at one rate.
struct Receiver {
    baud: f64,
    demod: CpmDemod,
    framer: Framer,
    /// Samples since this rate last read a frame.
    since_frame: usize,
}

impl Receiver {
    fn new(baud: EdacsBaud) -> Self {
        Self {
            baud: baud.hz(),
            demod: data_demod(baud),
            framer: Framer::new(),
            since_frame: 0,
        }
    }

    fn reset(&mut self) {
        self.demod.reset();
        self.framer.reset();
    }
}

fn receivers(baud: EdacsBaud) -> Vec<Receiver> {
    baud.rates()
        .iter()
        .map(|&rate| Receiver::new(rate))
        .collect()
}

impl NativeChannel for EdacsChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        let params = params(&settings)?;
        Ok(Self {
            baud: params.baud,
            addressing: params.addressing,
            receivers: receivers(params.baud),
            locked: None,
            symbols: Vec::new(),
            mapping: data_mapping(),
            working: WorkingChannel::new(params.baud),
        })
    }
}

impl Channel for EdacsChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        let params = params(&settings)?;
        if params.baud != self.baud {
            self.baud = params.baud;
            self.receivers = receivers(params.baud);
            self.locked = None;
            self.working = WorkingChannel::new(params.baud);
        }
        self.addressing = params.addressing;
        Ok(())
    }

    fn retuned(&mut self) {
        for receiver in &mut self.receivers {
            receiver.reset();
        }
        self.locked = None;
        self.working.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        let mut data = false;
        for index in 0..self.receivers.len() {
            if self.locked.is_some_and(|locked| locked != index) {
                continue;
            }
            let receiver = &mut self.receivers[index];
            self.symbols.clear();
            receiver.demod.process(iq, &mut self.symbols);
            if index == self.locked.unwrap_or(0) {
                out.symbols.cpm(
                    &receiver.demod,
                    &self.symbols,
                    &self.mapping,
                    receiver.baud,
                    INPUT_RATE_HZ,
                );
            }
            let before = out.events.len();
            for &symbol in &self.symbols {
                data |= receiver.framer.push(symbol, self.addressing, out);
            }
            receiver.since_frame = if out.events.len() > before {
                0
            } else {
                receiver.since_frame.saturating_add(iq.len())
            };
            if out.events.len() > before && self.receivers.len() > 1 {
                self.locked = Some(index);
            }
        }
        if self
            .locked
            .is_some_and(|locked| self.receivers[locked].since_frame >= samples(RELOCK_S))
        {
            self.locked = None;
        }
        self.working.process(iq, data, out);
    }

    fn rejected(&self) -> u32 {
        self.receivers
            .iter()
            .map(|receiver| receiver.framer.rejected)
            .fold(0, u32::saturating_add)
    }
}

/// One message as summed from its three copies and corrected.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Word {
    pub(crate) data: u32,
    pub(crate) errors: u32,
}

/// Finds frames in the bit stream and reads their messages.
struct Framer {
    sync: SyncDetector,
    inverse: SyncDetector,
    dotting: SyncDetector,
    /// The frame's soft bits after the sync, positive for a one.
    frame: [f32; FRAME_BITS],
    filled: usize,
    collecting: Option<bool>,
    bch: Bch,
    scratch: BchScratch,
    rejected: u32,
}

impl Framer {
    fn new() -> Self {
        let bch = Bch::new(BCH_FIELD_BITS, BCH_PRIMITIVE, BCH_CORRECTS);
        Self {
            sync: SyncDetector::new(SYNC, SYNC_BITS, SYNC_TOLERANCE),
            inverse: SyncDetector::new(!SYNC, SYNC_BITS, SYNC_TOLERANCE),
            dotting: SyncDetector::new(DOTTING, DOTTING_BITS, DOTTING_BITS),
            frame: [0.0; FRAME_BITS],
            filled: 0,
            collecting: None,
            scratch: bch.scratch(),
            bch,
            rejected: 0,
        }
    }

    fn reset(&mut self) {
        self.sync.reset();
        self.inverse.reset();
        self.dotting.reset();
        self.collecting = None;
        self.filled = 0;
    }

    /// Takes one soft bit, positive for a one; reports whether it ended a sync word or a run of
    /// dotting.
    fn push(&mut self, soft: f32, addressing: EdacsAddressing, out: &mut ChannelOutputs) -> bool {
        let bit = soft > 0.0;
        let synced = self.sync.push(bit);
        let inverse = self.inverse.push(bit);
        self.dotting.push(bit);
        if let Some(inverted) = self.collecting {
            self.frame[self.filled] = if inverted { -soft } else { soft };
            self.filled += 1;
            if self.filled == FRAME_BITS {
                self.collecting = None;
                self.emit(addressing, out);
            }
            return false;
        }
        if synced || inverse {
            self.collecting = Some(inverse && !synced);
            self.filled = 0;
            return true;
        }
        self.dotting_heard()
    }

    fn dotting_heard(&self) -> bool {
        let distance = hamming_distance(self.dotting.register(), DOTTING);
        distance <= DOTTING_TOLERANCE || distance >= DOTTING_BITS - DOTTING_TOLERANCE
    }

    fn emit(&mut self, addressing: EdacsAddressing, out: &mut ChannelOutputs) {
        let [first, second] = [0, 1].map(|half| self.word(half));
        let Some(first) = first else {
            return;
        };
        let Some(read) = message::read(
            first.data,
            second.map(|word| word.data),
            addressing.extended(),
            addressing.esk(),
        ) else {
            return;
        };
        let mut frame = read.first.frame();
        frame.errors_corrected = first.errors;
        out.events.push(DecoderEvent::of(&frame));
        if let (Some(message), Some(word)) = (read.second, second) {
            let mut frame = message.frame();
            frame.errors_corrected = word.errors;
            out.events.push(DecoderEvent::of(&frame));
        }
    }

    /// The message in half `half` of the frame, or `None`, counted, when it cannot be read.
    fn word(&mut self, half: usize) -> Option<Word> {
        let copies = &self.frame[half * COPIES * MESSAGE_BITS..(half + 1) * COPIES * MESSAGE_BITS];
        let (summed, disagreements) = vote(copies);
        let (word, reliability) = word_of(&summed);
        let (bch, scratch) = (&self.bch, &mut self.scratch);
        let repair = |trial: u64| {
            let mut bits: [bool; MESSAGE_BITS] =
                std::array::from_fn(|k| trial >> (MESSAGE_BITS - 1 - k) & 1 == 1);
            bch.decode_with_scratch(&mut bits, scratch)?;
            Some(
                bits.iter()
                    .fold(0, |value, &bit| value << 1 | u64::from(bit)),
            )
        };
        let acceptance = Acceptance::Radius(BCH_CORRECTS as u32);
        let Some(fixed) = chase(word, &reliability, CHASE_BITS, acceptance, repair) else {
            self.rejected = self.rejected.saturating_add(1);
            return None;
        };
        Some(Word {
            data: (fixed >> (MESSAGE_BITS - DATA_BITS)) as u32,
            errors: disagreements + (fixed ^ word).count_ones(),
        })
    }
}

/// The sum of a message's three soft copies, the middle one sent inverted, and how many bits
/// the copies did not all agree on.
pub(crate) fn vote(copies: &[f32]) -> ([f32; MESSAGE_BITS], u32) {
    let mut summed = [0.0; MESSAGE_BITS];
    let mut disagreements = 0;
    for (index, sum) in summed.iter_mut().enumerate() {
        let a = copies[index];
        let b = -copies[MESSAGE_BITS + index];
        let c = copies[2 * MESSAGE_BITS + index];
        *sum = a + b + c;
        disagreements += u32::from(!((a > 0.0) == (b > 0.0) && (b > 0.0) == (c > 0.0)));
    }
    (summed, disagreements)
}

/// The hard word of a soft message, its first bit the most significant, and each bit's
/// reliability by its place in the word.
fn word_of(soft: &[f32; MESSAGE_BITS]) -> (u64, [f32; MESSAGE_BITS]) {
    let word = soft
        .iter()
        .fold(0, |word, &value| word << 1 | u64::from(value > 0.0));
    (
        word,
        std::array::from_fn(|bit| soft[MESSAGE_BITS - 1 - bit].abs()),
    )
}

/// Plays a working channel's voice: whatever a keyed carrier brings once the site's data has
/// stopped, and the call's start and end as frames the trunk follower's grant names.
struct WorkingChannel {
    discriminator: FmDemod,
    detected: Vec<f32>,
    quieting: Quieting,
    voice: FmVoice,
    since_data: usize,
    since_carrier: usize,
    speaking: bool,
}

impl WorkingChannel {
    fn new(baud: EdacsBaud) -> Self {
        Self {
            discriminator: FmDemod::new(INPUT_RATE_HZ, air(baud).voice_deviation_hz),
            detected: Vec::new(),
            quieting: Quieting::new(INPUT_RATE_HZ),
            voice: FmVoice::new(INPUT_RATE_HZ),
            since_data: 0,
            since_carrier: usize::MAX,
            speaking: false,
        }
    }

    fn reset(&mut self) {
        self.quieting.reset();
        self.voice.reset();
        self.since_data = 0;
        self.since_carrier = usize::MAX;
        self.speaking = false;
    }

    fn process(&mut self, iq: &[Complex<f32>], data: bool, out: &mut ChannelOutputs) {
        self.discriminator.process(iq, &mut self.detected);
        let carrier = self.quieting.process(&self.detected);
        self.since_data = if data {
            0
        } else {
            self.since_data.saturating_add(iq.len())
        };
        let open = carrier && self.since_data >= samples(DATA_HOLD_S);
        self.voice.process(&self.detected, &mut out.audio_pcm);
        if !open {
            out.audio_pcm.fill(0.0);
        }
        if !out.audio_pcm.is_empty() {
            out.audio_rate = AUDIO_RATE;
        }
        self.frame_the_call(open, data, iq.len(), out);
    }

    fn frame_the_call(&mut self, open: bool, data: bool, len: usize, out: &mut ChannelOutputs) {
        self.since_carrier = if open {
            0
        } else {
            self.since_carrier.saturating_add(len)
        };
        if open && !self.speaking {
            self.speaking = true;
            out.events
                .push(DecoderEvent::of(&trunk::working_frame(DvFrameKind::Voice)));
        } else if self.speaking && (data || self.since_carrier >= samples(CARRIER_HANG_S)) {
            self.speaking = false;
            out.events.push(DecoderEvent::of(&trunk::working_frame(
                DvFrameKind::Terminator,
            )));
        }
    }
}

fn samples(seconds: f64) -> usize {
    (seconds * INPUT_RATE_HZ) as usize
}

const EDACS_BANDS: &[HzRange] = &[
    HzRange::mhz(136.0, 174.0),
    HzRange::mhz(403.0, 512.0),
    HzRange::mhz(806.0, 870.0),
];

fn edacs_signatures() -> Vec<SignalSignature> {
    [EdacsBaud::B9600, EdacsBaud::B4800]
        .into_iter()
        .map(|baud| {
            SignalSignature::synced(
                &format!("EDACS ({} bd)", baud.hz()),
                2.0 * air(baud).half_width_hz,
                FrameSync {
                    symbol_rate_hz: baud.hz(),
                    deviation_hz: air(baud).data_deviation_hz,
                    pulse: SyncPulse::Gaussian {
                        bt: 0.5,
                        pulse_span: 3,
                        matched_span: 3,
                    },
                    bits: SYNC_BITS,
                    tolerance: 2,
                    patterns: vec![SYNC, !SYNC & ((1 << SYNC_BITS) - 1)],
                    min_hits: 2,
                },
            )
            .on(EDACS_BANDS)
        })
        .collect()
}

pub(crate) fn edacs_decoder() -> Native<EdacsParams> {
    Native::new::<EdacsChannel>(|params: &EdacsParams| occupied_band(params.baud))
        .filter(|params| Ok(channel_filter(params.baud)))
        .signatures(edacs_signatures)
        .form(edacs_form)
        .kinds(edacs_kinds)
        .trunking(trunk::trunking())
}

pub(crate) fn edacs_form() -> Vec<SettingField> {
    vec![
        SettingField::new("baud", "Baud", Control::choice::<EdacsBaud>())
            .title("Control channel rate"),
        SettingField::new(
            "addressing",
            "Addressing",
            Control::choice::<EdacsAddressing>(),
        )
        .title("How the site numbers radios and groups"),
    ]
}

fn edacs_kinds() -> Vec<KindSpec> {
    Vec::new()
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
