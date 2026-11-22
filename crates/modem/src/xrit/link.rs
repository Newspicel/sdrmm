use num_complex::Complex;

use super::coarse::CoarseTune;
use super::session::{SessionAssembler, SessionStats, TransportFile};
use crate::ccsds::{
    Basis, Deframer, Depacketizer, FILL_VCID, FrameOutcome, MpduLayout, Precoding, VCID_COUNT,
    VcduHeader,
};
use crate::constellation::{Constellation, tables};
use crate::linear::{CarrierLoop, LinearDemod, LinearParams, LinearTiming, PhaseDetector};
use crate::pulse::{Norm, root_raised_cosine};

const PULSE_SPAN: usize = 8;
/// Narrow enough to hold a carrier at a few dB Es/N0; [`CoarseTune`] brings it within reach.
const CARRIER_BW: f64 = 0.004;
/// Symbols without a good frame before the carrier is searched for again.
const LOST_SYMBOLS: usize = 4 * crate::ccsds::CODED_BITS;
const SOFT_GAIN: f32 = 48.0;
const SOFT_LIMIT: f32 = 127.0;
/// The largest space packet an xRIT link carries: an 8 KiB transport data unit and its header.
const PACKET_CAPACITY: usize = 8_200;
const LAYOUT: MpduLayout = MpduLayout { insert_zone: 0 };

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkConfig {
    pub sps: usize,
    pub roll_off: f64,
    pub basis: Basis,
    pub precoding: Precoding,
    /// Files assembled at once, across all virtual channels and APIDs.
    pub sessions: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkStats {
    pub frames: u32,
    pub frames_corrected: u32,
    pub frames_failed: u32,
    pub packets_dropped: u32,
    pub session: SessionStats,
}

impl LinkStats {
    /// Everything lost on the way: frames RS could not mend, packets and files given up, and
    /// image lines filled black.
    #[must_use]
    pub fn lost(&self) -> u32 {
        self.frames_failed
            .saturating_add(self.packets_dropped)
            .saturating_add(self.session.packets_failed)
            .saturating_add(self.session.files_lost)
            .saturating_add(self.session.lines_failed)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    Waveform(String),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Waveform(error) => write!(f, "BPSK waveform: {error}"),
        }
    }
}

impl std::error::Error for LinkError {}

/// The receive side of an LRIT or HRIT downlink: BPSK to soft symbols, CADUs, the space packets
/// of every virtual channel, and the files they carry.
pub struct XritLink {
    sps: usize,
    coarse: CoarseTune,
    since_frame: usize,
    demod: LinearDemod,
    symbols: Vec<Complex<f32>>,
    soft: Vec<i16>,
    deframer: Deframer,
    channels: Vec<Depacketizer>,
    sessions: SessionAssembler,
    frames: u32,
    frames_corrected: u32,
    frames_failed: u32,
}

impl XritLink {
    pub fn new(config: LinkConfig) -> Result<Self, LinkError> {
        let pulse =
            root_raised_cosine(config.sps as f64, config.roll_off, PULSE_SPAN, Norm::Energy);
        let params = LinearParams::new(tables::bpsk(), pulse.clone(), config.sps)
            .map_err(|error| LinkError::Waveform(error.to_string()))?;
        let carrier = CarrierLoop::new(PhaseDetector::MthPower { m: 2 }, CARRIER_BW);
        Ok(Self {
            sps: config.sps,
            coarse: CoarseTune::new(),
            since_frame: 0,
            demod: LinearDemod::new(&params, &pulse, LinearTiming::CONTINUOUS, Some(carrier)),
            symbols: Vec::new(),
            soft: Vec::new(),
            deframer: Deframer::new(config.basis).with_precoding(config.precoding),
            channels: (0..VCID_COUNT)
                .map(|_| Depacketizer::new(LAYOUT, PACKET_CAPACITY))
                .collect(),
            sessions: SessionAssembler::new(config.sessions),
            frames: 0,
            frames_corrected: 0,
            frames_failed: 0,
        })
    }

    #[must_use]
    pub fn stats(&self) -> LinkStats {
        LinkStats {
            frames: self.frames,
            frames_corrected: self.frames_corrected,
            frames_failed: self.frames_failed,
            packets_dropped: self.channel_drops(),
            session: self.sessions.stats(),
        }
    }

    /// The decision points of the last block, after timing and carrier recovery.
    #[must_use]
    pub fn symbols(&self) -> &[Complex<f32>] {
        &self.symbols
    }

    #[must_use]
    pub fn constellation(&self) -> &Constellation {
        self.demod.params().constellation()
    }

    /// The carrier offset taken out, coarse and fine together.
    #[must_use]
    pub fn frequency_offset_cycles_per_sample(&self) -> f64 {
        self.coarse.turns() + self.demod.carrier_freq_cycles_per_symbol() / self.sps as f64
    }

    fn channel_drops(&self) -> u32 {
        self.channels
            .iter()
            .fold(0u32, |sum, channel| sum.saturating_add(channel.dropped()))
    }

    /// Starts over after a retune; whatever was part-built counts as lost.
    pub fn reset(&mut self) {
        self.coarse.reset();
        self.since_frame = 0;
        self.demod.reset();
        self.deframer.reset();
        for channel in &mut self.channels {
            channel.reset();
        }
        self.sessions.reset();
    }

    pub fn process(&mut self, iq: &[Complex<f32>], on_file: &mut impl FnMut(TransportFile<'_>)) {
        self.symbols.clear();
        let tuned = self.coarse.process(iq);
        self.demod.process(tuned, &mut self.symbols);
        self.since_frame += self.symbols.len();
        self.soft.clear();
        self.soft.extend(self.symbols.iter().map(|symbol| {
            (symbol.re * SOFT_GAIN)
                .round()
                .clamp(-SOFT_LIMIT, SOFT_LIMIT) as i16
        }));
        self.deframer.push(&self.soft);
        while let Some(outcome) = self.deframer.next_frame() {
            self.count(outcome);
            if outcome.failed {
                continue;
            }
            self.since_frame = 0;
            self.coarse.hold();
            let mut vcdu = [0u8; crate::ccsds::VCDU_BYTES];
            vcdu.copy_from_slice(self.deframer.vcdu());
            self.push_vcdu(&vcdu, on_file);
        }
        if self.since_frame > LOST_SYMBOLS {
            self.coarse.search();
        }
    }

    fn count(&mut self, outcome: FrameOutcome) {
        self.frames = self.frames.saturating_add(1);
        if outcome.failed {
            self.frames_failed = self.frames_failed.saturating_add(1);
        } else if outcome.corrected > 0 {
            self.frames_corrected = self.frames_corrected.saturating_add(1);
        }
    }

    /// Takes one corrected VCDU, as from a CADU file another receiver wrote.
    pub fn push_vcdu(&mut self, vcdu: &[u8], on_file: &mut impl FnMut(TransportFile<'_>)) {
        let Some(header) = VcduHeader::parse(vcdu) else {
            self.frames_failed = self.frames_failed.saturating_add(1);
            return;
        };
        if header.vcid == FILL_VCID {
            return;
        }
        let Some(channel) = self.channels.get_mut(usize::from(header.vcid)) else {
            return;
        };
        channel.load(vcdu);
        while let Some(packet) = channel.next_packet() {
            if let Some(file) = self.sessions.push(header.vcid, packet) {
                on_file(file);
            }
        }
    }
}
