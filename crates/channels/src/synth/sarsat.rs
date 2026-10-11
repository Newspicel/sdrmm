use num_complex::Complex;
use sdrmm_wire::SarsatProtocol;

use super::silence;
use crate::sarsat::{
    BIT_RATE, BIT_SYNC_BITS, FRAME_SYNC, FRAME_SYNC_BITS, PHASE_DEVIATION, PREAMBLE_S,
    SELF_TEST_SYNC,
    message::{self, Bits},
};

const LEVEL: f32 = 0.5;
const GAP_S: f64 = 0.3;
const STANDARD_PDF2_FIXED: u128 = 0b1101;

#[derive(Clone, Copy, Debug)]
pub struct EpirbFix {
    pub country: u16,
    pub mmsi_tail: u32,
    pub lat: f64,
    pub lon: f64,
}

#[must_use]
pub fn epirb_location(fix: EpirbFix) -> Vec<bool> {
    let bits = Bits::default()
        .with(25, 25, 1)
        .with(27, 36, u128::from(fix.country))
        .with(37, 40, 0b0010)
        .with(41, 60, u128::from(fix.mmsi_tail))
        .with(107, 110, STANDARD_PDF2_FIXED)
        .with(111, 112, 0b11);
    let located =
        message::encode_position(bits, SarsatProtocol::StandardLocation, fix.lat, fix.lon);
    message::seal(located).sent(true).collect()
}

#[must_use]
pub fn from_hex(hex: &str) -> Option<Vec<bool>> {
    let bits = message::from_hex(hex)?;
    Some(bits.sent(hex.len() * 4 > message::SHORT_BITS).collect())
}

#[must_use]
pub fn frame_bits(message: &[bool], self_test: bool) -> Vec<bool> {
    let sync = if self_test {
        SELF_TEST_SYNC
    } else {
        FRAME_SYNC
    };
    let mut bits = vec![true; BIT_SYNC_BITS];
    bits.extend((0..FRAME_SYNC_BITS).rev().map(|k| sync >> k & 1 == 1));
    bits.extend_from_slice(message);
    bits
}

#[must_use]
pub fn chips(bits: &[bool]) -> Vec<bool> {
    bits.iter().flat_map(|&bit| [bit, !bit]).collect()
}

#[must_use]
pub fn burst(message: &[bool], self_test: bool, rate: f64) -> Vec<Complex<f32>> {
    let chips = chips(&frame_bits(message, self_test));
    let preamble = (PREAMBLE_S * rate) as usize;
    let chip_rate = 2.0 * BIT_RATE;
    let modulated = (chips.len() as f64 * rate / chip_rate) as usize;
    (0..preamble + modulated)
        .map(|k| {
            let phase = k.checked_sub(preamble).map_or(0.0, |at| {
                let chip = ((at as f64 * chip_rate / rate) as usize).min(chips.len() - 1);
                if chips[chip] {
                    PHASE_DEVIATION
                } else {
                    -PHASE_DEVIATION
                }
            });
            Complex::from_polar(LEVEL, phase as f32)
        })
        .collect()
}

#[must_use]
pub fn transmission(message: &[bool], self_test: bool, rate: f64) -> Vec<Complex<f32>> {
    let gap = (GAP_S * rate) as usize;
    let mut iq = silence(gap);
    iq.extend(burst(message, self_test, rate));
    iq.extend(silence(gap));
    iq
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_burst_lasts_the_published_520_ms() {
        let message = epirb_location(EpirbFix {
            country: 366,
            mmsi_tail: 123_456,
            lat: 1.0,
            lon: 2.0,
        });
        assert_eq!(message.len(), 120);
        assert_eq!(burst(&message, false, 24_000.0).len(), 12_480);
        let short = from_hex("56E6804002202009655250").expect("hex");
        assert_eq!(frame_bits(&short, true).len(), 112);
        assert_eq!(
            &frame_bits(&short, true)[15..24],
            &[false, true, true, false, true, false, false, false, false]
        );
    }
}
