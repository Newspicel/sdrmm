use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::pocsag_bch_encode;

use crate::flex::{SYNC_CODES, SYNC_MARKER, codeword::WORDS};

const CAPCODE_OFFSET: u32 = 0x8000;
const WORD_BITS: usize = 21;
const NUMERIC_HEADER_BITS: usize = 6;
const NUMERIC_FILL: u32 = 12;
const ALPHA_VECTOR: u32 = 5;
const NUMERIC_VECTOR: u32 = 3;

#[derive(Clone, Debug)]
pub struct Page {
    pub address: u32,
    pub text: String,
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Flex1600_2,
    Flex1600_4,
    Flex3200_2,
    Flex3200_4,
}

impl Mode {
    fn sync_code(self) -> usize {
        match self {
            Self::Flex1600_2 => 0,
            Self::Flex1600_4 => 1,
            Self::Flex3200_2 => 2,
            Self::Flex3200_4 => 3,
        }
    }

    fn sync(self) -> u16 {
        SYNC_CODES[self.sync_code()].0
    }

    fn symbol_rate(self) -> usize {
        usize::from(SYNC_CODES[self.sync_code()].1.symbol_rate)
    }

    fn levels(self) -> usize {
        usize::from(SYNC_CODES[self.sync_code()].1.levels)
    }
}

fn reverse31(word: u32) -> u32 {
    word.reverse_bits() >> 1
}

fn reverse21(word: u32) -> u32 {
    word.reverse_bits() >> 11
}

fn encode_word(data: u32) -> u32 {
    let pocsag = pocsag_bch_encode(reverse21(data & 0x1F_FFFF));
    reverse31(pocsag >> 1) | (pocsag & 1) << 31
}

fn checksum(mut data: u32) -> u32 {
    let sum = (1..5).map(|index| data >> (index * 4) & 0xF).sum::<u32>() + (data >> 20);
    data |= (0xF_u32.wrapping_sub(sum)) & 0xF;
    data
}

fn text_words(text: &str) -> Vec<u32> {
    let mut slots = vec![0u8];
    slots.extend(text.bytes().filter(u8::is_ascii).map(|byte| byte & 0x7F));
    slots.push(3);
    while !slots.len().is_multiple_of(3) {
        slots.push(3);
    }
    slots
        .as_chunks::<3>()
        .0
        .iter()
        .map(|chunk| u32::from(chunk[0]) | u32::from(chunk[1]) << 7 | u32::from(chunk[2]) << 14)
        .collect()
}

fn alpha_header(header: u32, text: &[u32]) -> u32 {
    let sum = std::iter::once(header)
        .chain(text.iter().copied())
        .map(|word| (word & 0xFF) + (word >> 8 & 0xFF) + (word >> 16 & 0x1F))
        .sum::<u32>();
    header | !sum & 0x3FF
}

fn frame_words(page: &Page) -> [u32; WORDS] {
    let mut words = [0u32; WORDS];
    let text = text_words(&page.text);
    let message_start = 3;
    let message_len = text.len() + 1;
    words[0] = checksum(2 << 10);
    words[1] = page.address + CAPCODE_OFFSET;
    words[2] =
        checksum((ALPHA_VECTOR << 4) | (message_start as u32) << 7 | (message_len as u32) << 14);
    words[message_start] = alpha_header(3 << 11 | 1 << 19, &text);
    for (slot, word) in words[message_start + 1..].iter_mut().zip(text) {
        *slot = word;
    }
    words.map(encode_word)
}

fn numeric_code(digit: char) -> u32 {
    match digit {
        '0'..='9' => u32::from(digit) - u32::from('0'),
        ' ' => 10,
        'U' => 11,
        '-' => 13,
        ']' => 14,
        '[' => 15,
        _ => NUMERIC_FILL,
    }
}

fn numeric_words(text: &str) -> Vec<u32> {
    let mut bits = vec![false; NUMERIC_HEADER_BITS];
    for code in text.chars().map(numeric_code) {
        push_lsb(&mut bits, code, 4);
    }
    while !bits.len().is_multiple_of(WORD_BITS) {
        push_lsb(&mut bits, NUMERIC_FILL, 4);
    }
    bits.chunks(WORD_BITS)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0u32, |word, (bit, &set)| word | u32::from(set) << bit)
        })
        .collect()
}

fn numeric_frame_words(page: &Page) -> [u32; WORDS] {
    let mut words = [0u32; WORDS];
    let message = numeric_words(&page.text);
    let message_start = 3;
    let message_len = message.len() as u32;
    words[0] = checksum(2 << 10);
    words[1] = page.address + CAPCODE_OFFSET;
    words[2] =
        checksum((NUMERIC_VECTOR << 4) | (message_start as u32) << 7 | (message_len - 1) << 14);
    for (slot, word) in words[message_start..].iter_mut().zip(message) {
        *slot = word;
    }
    words.map(encode_word)
}

fn push_msb(bits: &mut Vec<bool>, value: u64, count: usize) {
    bits.extend((0..count).rev().map(|bit| value >> bit & 1 == 1));
}

fn push_lsb(bits: &mut Vec<bool>, value: u32, count: usize) {
    bits.extend((0..count).map(|bit| value >> bit & 1 == 1));
}

fn sync_bits(mode: Mode) -> Vec<bool> {
    let sync_code = mode.sync();
    let sync = u64::from(sync_code) << 48 | u64::from(SYNC_MARKER) << 16 | u64::from(!sync_code);
    let mut bits = (0..960).map(|index| index % 2 == 0).collect::<Vec<_>>();
    push_msb(&mut bits, sync, 64);
    bits.extend((0..16).map(|index| index % 2 == 0));
    bits
}

fn fiw_bits(cycle: u8, frame: u8) -> Vec<bool> {
    let fiw = encode_word(checksum(u32::from(cycle) << 4 | u32::from(frame) << 8));
    let mut bits = Vec::with_capacity(32);
    push_lsb(&mut bits, fiw, 32);
    bits
}

fn data_tone(bit: bool) -> f64 {
    if bit { 4_800.0 } else { -4_800.0 }
}

fn phase_bits(words: &[u32; WORDS]) -> Vec<bool> {
    let mut bits = Vec::with_capacity(WORDS * 32);
    for block in 0..11 {
        for bit in 0..32 {
            for word in 0..8 {
                bits.push(words[block * 8 + word] >> bit & 1 == 1);
            }
        }
    }
    bits
}

fn data_symbols(words: &[u32; WORDS], mode: Mode) -> Vec<f64> {
    let active = phase_bits(words);
    let idle = phase_bits(&[encode_word(0); WORDS]);
    let phases = [&active, &idle, &idle, &idle];
    let mut frequencies = Vec::with_capacity(mode.symbol_rate() * 176 / 100);
    let pair = |left: bool, right: bool| match (left, right) {
        (true, false) => 4_800.0,
        (true, true) => 1_600.0,
        (false, true) => -1_600.0,
        (false, false) => -4_800.0,
    };
    match (mode.symbol_rate(), mode.levels()) {
        (1_600, 2) => frequencies.extend(active.iter().map(|bit| data_tone(*bit))),
        (1_600, 4) => frequencies.extend(
            phases[0]
                .iter()
                .zip(phases[1])
                .map(|(left, right)| pair(*left, *right)),
        ),
        (3_200, 2) => {
            for (&phase_a, &phase_c) in phases[0].iter().zip(phases[2]) {
                frequencies.push(data_tone(phase_a));
                frequencies.push(data_tone(phase_c));
            }
        }
        (3_200, 4) => {
            for ((&phase_a, &phase_b), (&phase_c, &phase_d)) in phases[0]
                .iter()
                .zip(phases[1])
                .zip(phases[2].iter().zip(phases[3]))
            {
                frequencies.push(pair(phase_a, phase_b));
                frequencies.push(pair(phase_c, phase_d));
            }
        }
        _ => unreachable!(),
    }
    frequencies
}

fn append_symbols(
    iq: &mut Vec<Complex<f32>>,
    phase: &mut f64,
    frequencies: &[f64],
    sps: usize,
    rate: f64,
) {
    for &frequency in frequencies {
        let step = TAU * frequency / rate;
        for _ in 0..sps {
            *phase += step;
            iq.push(Complex::new(phase.cos() as f32, phase.sin() as f32));
        }
    }
}

fn air(words: &[u32; WORDS], cycle: u8, frame: u8, rate: f64, mode: Mode) -> Vec<Complex<f32>> {
    let header = sync_bits(mode)
        .into_iter()
        .map(|bit| -data_tone(bit))
        .chain(fiw_bits(cycle, frame).into_iter().map(data_tone))
        .collect::<Vec<_>>();
    let sync2 = (0..mode.symbol_rate() / 40)
        .map(|index| if index % 2 == 0 { -4_800.0 } else { 4_800.0 })
        .collect::<Vec<_>>();
    let data = data_symbols(words, mode);
    let tail = (0..mode.symbol_rate() * 6 / 100)
        .map(|index| if index % 2 == 0 { -4_800.0 } else { 4_800.0 })
        .collect::<Vec<_>>();
    let mut iq = Vec::new();
    let mut phase = 0.0;
    append_symbols(
        &mut iq,
        &mut phase,
        &header,
        (rate / 1_600.0) as usize,
        rate,
    );
    let data_sps = (rate / mode.symbol_rate() as f64) as usize;
    append_symbols(&mut iq, &mut phase, &sync2, data_sps, rate);
    append_symbols(&mut iq, &mut phase, &data, data_sps, rate);
    append_symbols(&mut iq, &mut phase, &tail, data_sps, rate);
    iq
}

#[must_use]
pub fn transmission_mode(
    page: &Page,
    cycle: u8,
    frame: u8,
    rate: f64,
    mode: Mode,
) -> Vec<Complex<f32>> {
    air(&frame_words(page), cycle, frame, rate, mode)
}

#[must_use]
pub fn transmission(page: &Page, cycle: u8, frame: u8, rate: f64) -> Vec<Complex<f32>> {
    transmission_mode(page, cycle, frame, rate, Mode::Flex1600_2)
}

#[must_use]
pub fn numeric_transmission(page: &Page, cycle: u8, frame: u8, rate: f64) -> Vec<Complex<f32>> {
    air(
        &numeric_frame_words(page),
        cycle,
        frame,
        rate,
        Mode::Flex1600_2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_words_survive_the_interleaver() {
        let page = Page {
            address: 123_456,
            text: "FLEX TEST".to_owned(),
        };
        for mode in [
            Mode::Flex1600_2,
            Mode::Flex1600_4,
            Mode::Flex3200_2,
            Mode::Flex3200_4,
        ] {
            assert_eq!(
                data_symbols(&frame_words(&page), mode).len(),
                mode.symbol_rate() * 176 / 100
            );
        }
    }

    #[test]
    fn every_mode_takes_its_sync_word_from_the_decoder_table() {
        for (mode, symbol_rate, levels) in [
            (Mode::Flex1600_2, 1_600, 2),
            (Mode::Flex1600_4, 1_600, 4),
            (Mode::Flex3200_2, 3_200, 2),
            (Mode::Flex3200_4, 3_200, 4),
        ] {
            let entry = SYNC_CODES[mode.sync_code()];
            assert_eq!(entry.0, mode.sync());
            assert_eq!(usize::from(entry.1.symbol_rate), symbol_rate);
            assert_eq!(usize::from(entry.1.levels), levels);
        }
    }

    #[test]
    fn numeric_digits_pack_four_bits_after_the_header() {
        let words = numeric_words("19");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0] & 0x3F, 0);
        assert_eq!(words[0] >> NUMERIC_HEADER_BITS & 0xF, 1);
        assert_eq!(words[0] >> (NUMERIC_HEADER_BITS + 4) & 0xF, 9);
        assert_eq!(words[0] >> (NUMERIC_HEADER_BITS + 8) & 0xF, NUMERIC_FILL);
    }
}
