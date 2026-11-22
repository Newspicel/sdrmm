//! `MetOp` AHRPT: QPSK at 2.3333 Msym/s carrying CCSDS CADUs under a rate 3/4 K=7 code and
//! Reed-Solomon (255,223) four deep. AVHRR/3 rides virtual channel 9 as one packet a scan line,
//! APID 103 when channel 3A is on and 104 for 3B, six lines a second at 2048 pixels.

pub(crate) mod coding;

use num_complex::Complex;
use sdrmm_decoder::ChannelError;
use sdrmm_dsp::{FracResampler, coding::fec::ccsds_rs::CcsdsReedSolomon};
use sdrmm_modem::{
    ccsds::{
        ASM, ASM_BYTES, CADU_BITS, CADU_BYTES, Depacketizer, INTERLEAVE, MAX_PACKET, MpduLayout,
        PacketHeader, VCDU_BYTES, randomize, sequence_gap,
    },
    constellation::{Constellation, tables},
    framesync::{FixedFramer, Framed},
    linear::{CarrierLoop, LinearDemod, LinearParams, LinearTiming, PhaseDetector},
    pulse::{self, Norm},
};

use super::{
    image::{PLANES, Scan},
    meteor::Pushed,
    noaa,
};
use coding::{Coder, MOTHER_CAPACITY};

pub const SYMBOL_RATE: f64 = 2_333_333.0;
pub const SPS: usize = 2;
/// What a 4 MS/s radio gives, the 3.5 MHz signal and a margin; resampled to two a symbol.
pub const INPUT_RATE_HZ: f64 = 4_000_000.0;
pub const DEMOD_RATE_HZ: f64 = SPS as f64 * SYMBOL_RATE;
pub const ROLL_OFF: f64 = 0.5;
pub const BANDWIDTH_HZ: f64 = SYMBOL_RATE * (1.0 + ROLL_OFF);
const PULSE_SPAN: usize = 8;
const CARRIER_BW: f64 = 0.004;
const FREQUENCY_AID: f64 = 0.01;
/// A two-byte insert zone sits ahead of the `M_PDU`.
pub const LAYOUT: MpduLayout = MpduLayout { insert_zone: 2 };
pub const AVHRR_VCID: u8 = 9;
pub const AVHRR_APIDS: [u16; 2] = [103, 104];
pub const PACKET_BYTES: usize = 12_966;
pub const WIDTH: usize = 2_048;
pub const LINE_MS: f64 = 1_000.0 / 6.0;
const CHANNELS: usize = 5;
/// The AVHRR words start this far into the packet, after the headers and the time code.
const WORDS_AT: usize = 20;
const PIXELS_AT: usize = 55;
const TIME_AT: usize = 8;
const MS_PER_DAY: u32 = 86_400_000;
/// The channels the picture planes take: 1, 2 and 4.
const KEPT: [usize; PLANES] = [0, 1, 3];
const CADU_SEARCH_ERRORS: u32 = 3;
const CADU_TRACK_ERRORS: u32 = 8;
const CADU_MISSES: u8 = 3;
/// Decoded bits without a frame Reed-Solomon accepts before the lock is given up.
const LOSS_BITS: usize = 6 * CADU_BITS;
/// A gap in a line's packet count larger than this is a new pass, not lost lines.
const MAX_GAP: u32 = 1_000;

fn demodulator() -> Result<LinearDemod, ChannelError> {
    let invalid = |error: String| ChannelError::InvalidSettings(format!("MetOp QPSK: {error}"));
    let constellation = tables::psk_rotated(4, std::f64::consts::FRAC_PI_4)
        .map_err(|error| invalid(error.to_string()))?;
    let pulse = pulse::root_raised_cosine(SPS as f64, ROLL_OFF, PULSE_SPAN, Norm::Energy);
    let params = LinearParams::new(constellation, pulse.clone(), SPS)
        .map_err(|error| invalid(error.to_string()))?;
    let carrier = CarrierLoop::new(PhaseDetector::MthPower { m: 4 }, CARRIER_BW)
        .with_frequency_aid(FREQUENCY_AID);
    Ok(LinearDemod::new(
        &params,
        &pulse,
        LinearTiming::CONTINUOUS,
        Some(carrier),
    ))
}

/// The packet sum check: every big-endian 16-bit word folded with XOR, the last one included, is zero.
#[must_use]
pub fn parity_holds(packet: &[u8]) -> bool {
    packet
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0u16, |sum, &word| sum ^ u16::from_be_bytes(word))
        == 0
}

/// The AVHRR packets of virtual channel 9.
#[must_use]
pub fn depacketizer() -> Depacketizer {
    Depacketizer::new(LAYOUT, MAX_PACKET).with_vcid(AVHRR_VCID)
}

/// Line packets read, kept and lost.
#[derive(Default)]
struct Lines {
    last: Option<(u16, u16)>,
    lost: u32,
}

impl Lines {
    /// Whether `packet` is a whole AVHRR line; lines it shows were skipped count as lost.
    fn take(&mut self, packet: &[u8]) -> bool {
        let Some(header) = PacketHeader::parse(packet) else {
            return false;
        };
        if !AVHRR_APIDS.contains(&header.apid) {
            return false;
        }
        if packet.len() != PACKET_BYTES || !parity_holds(packet) {
            self.lost = self.lost.saturating_add(1);
            return false;
        }
        if let Some((apid, previous)) = self.last
            && apid == header.apid
        {
            let gap = sequence_gap(previous, header.sequence);
            if gap < MAX_GAP {
                self.lost = self.lost.saturating_add(gap);
            }
        }
        self.last = Some((header.apid, header.sequence));
        true
    }
}

pub struct Link {
    resampler: FracResampler,
    resampled: Vec<Complex<f32>>,
    demod: LinearDemod,
    symbols: Vec<Complex<f32>>,
    coder: Coder,
    bits: Vec<bool>,
    read: usize,
    framer: FixedFramer,
    cadu: [u8; CADU_BYTES],
    rs: CcsdsReedSolomon,
    packets: Depacketizer,
    line: Box<[u8; PACKET_BYTES]>,
    lines: Lines,
    failed: u32,
    since_good: usize,
}

impl Link {
    pub fn new() -> Result<Self, ChannelError> {
        Ok(Self {
            resampler: FracResampler::new(DEMOD_RATE_HZ / INPUT_RATE_HZ),
            resampled: Vec::with_capacity(2 * super::CHUNK),
            demod: demodulator()?,
            symbols: Vec::with_capacity(super::CHUNK),
            coder: Coder::new(),
            bits: Vec::with_capacity(2 * MOTHER_CAPACITY),
            read: 0,
            framer: FixedFramer::new(
                u64::from(ASM),
                32,
                CADU_BITS,
                CADU_SEARCH_ERRORS,
                CADU_TRACK_ERRORS,
                CADU_MISSES,
            ),
            cadu: [0; CADU_BYTES],
            rs: CcsdsReedSolomon::new(),
            packets: depacketizer(),
            line: Box::new([0; PACKET_BYTES]),
            lines: Lines::default(),
            failed: 0,
            since_good: 0,
        })
    }

    pub fn reset(&mut self) {
        self.resampler.reset();
        self.demod.reset();
        self.coder.reset();
        self.framer.reset();
        self.packets.reset();
        self.lines.last = None;
        self.bits.clear();
        self.read = 0;
        self.since_good = 0;
    }

    /// CADUs Reed-Solomon could not mend, frames the framer gave up and lines never drawn.
    #[must_use]
    pub fn dropped(&self) -> u32 {
        self.failed
            .saturating_add(self.framer.dropped())
            .saturating_add(self.lines.lost)
    }

    /// The last complete AVHRR packet, primary header first.
    #[must_use]
    pub fn line(&self) -> &[u8] {
        self.line.as_slice()
    }

    /// The decision points of the last [`Self::feed`], after timing and carrier recovery.
    #[must_use]
    pub fn symbols(&self) -> &[Complex<f32>] {
        &self.symbols
    }

    #[must_use]
    pub fn constellation(&self) -> &Constellation {
        self.demod.params().constellation()
    }

    #[must_use]
    pub fn frequency_offset_hz(&self) -> f64 {
        self.demod.carrier_freq_cycles_per_symbol() * SYMBOL_RATE
    }

    /// Demodulates and decodes `iq`; [`Self::next`] then hands out what it completed.
    pub fn feed(&mut self, iq: &[Complex<f32>]) {
        self.resampled.clear();
        self.resampler.process(iq, &mut self.resampled);
        self.symbols.clear();
        self.demod.process(&self.resampled, &mut self.symbols);
        self.bits.clear();
        self.read = 0;
        self.coder.push(&self.symbols, &mut self.bits);
    }

    pub fn next(&mut self) -> Option<Pushed> {
        while let Some(&bit) = self.bits.get(self.read) {
            self.read += 1;
            self.since_good += 1;
            if self.since_good > LOSS_BITS {
                self.since_good = 0;
                self.coder.unlock();
                self.framer.reset();
            }
            if let Some(framed) = self.framer.push(bit) {
                return Some(self.on_cadu(framed));
            }
        }
        None
    }

    /// A CADU Reed-Solomon cannot mend counts as damaged and is passed over; the packet it
    /// broke shows as a gap in the unit counter.
    fn on_cadu(&mut self, framed: Framed) -> Pushed {
        self.cadu
            .copy_from_slice(&self.framer.frame()[..CADU_BYTES]);
        let coded = &mut self.cadu[ASM_BYTES..];
        randomize(coded);
        if self.rs.decode_interleaved(coded, INTERLEAVE).failed > 0 {
            self.failed = self.failed.saturating_add(1);
            return Pushed {
                cadu: Some(Framed {
                    flywheel: true,
                    ..framed
                }),
                line: false,
            };
        }
        self.since_good = 0;
        self.packets
            .load(&self.cadu[ASM_BYTES..ASM_BYTES + VCDU_BYTES]);
        let mut line = false;
        while let Some(packet) = self.packets.next_packet() {
            if self.lines.take(packet) {
                self.line.copy_from_slice(packet);
                line = true;
            }
        }
        Pushed {
            cadu: Some(Framed {
                flywheel: false,
                ..framed
            }),
            line,
        }
    }
}

/// Milliseconds of the UTC day the line was scanned, `None` when its stamp is no time of day.
#[must_use]
pub fn time_ms(line: &[u8]) -> Option<u32> {
    let stamp = line.get(TIME_AT..TIME_AT + 4)?;
    let ms = u32::from_be_bytes(stamp.try_into().ok()?);
    (ms < MS_PER_DAY).then_some(ms)
}

/// Channel `channel` (0 to 4) of the line's AVHRR scan into `out`.
pub fn unpack(line: &[u8], channel: usize, out: &mut [u16; WIDTH]) {
    let words = line.get(WORDS_AT..).unwrap_or_default();
    for (pixel, value) in out.iter_mut().enumerate() {
        *value = noaa::word(words, PIXELS_AT + pixel * CHANNELS + channel);
    }
}

/// AVHRR channels 1, 2 and 4 of `line` into the picture's planes.
pub fn scan(line: &[u8], out: &mut Scan) {
    for (plane, &channel) in out.planes.iter_mut().zip(&KEPT) {
        if let Ok(row) = <&mut [u16; WIDTH]>::try_from(&mut plane[..]) {
            unpack(line, channel, row);
        }
    }
    out.time_ms = time_ms(line);
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_modem::ccsds::{PACKET_HEADER, Segment};

    fn packet(apid: u16, sequence: u16) -> Vec<u8> {
        let mut bytes = vec![0u8; PACKET_BYTES];
        PacketHeader {
            secondary: true,
            apid,
            segment: Segment::Unsegmented,
            sequence,
            length: PACKET_BYTES - PACKET_HEADER,
        }
        .write(&mut bytes);
        bytes[100] = 0x5A;
        let sum = bytes[..PACKET_BYTES - 2]
            .as_chunks::<2>()
            .0
            .iter()
            .fold(0u16, |sum, &word| sum ^ u16::from_be_bytes(word));
        bytes[PACKET_BYTES - 2..].copy_from_slice(&sum.to_be_bytes());
        bytes
    }

    #[test]
    fn skipped_and_corrupt_lines_count_as_lost() {
        let mut lines = Lines::default();
        assert!(lines.take(&packet(103, 16_383)));
        assert!(lines.take(&packet(103, 0)));
        assert!(lines.take(&packet(103, 3)));
        assert_eq!(lines.lost, 2);
        let mut corrupt = packet(103, 4);
        corrupt[200] ^= 1;
        assert!(!lines.take(&corrupt));
        assert!(!lines.take(&packet(34, 5)));
        assert_eq!(lines.lost, 3);
    }

    #[test]
    fn the_time_code_reads_milliseconds_of_the_day() {
        let mut line = packet(103, 0);
        line[TIME_AT..TIME_AT + 4].copy_from_slice(&32_816_483u32.to_be_bytes());
        assert_eq!(time_ms(&line), Some(32_816_483));
        line[TIME_AT] = 0xFF;
        assert_eq!(time_ms(&line), None);
    }
}
