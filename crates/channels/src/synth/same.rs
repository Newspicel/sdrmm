use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_wire::{SAME_BAUD, SAME_BURSTS, SAME_MARK_HZ, SAME_SPACE_HZ};

use super::{fm_modulate, silence};
use crate::same::{
    DEVIATION_HZ,
    burst::{PREAMBLE_BYTE, PREAMBLE_BYTES},
};

const LEVEL: f32 = 0.8;
const GAP_S: f64 = 0.5;

#[must_use]
pub fn bits(header: &str) -> Vec<bool> {
    let bytes = std::iter::repeat_n(PREAMBLE_BYTE, PREAMBLE_BYTES).chain(header.bytes());
    bytes
        .flat_map(|byte| (0..8).map(move |bit| byte >> bit & 1 == 1))
        .collect()
}

#[must_use]
pub fn afsk(bits: &[bool], rate: f64) -> Vec<f32> {
    let len = (bits.len() as f64 * rate / SAME_BAUD) as usize;
    let mut phase = 0.0f64;
    (0..len)
        .map(|k| {
            let at = ((k as f64 * SAME_BAUD / rate) as usize).min(bits.len().saturating_sub(1));
            let tone = if bits.get(at).copied().unwrap_or(false) {
                SAME_MARK_HZ
            } else {
                SAME_SPACE_HZ
            };
            phase = (phase + TAU * tone / rate).rem_euclid(TAU);
            LEVEL * phase.sin() as f32
        })
        .collect()
}

#[must_use]
pub fn burst(header: &str, rate: f64) -> Vec<Complex<f32>> {
    fm_modulate(&afsk(&bits(header), rate), DEVIATION_HZ, rate)
}

#[must_use]
pub fn transmission(header: &str, copies: u8, rate: f64) -> Vec<Complex<f32>> {
    let gap = (GAP_S * rate) as usize;
    let mut iq = silence(gap);
    for _ in 0..copies {
        iq.extend(burst(header, rate));
        iq.extend(silence(gap));
    }
    iq
}

#[must_use]
pub fn alert(header: &str, rate: f64) -> Vec<Complex<f32>> {
    transmission(header, SAME_BURSTS, rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_burst_carries_the_preamble_and_every_character() {
        let header = "ZCZC-WXR-TOR-048201+0030-1232200-KEC55/NW-";
        let bits = bits(header);
        assert_eq!(bits.len(), (PREAMBLE_BYTES + header.len()) * 8);
        assert_eq!(
            &bits[..8],
            &[true, true, false, true, false, true, false, true]
        );
    }

    #[test]
    fn a_bit_lasts_an_exact_number_of_tone_cycles() {
        let rate = 12_500.0;
        assert_eq!(afsk(&[true], rate).len(), 24);
        assert_eq!(SAME_MARK_HZ / SAME_BAUD, 4.0);
        assert_eq!(SAME_SPACE_HZ / SAME_BAUD, 3.0);
    }
}
