//! The C4FM front end of DMR, P25, NXDN, YSF, dPMR, M17 and TETRAPOL: RRC shaped 4FSK on
//! the dibit levels +1, +3, -1, -3, and a window that slices dibits against a sync anchor.

use std::collections::VecDeque;

use sdrmm_dsp::coding::fec::conv::CONFIDENT;

use super::{CpmDemod, CpmParams, KnownSymbols, Mapping, TIMING_BW_BURST};
use crate::{
    pulse::{self, Norm},
    soft::SoftBit,
};

pub const RRC_SPAN: usize = 8;

/// How long a level anchor holds without a fresh sync, in symbols.
const ANCHOR_TIMEOUT_SYMBOLS: u32 = 4_800;

#[must_use]
pub fn dibit_mapping() -> Mapping {
    Mapping::new(vec![1.0, 3.0, -1.0, -3.0])
}

#[must_use]
pub fn c4fm_params(input_rate: f64, baud: f64, deviation_hz: f64, alpha: f64) -> CpmParams {
    let sps = input_rate / baud;
    CpmParams::from_deviation(
        dibit_mapping(),
        deviation_hz,
        baud,
        pulse::root_raised_cosine(sps, alpha, RRC_SPAN, Norm::Area),
        sps,
    )
}

#[must_use]
pub fn c4fm_demod(params: &CpmParams) -> CpmDemod {
    CpmDemod::new(params, params.freq_pulse(), TIMING_BW_BURST)
}

/// A sync word of `bits` with every dibit's sign flipped, as an inverted channel sends it.
#[must_use]
pub const fn inverted(pattern: u64, bits: u32) -> u64 {
    pattern ^ (0xAAAA_AAAA_AAAA_AAAA >> (64 - bits))
}

pub fn invert_dibits(bits: &mut [bool]) {
    for bit in bits.iter_mut().step_by(2) {
        *bit = !*bit;
    }
}

pub struct DibitWindow {
    soft: VecDeque<f32>,
    register: u64,
    capacity: usize,
    mapping: Mapping,
    levels: KnownSymbols,
    measured: Vec<f32>,
    pattern: Vec<u8>,
    soft_scratch: Vec<SoftBit>,
}

impl DibitWindow {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            soft: VecDeque::with_capacity(capacity),
            register: 0,
            capacity,
            mapping: dibit_mapping(),
            levels: KnownSymbols::from_mapping(&dibit_mapping(), ANCHOR_TIMEOUT_SYMBOLS),
            measured: Vec::new(),
            pattern: Vec::new(),
            soft_scratch: Vec::new(),
        }
    }

    pub fn push(&mut self, symbol: f32) {
        if self.soft.len() == self.capacity {
            self.soft.pop_front();
        }
        self.soft.push_back(symbol);
        self.levels.tick();
        self.register =
            self.register << 2 | u64::from(self.mapping.slice(self.levels.correct(symbol)));
    }

    pub fn anchor(&mut self, pattern: u64, bits: u32) {
        self.measured.clear();
        self.pattern.clear();
        for i in 0..bits as usize / 2 {
            self.measured.push(self.raw(i));
            self.pattern.push((pattern >> (2 * i)) as u8 & 0b11);
        }
        self.levels.anchor(&self.pattern, &self.measured);
    }

    #[must_use]
    pub fn sync_distance(&self, pattern: u64, bits: u32) -> u32 {
        self.distance_before(pattern, bits, 0)
    }

    /// Hamming distance of `pattern` to the symbols that ended `back` symbols ago.
    #[must_use]
    pub fn distance_before(&self, pattern: u64, bits: u32, back: u32) -> u32 {
        let mask = if bits >= 64 {
            u64::MAX
        } else {
            (1 << bits) - 1
        };
        let shift = 2 * back;
        let register = if shift >= 64 {
            0
        } else {
            self.register >> shift
        };
        sdrmm_dsp::hamming_distance(register & mask, pattern & mask)
    }

    #[must_use]
    pub fn soft(&self, back: usize) -> f32 {
        self.levels.correct(self.raw(back))
    }

    fn raw(&self, back: usize) -> f32 {
        let len = self.soft.len();
        if back >= len {
            return 0.0;
        }
        self.soft[len - 1 - back]
    }

    pub fn bits(&self, end_back: usize, symbols: usize, out: &mut Vec<bool>) {
        out.clear();
        for i in (0..symbols).rev() {
            let dibit = self.mapping.slice(self.soft(end_back + i));
            out.push(dibit & 0b10 != 0);
            out.push(dibit & 0b01 != 0);
        }
    }

    pub fn soft_bits(&mut self, end_back: usize, symbols: usize, out: &mut Vec<i16>) {
        out.clear();
        for i in (0..symbols).rev() {
            self.soft_scratch.clear();
            self.mapping
                .soft_bits(self.soft(end_back + i), &mut self.soft_scratch);
            out.extend(
                self.soft_scratch
                    .iter()
                    .map(|b| (b.0 * f32::from(CONFIDENT)) as i16),
            );
        }
    }

    pub fn vocoder_soft_bits(&mut self, end_back: usize, symbols: usize, out: &mut Vec<i8>) {
        out.clear();
        for i in (0..symbols).rev() {
            self.soft_scratch.clear();
            self.mapping
                .soft_bits(self.soft(end_back + i), &mut self.soft_scratch);
            out.extend(
                self.soft_scratch
                    .iter()
                    .map(|b| (b.0 * f32::from(i8::MAX)) as i8),
            );
        }
    }

    pub fn reset(&mut self) {
        self.soft.clear();
        self.register = 0;
        self.levels.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_inverted_sync_flips_every_dibit_sign() {
        let mut bits = [false, true, true, true];
        invert_dibits(&mut bits);
        assert_eq!(bits, [true, true, false, true]);
        assert_eq!(inverted(0b0111, 4), 0b1101);
    }

    #[test]
    fn the_window_finds_a_sync_and_slices_its_dibits() {
        let mapping = dibit_mapping();
        let sync = [0b01u8, 0b11, 0b00, 0b10, 0b01];
        let pattern = sync
            .iter()
            .fold(0u64, |word, &dibit| word << 2 | u64::from(dibit));
        let mut window = DibitWindow::new(16);
        for &dibit in &[0b00u8, 0b11, 0b10]
            .iter()
            .chain(&sync)
            .copied()
            .collect::<Vec<_>>()
        {
            window.push(mapping.levels()[usize::from(dibit)]);
        }
        assert_eq!(window.sync_distance(pattern, 10), 0);
        assert_eq!(window.sync_distance(inverted(pattern, 10), 10), 5);
        window.anchor(pattern, 10);
        let mut bits = Vec::new();
        window.bits(0, 2, &mut bits);
        assert_eq!(bits, vec![true, false, false, true]);
        let mut soft = Vec::new();
        window.soft_bits(0, 1, &mut soft);
        assert!(soft[0] < 0 && soft[1] > 0, "{soft:?}");
    }
}
