//! Motorola MDC-1200: 1200 baud FFSK bursts at the start or end of an FM transmission that
//! name the radio and carry emergency, status and call signalling.

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::registry::Native;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_dsp::SyncDetector;
use sdrmm_modem::ffsk::{FfskDemod, FfskTones};
use sdrmm_wire::{
    ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventContract, FilterSpec,
    HzRange, KindSpec, LAND_MOBILE, Modulation, SettingField, SignalSignature,
};

mod frame;
mod model;
pub use model::*;

use frame::{BLOCK_BITS, SYNC, SYNC_BITS};

const RATE: f64 = 48_000.0;
const BANDWIDTH_HZ: f64 = 12_500.0;
const DEVIATION_HZ: f64 = 3_000.0;
const CHANNEL_TAPS: usize = 65;
const SYNC_TOLERANCE: u32 = 4;

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "mdc1200".to_owned(),
    name: "MDC-1200".to_owned(),
    summary: "Motorola unit IDs and status on FM".to_owned(),
    family: DecoderFamily::PAGING,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![MdcPacket::KIND],
    ..ChannelDescriptor::default()
});

/// Where the bit stream stands: hunting the sync word, or filling a block. `second` holds the
/// first block of a double packet while its second block arrives.
#[derive(Clone, Copy)]
enum State {
    Hunt,
    Block {
        inverted: bool,
        filled: usize,
        second: Option<frame::Block>,
    },
}

/// Turns sliced bits into packets. MDC keys a space tone to flip the data bit and a mark to
/// keep it, so the data is the running parity of the spaces.
struct Framer {
    level: bool,
    sync: SyncDetector,
    state: State,
    bits: [bool; BLOCK_BITS],
    rejected: u32,
}

impl Framer {
    fn new() -> Self {
        Self {
            level: false,
            sync: SyncDetector::new(SYNC, SYNC_BITS, SYNC_TOLERANCE),
            state: State::Hunt,
            bits: [false; BLOCK_BITS],
            rejected: 0,
        }
    }

    fn reset(&mut self) {
        self.sync.reset();
        self.state = State::Hunt;
    }

    fn push(&mut self, mark: bool, out: &mut ChannelOutputs) {
        self.level ^= !mark;
        let level = self.level;
        match &mut self.state {
            State::Hunt => {
                self.sync.push(level);
                if let Some(inverted) = self.synced() {
                    self.state = State::Block {
                        inverted,
                        filled: 0,
                        second: None,
                    };
                }
            }
            State::Block {
                inverted,
                filled,
                second,
            } => {
                self.bits[*filled] = level ^ *inverted;
                *filled += 1;
                if *filled == BLOCK_BITS {
                    let (inverted, first) = (*inverted, *second);
                    self.finish(inverted, first, out);
                }
            }
        }
    }

    /// Whether the sync word just arrived, and if so whether upright or inverted: the running
    /// parity starts from an unknown level.
    fn synced(&self) -> Option<bool> {
        let distance = sdrmm_dsp::hamming_distance(self.sync.register(), SYNC);
        if distance <= SYNC_TOLERANCE {
            Some(false)
        } else if distance >= SYNC_BITS - SYNC_TOLERANCE {
            Some(true)
        } else {
            None
        }
    }

    fn finish(&mut self, inverted: bool, first: Option<frame::Block>, out: &mut ChannelOutputs) {
        self.reset();
        let block = frame::decode(&self.bits);
        match (first, block) {
            (None, Some(block)) if frame::is_double(block.op) => {
                self.state = State::Block {
                    inverted,
                    filled: 0,
                    second: Some(block),
                };
            }
            (None, Some(block)) => emit(out, packet(block, None)),
            (Some(first), second) => {
                emit(out, packet(first, second.map(extra)));
                if second.is_none() {
                    self.rejected = self.rejected.saturating_add(1);
                }
            }
            (None, None) => self.rejected = self.rejected.saturating_add(1),
        }
    }
}

fn extra(block: frame::Block) -> ([u8; 4], u32) {
    let [high, low] = block.unit.to_be_bytes();
    ([block.op, block.arg, high, low], block.repaired)
}

fn packet(block: frame::Block, second: Option<([u8; 4], u32)>) -> MdcPacket {
    let double = frame::is_double(block.op);
    MdcPacket {
        unit: block.unit,
        op: block.op,
        arg: block.arg,
        extra: second.map(|(extra, _)| extra),
        truncated: double && second.is_none(),
        repaired: block.repaired + second.map_or(0, |(_, repaired)| repaired),
    }
}

fn emit(out: &mut ChannelOutputs, packet: MdcPacket) {
    out.events.push(DecoderEvent::of(&packet));
}

pub struct MdcChannel {
    demod: FfskDemod,
    symbols: Vec<f32>,
    framer: Framer,
}

impl MdcChannel {
    fn slice(&mut self, out: &mut ChannelOutputs) {
        for &symbol in &self.symbols {
            self.framer.push(symbol > 0.0, out);
        }
    }

    #[cfg(test)]
    fn audio(&mut self, audio: &[f32], out: &mut ChannelOutputs) {
        self.demod.audio_symbols(audio, &mut self.symbols);
        self.slice(out);
    }
}

impl NativeChannel for MdcChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        sdrmm_decoder::settings_of::<MdcParams>(&settings)?;
        Ok(Self {
            demod: FfskDemod::new(RATE, DEVIATION_HZ, FfskTones::CCIR_1200),
            symbols: Vec::new(),
            framer: Framer::new(),
        })
    }
}

impl Channel for MdcChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        sdrmm_decoder::settings_of::<MdcParams>(&settings).map(|_| ())
    }

    fn retuned(&mut self) {
        self.demod.reset();
        self.framer.reset();
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        self.demod.symbols(iq, &mut self.symbols);
        self.slice(out);
    }

    fn rejected(&self) -> u32 {
        self.framer.rejected
    }
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-BANDWIDTH_HZ / 2.0, BANDWIDTH_HZ / 2.0)
}

pub(crate) fn channel_filter() -> FilterSpec {
    FilterSpec::lowpass(CHANNEL_TAPS, BANDWIDTH_HZ / 2.0)
}

fn mdc_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "MDC-1200 (FFSK over FM)",
            &[Modulation::Fm],
            HzRange::new(6_000.0, 25_000.0),
            "a short 1200 baud tone burst at the start or end of an FM call",
        )
        .symbol_rate(HzRange::about(1_200.0, 0.06))
        .deviation(HzRange::new(1_500.0, 3_800.0))
        .on(LAND_MOBILE),
    ]
}

fn mdc_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(MdcPacket::KIND)
}

pub(crate) fn mdc1200_decoder() -> Native<MdcParams> {
    Native::new::<MdcChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .signatures(mdc_signatures)
        .trial(mdc_trial)
        .form(mdc_form)
        .kinds(mdc_kinds)
}

pub(crate) fn mdc_form() -> Vec<SettingField> {
    Vec::new()
}

fn mdc_kinds() -> Vec<KindSpec> {
    vec![mdc_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
