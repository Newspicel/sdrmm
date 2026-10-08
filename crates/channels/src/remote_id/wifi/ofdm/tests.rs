use super::{byte, descramble, tables::RATES};
use crate::synth::remote_id::ofdm;

#[test]
fn the_service_field_seeds_the_descrambler() {
    let mut register = 0b101_1101u8;
    let data: Vec<bool> = (0..64).map(|k| k % 3 == 0 && k >= 16).collect();
    let scrambled: Vec<bool> = data
        .iter()
        .map(|&bit| {
            let next = (register >> 6 ^ register >> 3) & 1;
            register = (register << 1 | next) & 0x7F;
            bit ^ (next == 1)
        })
        .collect();
    assert_eq!(descramble(&scrambled), data);
}

#[test]
fn bytes_are_least_significant_bit_first() {
    assert_eq!(
        byte(&[true, false, false, false, false, false, false, true]),
        0x81
    );
}

#[test]
fn a_frame_lasts_its_preamble_signal_and_data_symbols() {
    let mpdu = vec![0x80u8; 100];
    let iq = ofdm(&mpdu, 6);
    assert_eq!(iq.len(), 320 + 80 * (1 + RATES[0].symbols(100)) + 16);
}
