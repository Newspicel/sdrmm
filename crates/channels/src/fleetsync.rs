//! Kenwood `FleetSync I` and II: FFSK bursts in an FM channel that name the calling fleet and
//! unit and carry status and short messages. Radios send them at 1200 or 2400 baud, so two
//! lanes listen at once.

use std::sync::LazyLock;

use num_complex::Complex;
use sdrmm_decoder::registry::Native;
use sdrmm_decoder::{
    Channel, ChannelCtx, ChannelError, ChannelOutputs, NativeChannel, check_input_rate,
};
use sdrmm_dsp::{SyncDetector, SyndromeDecoder};
use sdrmm_modem::cpm::Mapping;
use sdrmm_modem::ffsk::{FfskDemod, FfskTones};
use sdrmm_wire::{
    ChannelDescriptor, ChannelSettings, DecoderEvent, DecoderFamily, EventContract, FilterSpec,
    HzRange, KindSpec, LAND_MOBILE, Modulation, SettingField, SignalSignature,
};

mod frame;
mod model;
pub use model::*;

use frame::{Assembly, BLOCK_BITS, SYNC_BITS, SYNC_WORDS, Step};

const RATE: f64 = 48_000.0;
const BANDWIDTH_HZ: f64 = 12_500.0;
const DEVIATION_HZ: f64 = 3_000.0;
const CHANNEL_TAPS: usize = 65;
const SYNC_TOLERANCE: u32 = 3;
const LANES: [FfskTones; 2] = [FfskTones::CCIR_1200, FfskTones::MSK_2400];

static DESCRIPTOR: LazyLock<ChannelDescriptor> = LazyLock::new(|| ChannelDescriptor {
    type_id: "fleetsync".to_owned(),
    name: "FleetSync".to_owned(),
    summary: "Kenwood unit IDs, status and messages on FM".to_owned(),
    family: DecoderFamily::PAGING,
    verification: sdrmm_wire::Verification::OnAir,
    bandwidth_hz: BANDWIDTH_HZ,
    input_rate_hz: RATE,
    has_audio: false,
    emits: vec![FleetSyncMessage::KIND],
    ..ChannelDescriptor::default()
});

/// Sliced bits of one baud rate in, messages out: a sync word, then 64 bit blocks until one
/// fails.
struct Framer {
    baud: u32,
    syncs: [SyncDetector; 2],
    bch: SyndromeDecoder,
    gathering: bool,
    filled: usize,
    block: u64,
    assembly: Assembly,
    rejected: u32,
}

impl Framer {
    fn new(baud: u32) -> Self {
        Self {
            baud,
            syncs: SYNC_WORDS.map(|word| SyncDetector::new(word, SYNC_BITS, SYNC_TOLERANCE)),
            bch: frame::bch(),
            gathering: false,
            filled: 0,
            block: 0,
            assembly: Assembly::new(),
            rejected: 0,
        }
    }

    fn reset(&mut self) {
        for sync in &mut self.syncs {
            sync.reset();
        }
        self.gathering = false;
    }

    fn push(&mut self, bit: bool, out: &mut ChannelOutputs) {
        if !self.gathering {
            let synced = self
                .syncs
                .iter_mut()
                .fold(false, |any, sync| sync.push(bit) | any);
            if synced {
                self.gathering = true;
                self.filled = 0;
                self.assembly.clear();
            }
            return;
        }
        self.block = self.block << 1 | u64::from(bit);
        self.filled += 1;
        if self.filled < BLOCK_BITS {
            return;
        }
        self.filled = 0;
        let (first, second) = ((self.block >> 32) as u32, self.block as u32);
        if self.assembly.push(&self.bch, first, second) == Step::Done {
            self.finish(out);
        }
    }

    fn finish(&mut self, out: &mut ChannelOutputs) {
        self.reset();
        match self.assembly.message(self.baud) {
            Some(message) => out.events.push(DecoderEvent::of(&message)),
            None => self.rejected = self.rejected.saturating_add(1),
        }
    }
}

struct Lane {
    baud: f64,
    demod: FfskDemod,
    framer: Framer,
}

pub struct FleetSyncChannel {
    lanes: [Lane; 2],
    symbols: Vec<f32>,
    rail: Mapping,
    /// The lane the scope shows: the last one to find a sync word.
    watched: usize,
}

impl FleetSyncChannel {
    fn lanes() -> [Lane; 2] {
        LANES.map(|tones| Lane {
            baud: tones.baud,
            demod: FfskDemod::new(RATE, DEVIATION_HZ, tones),
            framer: Framer::new(tones.baud as u32),
        })
    }

    fn slice(lane: &mut Lane, symbols: &[f32], out: &mut ChannelOutputs) {
        for &symbol in symbols {
            lane.framer.push(symbol > 0.0, out);
        }
    }
}

impl NativeChannel for FleetSyncChannel {
    fn descriptor() -> &'static ChannelDescriptor {
        &DESCRIPTOR
    }

    fn new(ctx: ChannelCtx, settings: ChannelSettings) -> Result<Self, ChannelError> {
        check_input_rate(ctx, &DESCRIPTOR)?;
        sdrmm_decoder::settings_of::<FleetSyncParams>(&settings)?;
        Ok(Self {
            lanes: Self::lanes(),
            symbols: Vec::new(),
            rail: Mapping::natural(2),
            watched: 0,
        })
    }
}

impl Channel for FleetSyncChannel {
    fn apply(&mut self, settings: ChannelSettings) -> Result<(), ChannelError> {
        sdrmm_decoder::settings_of::<FleetSyncParams>(&settings).map(|_| ())
    }

    fn retuned(&mut self) {
        for lane in &mut self.lanes {
            lane.demod.reset();
            lane.framer.reset();
        }
    }

    fn process(&mut self, iq: &[Complex<f32>], out: &mut ChannelOutputs) {
        for (index, lane) in self.lanes.iter_mut().enumerate() {
            lane.demod.symbols(iq, &mut self.symbols);
            if index == self.watched {
                out.symbols
                    .levels(&self.symbols, &[], &self.rail, lane.baud, 0.0);
            }
            Self::slice(lane, &self.symbols, out);
        }
        if let Some(gathering) = self.lanes.iter().position(|lane| lane.framer.gathering) {
            self.watched = gathering;
        }
    }

    fn rejected(&self) -> u32 {
        self.lanes
            .iter()
            .fold(0u32, |sum, lane| sum.saturating_add(lane.framer.rejected))
    }
}

pub(crate) fn occupied_band() -> (f64, f64) {
    (-BANDWIDTH_HZ / 2.0, BANDWIDTH_HZ / 2.0)
}

pub(crate) fn channel_filter() -> FilterSpec {
    FilterSpec::lowpass(CHANNEL_TAPS, BANDWIDTH_HZ / 2.0)
}

fn fleetsync_signatures() -> Vec<SignalSignature> {
    vec![
        SignalSignature::new(
            "FleetSync (FFSK over FM)",
            &[Modulation::Fm],
            HzRange::new(6_000.0, 25_000.0),
            "a short 1200 or 2400 baud tone burst at the start or end of an FM call",
        )
        .symbol_rate(HzRange::new(1_100.0, 2_600.0))
        .deviation(HzRange::new(1_500.0, 3_800.0))
        .on(LAND_MOBILE),
    ]
}

fn fleetsync_trial() -> sdrmm_wire::SignalTrial {
    sdrmm_wire::SignalTrial::proven_by(FleetSyncMessage::KIND)
}

pub(crate) fn fleetsync_decoder() -> Native<FleetSyncParams> {
    Native::new::<FleetSyncChannel>(|_| occupied_band())
        .filter(|_| Ok(channel_filter()))
        .signatures(fleetsync_signatures)
        .trial(fleetsync_trial)
        .form(fleetsync_form)
        .kinds(fleetsync_kinds)
}

pub(crate) fn fleetsync_form() -> Vec<SettingField> {
    Vec::new()
}

fn fleetsync_kinds() -> Vec<KindSpec> {
    vec![fleetsync_kind()]
}

#[cfg(test)]
pub(crate) mod fixture;
#[cfg(test)]
mod tests;
