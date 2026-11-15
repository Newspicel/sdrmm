use num_complex::Complex;

use super::{
    super::{
        BlePhy, access_address_bits,
        coded::{AA_BLOCKS, INDICATOR_BLOCKS, coded_access_address, indicator, s8_pattern},
        le1m_preamble,
        track::pseudo_symbols,
    },
    AA_SYMBOLS, ONE_SYMBOLS,
};

pub(super) struct Patterns {
    pub(super) one_symbols: Vec<bool>,
    pub(super) one_rising: u64,
    pub(super) one_turns: u64,
    pub(super) one_known: Vec<Complex<f32>>,
    pub(super) coded: u64,
    pub(super) coded_known: Vec<Complex<f32>>,
    pub(super) indicators: [(BlePhy, [bool; INDICATOR_BLOCKS]); 2],
}

impl Patterns {
    pub(super) fn new() -> Self {
        let one_symbols: Vec<bool> = le1m_preamble().chain(access_address_bits()).collect();
        let mut one_known = Vec::with_capacity(ONE_SYMBOLS);
        pseudo_symbols(one_symbols.iter().copied(), &mut one_known);
        let (coded_bits, after) = coded_access_address();
        let coded = (0..AA_BLOCKS).fold(0u64, |acc, j| {
            acc | u64::from(coded_bits[AA_BLOCKS - 1 - j]) << j
        });
        let mut coded_known = Vec::with_capacity(AA_SYMBOLS);
        pseudo_symbols(
            coded_bits.iter().flat_map(|&bit| s8_pattern(bit)),
            &mut coded_known,
        );
        let (one_rising, one_turns) = transitions(&one_symbols);
        Self {
            one_symbols,
            one_rising,
            one_turns,
            one_known,
            coded,
            coded_known,
            indicators: [
                (BlePhy::CodedS8, indicator(after, BlePhy::CodedS8)),
                (BlePhy::CodedS2, indicator(after, BlePhy::CodedS2)),
            ],
        }
    }
}

fn transitions(symbols: &[bool]) -> (u64, u64) {
    let last = symbols.len() - 1;
    symbols
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| pair[0] != pair[1])
        .fold((0, 0), |(rising, turns), (k, pair)| {
            let bit = 1u64 << (last - 1 - k);
            (rising | if pair[1] { bit } else { 0 }, turns | bit)
        })
}
