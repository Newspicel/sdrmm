use std::f32::consts::{FRAC_PI_2, PI};

use num_complex::Complex;
use sdrmm_wire::RemoteIdPhy;

use super::{CHIP_SAMPLES, SymbolReader, plcp::Header, quadrant};

pub(crate) const CHIPS: usize = 8;

fn qpsk(high: bool, low: bool) -> f32 {
    match (high, low) {
        (false, false) => 0.0,
        (false, true) => FRAC_PI_2,
        (true, false) => PI,
        (true, true) => 3.0 * FRAC_PI_2,
    }
}

pub(crate) fn phases(phy: RemoteIdPhy, label: u8) -> [f32; 3] {
    let bit = |index: u8| label >> index & 1 == 1;
    if phy == RemoteIdPhy::Cck11m {
        [
            qpsk(bit(0), bit(1)),
            qpsk(bit(2), bit(3)),
            qpsk(bit(4), bit(5)),
        ]
    } else {
        [
            if bit(0) { PI } else { 0.0 } + FRAC_PI_2,
            0.0,
            if bit(1) { PI } else { 0.0 },
        ]
    }
}

pub(crate) fn codeword(phi1: f32, [phi2, phi3, phi4]: [f32; 3]) -> [Complex<f32>; CHIPS] {
    let chip = |phase: f32, sign: f32| Complex::from_polar(sign, phase);
    [
        chip(phi1 + phi2 + phi3 + phi4, 1.0),
        chip(phi1 + phi3 + phi4, 1.0),
        chip(phi1 + phi2 + phi4, 1.0),
        chip(phi1 + phi4, -1.0),
        chip(phi1 + phi2 + phi3, 1.0),
        chip(phi1 + phi3, 1.0),
        chip(phi1 + phi2, -1.0),
        chip(phi1, 1.0),
    ]
}

pub(crate) fn label_bits(phy: RemoteIdPhy) -> u8 {
    if phy == RemoteIdPhy::Cck11m { 6 } else { 2 }
}

fn interpolate(history: &[Complex<f32>], position: f64) -> Option<Complex<f32>> {
    let index = position.floor();
    let weight = (position - index) as f32;
    let index = index as usize;
    Some(history.get(index)? * (1.0 - weight) + history.get(index + 1)? * weight)
}

pub(crate) fn psdu(reader: &mut SymbolReader<'_>, header: &Header) -> Option<Vec<u8>> {
    let label_bits = label_bits(header.phy);
    let bits_per_symbol = usize::from(label_bits) + 2;
    let symbols = (header.bytes * 8).div_ceil(bits_per_symbol);
    let start = reader.chip_start();
    let history = reader.history();
    let end = start + (symbols * CHIPS) as f64 * CHIP_SAMPLES;
    if end as usize + 2 > history.len() {
        return None;
    }
    let codebook: Vec<[Complex<f32>; CHIPS]> = (0..1u8 << label_bits)
        .map(|label| codeword(0.0, phases(header.phy, label)))
        .collect();
    let rotation = reader.rotation().powf(CHIPS as f32 / 11.0);
    let mut previous = reader.last_phase();
    let mut bits = Vec::with_capacity(symbols * bits_per_symbol);
    for symbol in 0..symbols {
        let chips: [Complex<f32>; CHIPS] = std::array::from_fn(|chip| {
            let position = start + ((symbol * CHIPS + chip) as f64 + 0.5) * CHIP_SAMPLES - 0.5;
            interpolate(history, position).unwrap_or_default()
        });
        let (label, z) = codebook
            .iter()
            .enumerate()
            .map(|(label, code)| {
                let z: Complex<f32> = chips.iter().zip(code).map(|(r, c)| r * c.conj()).sum();
                (label as u8, z)
            })
            .max_by(|a, b| a.1.norm_sqr().total_cmp(&b.1.norm_sqr()))?;
        let mut differential = z * previous.conj() * rotation;
        if symbol % 2 == 1 {
            differential = -differential;
        }
        previous = z;
        bits.extend(quadrant(differential));
        bits.extend((0..label_bits).map(|bit| label >> bit & 1 == 1));
    }
    reader.advance_to(end);
    let descrambled: Vec<bool> = bits
        .into_iter()
        .map(|bit| reader.descrambled(bit))
        .collect();
    Some(
        descrambled
            .as_chunks::<8>()
            .0
            .iter()
            .take(header.bytes)
            .map(|byte| {
                byte.iter()
                    .enumerate()
                    .fold(0u8, |acc, (bit, &value)| acc | u8::from(value) << bit)
            })
            .collect(),
    )
}
