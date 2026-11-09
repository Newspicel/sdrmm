use num_complex::Complex;

use super::{
    CHIP_SAMPLES, SYMBOL, SymbolReader, WifiPhy, byte, nearest_quadrant, plcp::Header, quadrant,
};
use crate::{
    linear::PhaseTracker,
    spread::{CckMode, Codebook, integrate},
    wifi::Sink,
};

pub const CHIPS: usize = 8;
const MAX_WORDS: usize = 64;
const SYMBOL_SAMPLES: f64 = CHIPS as f64 * CHIP_SAMPLES;
const TIMING_GAIN: f64 = 0.1;
const TIMING_SPREAD: f64 = CHIP_SAMPLES / 2.0;

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

/// The eight chips of a symbol starting at `start`, each integrated over its own interval and
/// taken off the carrier at its own centre.
fn chips(
    history: &[Complex<f32>],
    start: f64,
    derotators: &[Complex<f32>; CHIPS],
) -> [Complex<f32>; CHIPS] {
    std::array::from_fn(|chip| {
        let from = start + chip as f64 * CHIP_SAMPLES;
        integrate(history, from, from + CHIP_SAMPLES) * derotators[chip]
    })
}

/// One phasor per chip, the tracked carrier at that chip's centre relative to the symbol's.
fn derotators(tracker: &PhaseTracker) -> [Complex<f32>; CHIPS] {
    let step = Complex::from_polar(1.0, -tracker.freq() * CHIP_SAMPLES as f32);
    let first = tracker.derotator((0.5 * CHIP_SAMPLES - 0.5 * SYMBOL_SAMPLES) as f32);
    let mut phasor = first;
    std::array::from_fn(|_| {
        let here = phasor;
        phasor *= step;
        here
    })
}

/// The word and quadrant whose rotation of the bank entry lies furthest along the real axis.
fn decide(bank: &[Complex<f32>]) -> (u32, Complex<f32>) {
    let mut best = (0, f32::NEG_INFINITY);
    for (index, z) in bank.iter().enumerate() {
        let metric = z.re.abs().max(z.im.abs());
        if metric > best.1 {
            best = (index, metric);
        }
    }
    (best.0 as u32, nearest_quadrant(bank[best.0]))
}

fn energy(chips: &[Complex<f32>; CHIPS], word: &[Complex<f32>; CHIPS]) -> f32 {
    chips
        .iter()
        .zip(word)
        .map(|(&y, &w)| y * w.conj())
        .sum::<Complex<f32>>()
        .norm()
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
        let history = reader.history();
        let mut start = reader.chip_start();
        let end = start + symbols as f64 * SYMBOL_SAMPLES;
        let (mut tracker, mut point) = reader.carrier();
        tracker.advance((0.5 * (SYMBOL as f64 + SYMBOL_SAMPLES)) as f32);
        let mut pending = [false; 8];
        let mut filled = 0;
        let mut labels = [false; 6];
        for symbol in 0..symbols {
            let turns = derotators(&tracker);
            let on_time = chips(history, start, &turns);
            book.correlate(&on_time, &mut self.bank);
            let (index, decided) = decide(&self.bank[..words]);
            tracker.steer(self.bank[index as usize], decided, SYMBOL_SAMPLES as f32);
            let mut step = decided * point.conj();
            if symbol % 2 == 1 {
                step = -step;
            }
            point = decided;
            let word = &book.words()[index as usize];
            let early = energy(&chips(history, start - TIMING_SPREAD, &turns), word);
            let late = energy(&chips(history, start + TIMING_SPREAD, &turns), word);
            let nudge = if early + late > 0.0 {
                TIMING_GAIN * f64::from((late - early) / (late + early)) * TIMING_SPREAD
            } else {
                0.0
            };
            start += SYMBOL_SAMPLES + nudge;
            tracker.advance((SYMBOL_SAMPLES + nudge) as f32);
            word_bits(header.phy, index, &mut labels);
            for &bit in quadrant(step).iter().chain(&labels[..label_bits]) {
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
        reader.advance_to(start.max(end));
        true
    }
}
