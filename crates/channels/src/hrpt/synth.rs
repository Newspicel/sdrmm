use num_complex::Complex;
use sdrmm_dsp::FracResampler;
use sdrmm_modem::{
    ccsds::{
        self, ASM_BYTES, Basis, Cadu, CaduWriter, NO_PACKET_START, PACKET_HEADER, PacketHeader,
        Segment, randomize,
    },
    pulse::{self, Norm},
    split_phase::modulate,
};

use super::{
    BIT_RATE, HrptSatellite,
    meteor::{CADU_BYTES, CHANNELS, LINE_BYTES, LINE_MS, LINE_SPANS, LINE_SYNC, TIME_AT, WIDTH},
    metop, noaa,
};

pub const INDEX_RAD: f64 = 1.2;
pub const START_HOUR_MOSCOW: u8 = 8;
const PIXELS_AT: usize = 50;
const GROUP_STRIDE: usize = 30;
const GROUP_BYTES: usize = 5;
/// About 25 ms of filler, for the carrier and bit clock to lock before the first frame.
const LEAD_BYTES: usize = 2_048;
/// Fillers before the first `MetOp` frame, about 40 ms for the carrier loop to pull in.
const METOP_LEAD: usize = 8;

/// The 10-bit level the test scene has at `x` on `line` in `channel`.
#[must_use]
pub fn level(channel: usize, line: usize, x: usize) -> u16 {
    ((x * 3 + line * 7 + channel * 101) % 1024) as u16
}

/// MSU-MR line `index` of the test scene, stamped from 08:00:00 Moscow time.
#[must_use]
pub fn scan_line(index: usize) -> Vec<u8> {
    let mut line = vec![0u8; LINE_BYTES];
    line[..8].copy_from_slice(&LINE_SYNC.to_be_bytes());
    let ticks = (index as f64 * LINE_MS * 0.255).round() as u32;
    let seconds = ticks / 255;
    line[TIME_AT..TIME_AT + 4].copy_from_slice(&[
        START_HOUR_MOSCOW + (seconds / 3600) as u8,
        (seconds / 60 % 60) as u8,
        (seconds % 60) as u8,
        (ticks % 255) as u8,
    ]);
    for channel in 0..CHANNELS {
        for group in 0..WIDTH / 4 {
            let [a, b, c, d] = std::array::from_fn(|k| level(channel, index, group * 4 + k));
            let at = PIXELS_AT + group * GROUP_STRIDE + channel * GROUP_BYTES;
            line[at..at + GROUP_BYTES].copy_from_slice(&[
                (a >> 2) as u8,
                ((a & 0x03) << 6 | b >> 4) as u8,
                ((b & 0x0F) << 4 | c >> 6) as u8,
                ((c & 0x3F) << 2 | d >> 8) as u8,
                d as u8,
            ]);
        }
    }
    line
}

fn empty_cadu() -> [u8; CADU_BYTES] {
    let mut cadu = [0u8; CADU_BYTES];
    cadu[..ASM_BYTES].copy_from_slice(&ccsds::ASM.to_be_bytes());
    cadu
}

/// CADUs carrying `lines` scan lines, the stream padded to whole CADUs.
#[must_use]
pub fn cadus(lines: usize) -> Vec<[u8; CADU_BYTES]> {
    let stream: Vec<u8> = (0..lines).flat_map(scan_line).collect();
    let per_cadu: usize = LINE_SPANS.iter().map(|&(_, len)| len).sum();
    stream
        .chunks(per_cadu)
        .map(|chunk| {
            let mut cadu = empty_cadu();
            let mut taken = 0;
            for &(start, len) in &LINE_SPANS {
                let part = &chunk[taken.min(chunk.len())..(taken + len).min(chunk.len())];
                cadu[start..start + part.len()].copy_from_slice(part);
                taken += len;
            }
            cadu
        })
        .collect()
}

/// NOAA minor frame `index` of the test scene, its time code from 05:00:00 UTC.
#[must_use]
pub fn minor_frame(index: usize) -> Vec<u16> {
    let mut words = vec![0u16; noaa::FRAME_WORDS];
    words[..6].copy_from_slice(&[644, 367, 860, 413, 527, 149]);
    let ms = 5 * 3_600_000 + (index as f64 * noaa::LINE_MS).round() as u32;
    words[8..12].copy_from_slice(&[
        622,
        (ms >> 20 & 0x7F) as u16,
        (ms >> 10 & 0x3FF) as u16,
        (ms & 0x3FF) as u16,
    ]);
    for pixel in 0..noaa::WIDTH {
        for channel in 0..noaa::CHANNELS {
            words[750 + pixel * noaa::CHANNELS + channel] = level(channel, index, pixel);
        }
    }
    words
}

/// Random filler, as a downlink sends before its first frame: an alternating pattern would let
/// the bit clock settle half a bit off.
fn filler() -> Vec<u8> {
    let mut state = 0x2545_F491_u32;
    (0..LEAD_BYTES)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 17;
            state ^= state << 5;
            (state >> 24) as u8
        })
        .collect()
}

fn bits_of_bytes(bytes: &[u8]) -> impl Iterator<Item = bool> + '_ {
    bytes
        .iter()
        .flat_map(|byte| (0..8).rev().map(move |shift| byte >> shift & 1 == 1))
}

/// The downlink of `lines` scan lines at `rate`, its carrier `offset_hz` off centre, and one
/// more whose sync word vouches for the last.
#[must_use]
pub fn transmission(
    satellite: HrptSatellite,
    lines: usize,
    rate: f64,
    offset_hz: f64,
) -> Vec<Complex<f32>> {
    let mut bits: Vec<bool> = bits_of_bytes(&filler()).collect();
    match satellite {
        HrptSatellite::Meteor => {
            let mut frames = cadus(lines + 1);
            frames.push(empty_cadu());
            bits.extend(bits_of_bytes(frames.as_flattened()));
        }
        HrptSatellite::Metop => {
            let mut cadus = metop_cadus(lines);
            cadus.extend(metop_cadus(0));
            return metop_signal(cadus.as_flattened(), rate, offset_hz);
        }
        HrptSatellite::Noaa => {
            for index in 0..=lines {
                for word in minor_frame(index) {
                    bits.extend(
                        (0..noaa::WORD_BITS)
                            .rev()
                            .map(|shift| word >> shift & 1 == 1),
                    );
                }
            }
        }
    }
    modulate(&bits, rate, BIT_RATE, INDEX_RAD, offset_hz)
}

/// `MetOp` AVHRR line `index` of the test scene: an APID 103 packet stamped from 05:00:00 UTC.
#[must_use]
pub fn avhrr_packet(index: usize) -> Vec<u8> {
    let mut packet = vec![0u8; metop::PACKET_BYTES];
    PacketHeader {
        secondary: true,
        apid: metop::AVHRR_APIDS[0],
        segment: Segment::Unsegmented,
        sequence: index as u16,
        length: metop::PACKET_BYTES - PACKET_HEADER,
    }
    .write(&mut packet);
    let ms = 5 * 3_600_000 + (index as f64 * metop::LINE_MS).round() as u32;
    packet[6..8].copy_from_slice(&8_825u16.to_be_bytes());
    packet[8..12].copy_from_slice(&ms.to_be_bytes());
    let words = &mut packet[20..metop::PACKET_BYTES - 2];
    for pixel in 0..metop::WIDTH {
        for channel in 0..5 {
            let at = (55 + pixel * 5 + channel) * 10;
            let value = level(channel, index, pixel);
            for bit in 0..10 {
                if value >> (9 - bit) & 1 == 1 {
                    let at = at + bit;
                    words[at / 8] |= 0x80 >> (at % 8);
                }
            }
        }
    }
    let sum = packet[..metop::PACKET_BYTES - 2]
        .as_chunks::<2>()
        .0
        .iter()
        .fold(0u16, |sum, &word| sum ^ u16::from_be_bytes(word));
    packet[metop::PACKET_BYTES - 2..].copy_from_slice(&sum.to_be_bytes());
    packet
}

/// `MetOp` CADUs carrying `lines` AVHRR lines on virtual channel 9, padded to whole units, as
/// the archive keeps them: derandomized, Reed-Solomon parity in place.
#[must_use]
pub fn metop_cadus(lines: usize) -> Vec<Cadu> {
    let zone = metop::LAYOUT.zone_len();
    let mut stream = vec![0u8; 300];
    let mut starts = Vec::new();
    for index in 0..lines {
        starts.push(stream.len());
        stream.extend(avhrr_packet(index));
    }
    let writer = CaduWriter::new(11, metop::LAYOUT, Basis::Dual);
    stream
        .chunks(zone)
        .enumerate()
        .map(|(unit, chunk)| {
            let from = unit * zone;
            let first = starts
                .iter()
                .find(|&&start| (from..from + zone).contains(&start))
                .map_or(NO_PACKET_START, |&start| (start - from) as u16);
            let mut cadu = writer.cadu(metop::AVHRR_VCID, unit as u32, first, chunk);
            randomize(&mut cadu[ASM_BYTES..]);
            cadu
        })
        .collect()
}

/// The AHRPT downlink of `cadus` (derandomized, as archived) at `rate`, `offset_hz` off centre.
#[must_use]
pub fn metop_signal(cadus: &[u8], rate: f64, offset_hz: f64) -> Vec<Complex<f32>> {
    let mut bytes: Vec<u8> = (0..METOP_LEAD).flat_map(|_| filler()).collect();
    for cadu in cadus.chunks(ccsds::CADU_BYTES) {
        let mut cadu = cadu.to_vec();
        randomize(&mut cadu[ASM_BYTES..]);
        bytes.extend(cadu);
    }
    bytes.extend(filler());
    let bits: Vec<bool> = bits_of_bytes(&bytes).collect();
    let symbols = metop::coding::tests::encode(&bits);
    let sps = metop::SPS;
    let taps = pulse::root_raised_cosine(sps as f64, metop::ROLL_OFF, 8, Norm::Energy);
    let mut shaped = vec![Complex::default(); symbols.len() * sps + taps.len()];
    for (index, &symbol) in symbols.iter().enumerate() {
        for (k, &tap) in taps.iter().enumerate() {
            shaped[index * sps + k] += symbol * tap;
        }
    }
    let mut out = Vec::with_capacity(shaped.len());
    FracResampler::new(rate / metop::DEMOD_RATE_HZ).process(&shaped, &mut out);
    let step = std::f64::consts::TAU * offset_hz / rate;
    for (index, sample) in out.iter_mut().enumerate() {
        let phase = step * index as f64;
        *sample *= Complex::new(phase.cos() as f32, phase.sin() as f32) * 0.3;
    }
    out
}
