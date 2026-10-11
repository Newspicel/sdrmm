use sdrmm_dsp::{Prbs, PrbsSpec, crc16_msb_bits};

pub(crate) const SYNC: u64 = 0x1ACF_FC1D;
pub(crate) const SYNC_BITS: u32 = 32;
pub(crate) const SEQUENCE_BITS: usize = 32;
pub(crate) const CRC_BITS: usize = 16;
pub(crate) const PREAMBLE_BITS: usize = 32;
pub(crate) const TAIL_BITS: usize = 8;
const CRC_POLY: u16 = 0x1021;
const CRC_INIT: u16 = 0xFFFF;
const PAYLOAD: PrbsSpec = PrbsSpec {
    stages: 9,
    taps: 0x110,
    seed: 0x1FF,
};

#[must_use]
pub(crate) fn body_bits(payload_bytes: u16) -> usize {
    SEQUENCE_BITS + 8 * usize::from(payload_bytes) + CRC_BITS
}

#[must_use]
pub(crate) fn payload(payload_bytes: u16) -> Vec<bool> {
    let mut prbs = Prbs::new(PAYLOAD);
    (0..8 * usize::from(payload_bytes))
        .map(|_| prbs.next_bit())
        .collect()
}

fn crc(bits: &[bool]) -> u16 {
    crc16_msb_bits(CRC_POLY, CRC_INIT, bits)
}

fn word(bits: &[bool]) -> u32 {
    bits.iter()
        .fold(0u32, |word, &bit| (word << 1) | u32::from(bit))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Checked {
    pub sequence: Option<u32>,
    pub bit_errors: u32,
    pub bits: u32,
}

#[must_use]
pub(crate) fn check(body: &[bool], expected: &[bool]) -> Checked {
    let protected = SEQUENCE_BITS + expected.len();
    let (data, sent_crc) = body.split_at(protected);
    let received = &data[SEQUENCE_BITS..];
    let bit_errors = received
        .iter()
        .zip(expected)
        .filter(|(got, want)| got != want)
        .count() as u32;
    Checked {
        sequence: (crc(data) == word(sent_crc) as u16).then(|| word(&data[..SEQUENCE_BITS])),
        bit_errors,
        bits: expected.len() as u32,
    }
}

#[cfg(any(test, feature = "synth"))]
#[must_use]
pub fn frame_bits(sequence: u32, payload_bytes: u16) -> Vec<bool> {
    let alternating = |len: usize| (0..len).map(|k| k % 2 == 0);
    let mut data: Vec<bool> = (0..SEQUENCE_BITS)
        .rev()
        .map(|bit| sequence >> bit & 1 == 1)
        .collect();
    data.extend(payload(payload_bytes));
    let crc = crc(&data);
    let mut bits: Vec<bool> = alternating(PREAMBLE_BITS).collect();
    bits.extend((0..SYNC_BITS).rev().map(|bit| SYNC >> bit & 1 == 1));
    bits.extend(data);
    bits.extend((0..CRC_BITS).rev().map(|bit| crc >> bit & 1 == 1));
    bits.extend(alternating(TAIL_BITS));
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(sequence: u32, payload_bytes: u16) -> Vec<bool> {
        let bits = frame_bits(sequence, payload_bytes);
        let start = PREAMBLE_BITS + SYNC_BITS as usize;
        bits[start..start + body_bits(payload_bytes)].to_vec()
    }

    #[test]
    fn a_clean_frame_checks_out_with_its_sequence() {
        let expected = payload(32);
        let checked = check(&body(0xDEAD_BEEF, 32), &expected);
        assert_eq!(
            checked,
            Checked {
                sequence: Some(0xDEAD_BEEF),
                bit_errors: 0,
                bits: 256
            }
        );
    }

    #[test]
    fn a_flipped_payload_bit_fails_the_crc_and_is_counted() {
        let expected = payload(8);
        let mut bits = body(7, 8);
        bits[SEQUENCE_BITS + 3] ^= true;
        bits[SEQUENCE_BITS + 40] ^= true;
        let checked = check(&bits, &expected);
        assert_eq!(checked.sequence, None);
        assert_eq!(checked.bit_errors, 2);
    }

    #[test]
    fn the_payload_is_prbs9_and_the_frame_has_the_stated_length() {
        let bits = payload(80);
        let period = (1 << 9) - 1;
        assert_eq!(bits[..64], bits[period..period + 64]);
        assert_eq!(
            frame_bits(1, 32).len(),
            PREAMBLE_BITS + SYNC_BITS as usize + body_bits(32) + TAIL_BITS
        );
    }
}
