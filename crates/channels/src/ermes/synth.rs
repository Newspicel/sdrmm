use crate::ermes::BCH;
use num_complex::Complex;
use sdrmm_modem::synth::{NARROW_HZ, Render, Signal};
use sdrmm_modem::{
    cpm::{CpmMod, CpmParams, Mapping},
    pulse::{self, Norm},
};

const PREAMBLE: u32 = 0b00_10_00_10_00_10_00_10_00_10_00_10_00_10_00;
const SYNC: u32 = 0b10_00_10_10_00_10_00_00_10_10_00_00_10_10_10;
const APT: u32 = 0b10_01_00_11_10_00_01_10_00_10_00_11_10_01_00;
const DELIMITER: u32 = 0b11_01_01_01_11_10_01_11_11_10_11_10_11_10_11;

#[derive(Clone, Debug)]
pub struct Page {
    pub local_address: u32,
    pub message_number: u8,
    pub text: String,
    pub urgent: bool,
    pub alert: u8,
}

fn push_word(bits: &mut Vec<bool>, word: u32) {
    bits.extend((0..30).rev().map(|bit| word >> bit & 1 == 1));
}

fn payload_words(text: &str) -> Vec<u32> {
    let mut bits = Vec::new();
    for byte in text.bytes().filter(u8::is_ascii) {
        bits.extend((0..7).rev().map(|bit| byte >> bit & 1 == 1));
    }
    bits.extend((0..7).rev().map(|bit| 0x11 >> bit & 1 == 1));
    while !bits.len().is_multiple_of(18) {
        let position = bits.len() % 7;
        bits.push(0x11 >> (6 - position) & 1 == 1);
    }
    bits.as_chunks::<18>()
        .0
        .iter()
        .map(|chunk| {
            chunk
                .iter()
                .fold(0u32, |word, bit| word << 1 | u32::from(*bit))
        })
        .collect()
}

fn message_words(page: &Page) -> Vec<u32> {
    let vif = 2 << 4 | u8::from(page.urgent) << 3 | page.alert & 7;
    let header = u64::from(page.local_address & 0x3F_FFFF) << 14
        | u64::from(page.message_number & 0x1F) << 9
        | u64::from(vif);
    let mut words = vec![(header >> 18) as u32, header as u32 & 0x3_FFFF];
    words.extend(payload_words(&page.text));
    words
}

#[must_use]
pub fn bits(page: &Page) -> Vec<bool> {
    let mut bits = Vec::new();
    for _ in 0..8 {
        push_word(&mut bits, PREAMBLE);
    }
    push_word(&mut bits, SYNC);
    for word in [0x12_345, 0x2_AAAA, 0x1_5555] {
        push_word(&mut bits, BCH.encode(word));
    }
    for _ in 0..5 {
        push_word(&mut bits, APT);
    }
    let mut words = vec![DELIMITER];
    words.extend(message_words(page).into_iter().map(|word| BCH.encode(word)));
    words.push(DELIMITER);
    let padded = words.len().next_multiple_of(9);
    words.resize(padded, DELIMITER);
    for block in words.as_chunks::<9>().0 {
        for bit in (0..30).rev() {
            for &word in block {
                bits.push(word >> bit & 1 == 1);
            }
        }
    }
    for _ in 0..4 {
        push_word(&mut bits, PREAMBLE);
    }
    bits
}

#[must_use]
pub fn transmission(page: &Page, rate: f64) -> Vec<Complex<f32>> {
    let bits = bits(page);
    let symbols = bits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from(pair[0]) << 1 | u8::from(pair[1]))
        .collect::<Vec<_>>();
    let mapping = Mapping::new(vec![-3.0, -1.0, 3.0, 1.0]);
    let sps = rate / 3_125.0;
    let params =
        CpmParams::from_deviation(mapping, 4_687.5, 3_125.0, pulse::rect(sps, Norm::Area), sps);
    let mut modulator = CpmMod::new(params);
    let mut iq = Vec::new();
    modulator.modulate(&symbols, &mut iq);
    modulator.flush(&mut iq);
    iq
}

/// The signals a generator offers for this decoder.
pub(crate) static SIGNALS: &[Signal] = &[Signal {
    id: "ermes",
    label: "ERMES",
    rate_hz: NARROW_HZ,
    render: Render::Always(ermes),
}];

fn ermes() -> Vec<Complex<f32>> {
    let page = Page {
        local_address: 4_242,
        message_number: 1,
        text: "SDRmm signal generator".to_owned(),
        urgent: false,
        alert: 1,
    };
    transmission(&page, NARROW_HZ)
}

/// The reference captures the fixtures are written from.
pub(crate) static RECIPES: &[sdrmm_modem::synth::Recipe] = &[sdrmm_modem::synth::Recipe {
    stem: "ermes_alpha_240k",
    rate: sdrmm_modem::synth::NARROW_HZ,
    note: "ermes channel at -30 kHz -> address 234567 \"SDRmm ERMES FIXTURE\", urgent alert 5",
    iq: || Ok(fixture_page()),
}];

fn fixture_page() -> Vec<Complex<f32>> {
    let page = Page {
        local_address: 234_567,
        message_number: 3,
        text: "SDRmm ERMES FIXTURE".to_string(),
        urgent: true,
        alert: 5,
    };
    sdrmm_modem::synth::shifted(
        transmission(&page, sdrmm_modem::synth::NARROW_HZ),
        -30_000.0,
        sdrmm_modem::synth::NARROW_HZ,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_is_symbol_aligned() {
        let page = Page {
            local_address: 123_456,
            message_number: 3,
            text: "ERMES TEST".to_owned(),
            urgent: true,
            alert: 5,
        };
        assert!(bits(&page).len().is_multiple_of(2));
    }
}
