use num_complex::Complex;

use super::{BARKER_CHIPS, CHIP_SAMPLES, SymbolReader, WifiPhy, byte, plcp::Header, quadrant};
use crate::{
    spread::{CckMode, Codebook},
    wifi::Sink,
};

pub const CHIPS: usize = 8;
const MAX_WORDS: usize = 64;

pub(crate) struct Cck {
    full: Codebook,
    half: Codebook,
    bank: [Complex<f32>; MAX_WORDS],
}

fn mode(phy: WifiPhy) -> CckMode {
    if phy == WifiPhy::Cck11m {
        CckMode::Bits8
    } else {
        CckMode::Bits4
    }
}

fn label_bits(phy: WifiPhy) -> usize {
    mode(phy).bits_per_symbol() - 2
}

pub(crate) fn duration(phy: WifiPhy, bytes: usize) -> usize {
    let symbols = (bytes * 8).div_ceil(label_bits(phy) + 2);
    ((symbols * CHIPS) as f64 * CHIP_SAMPLES).ceil() as usize
}

fn qpsk(high: bool, low: bool) -> u32 {
    u32::from(high) << 1 | u32::from(low)
}

fn word_index(phy: WifiPhy, bits: &[bool]) -> u32 {
    if phy == WifiPhy::Cck11m {
        qpsk(bits[0], bits[1]) | qpsk(bits[2], bits[3]) << 2 | qpsk(bits[4], bits[5]) << 4
    } else {
        u32::from(bits[0]) | u32::from(bits[1]) << 1
    }
}

fn word_bits(phy: WifiPhy, index: u32, out: &mut [bool; 6]) {
    if phy == WifiPhy::Cck11m {
        for pair in 0..3 {
            let value = index >> (2 * pair) & 3;
            out[2 * pair] = value & 2 != 0;
            out[2 * pair + 1] = value & 1 != 0;
        }
    } else {
        out[0] = index & 1 != 0;
        out[1] = index & 2 != 0;
    }
}

#[must_use]
pub fn codeword(phy: WifiPhy, phi1: f32, label_bits: &[bool]) -> [Complex<f32>; CHIPS] {
    let book = Codebook::new(mode(phy));
    let rotation = Complex::from_polar(1.0, phi1);
    let word = book.words()[word_index(phy, label_bits) as usize];
    std::array::from_fn(|chip| word[chip] * rotation)
}

fn interpolate(history: &[Complex<f32>], position: f64) -> Complex<f32> {
    let index = position.floor();
    let weight = (position - index) as f32;
    let index = index as usize;
    match (history.get(index), history.get(index + 1)) {
        (Some(&a), Some(&b)) => a * (1.0 - weight) + b * weight,
        _ => Complex::default(),
    }
}

impl Cck {
    pub(crate) fn new() -> Self {
        Self {
            full: Codebook::new(CckMode::Bits8),
            half: Codebook::new(CckMode::Bits4),
            bank: [Complex::default(); MAX_WORDS],
        }
    }

    pub(crate) fn psdu(
        &mut self,
        reader: &mut SymbolReader<'_>,
        header: &Header,
        bytes: &mut Vec<u8>,
        sink: &impl Sink,
    ) -> bool {
        let book = if header.phy == WifiPhy::Cck11m {
            &self.full
        } else {
            &self.half
        };
        let words = book.words().len();
        let label_bits = label_bits(header.phy);
        let symbols = (header.bytes * 8).div_ceil(label_bits + 2);
        let start = reader.chip_start();
        let history = reader.history();
        let end = start + (symbols * CHIPS) as f64 * CHIP_SAMPLES;
        let rotation = reader.rotation().powf(CHIPS as f32 / BARKER_CHIPS as f32);
        let mut previous = reader.last_phase();
        let mut pending = [false; 8];
        let mut filled = 0;
        let mut labels = [false; 6];
        for symbol in 0..symbols {
            let chips: [Complex<f32>; CHIPS] = std::array::from_fn(|chip| {
                let position = start + ((symbol * CHIPS + chip) as f64 + 0.5) * CHIP_SAMPLES - 0.5;
                interpolate(history, position)
            });
            book.correlate(&chips, &mut self.bank);
            let (index, z) = self.bank[..words]
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.norm_sqr().total_cmp(&b.1.norm_sqr()))
                .map_or((0, Complex::default()), |(index, &z)| (index as u32, z));
            let mut differential = z * previous.conj() * rotation;
            if symbol % 2 == 1 {
                differential = -differential;
            }
            previous = z;
            word_bits(header.phy, index, &mut labels);
            let phase_bits = quadrant(differential);
            for &bit in phase_bits.iter().chain(&labels[..label_bits]) {
                pending[filled] = reader.descrambled(bit);
                filled += 1;
                if filled == 8 && bytes.len() < header.bytes {
                    bytes.push(byte(&pending));
                    filled = 0;
                    if bytes.len() == 1 && !sink.accepts(bytes[0]) {
                        reader.advance_to(end);
                        return false;
                    }
                }
            }
        }
        reader.advance_to(end);
        true
    }
}
