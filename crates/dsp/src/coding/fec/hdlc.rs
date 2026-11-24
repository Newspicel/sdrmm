use super::crc::crc16_x25;

const FCS_BYTES: usize = 2;
const MIN_FRAME_BYTES: usize = FCS_BYTES + 1;
const MAX_REPAIR_BURST_BITS: usize = 2;

#[must_use]
pub fn hdlc_fcs_ok(frame: &[u8]) -> bool {
    if frame.len() < MIN_FRAME_BYTES {
        return false;
    }
    let (payload, fcs) = frame.split_at(frame.len() - FCS_BYTES);
    let [lo, hi] = fcs else { return false };
    crc16_x25(payload) == u16::from_le_bytes([*lo, *hi])
}

/// Flips every run of one or two adjacent bits, LSB first as HDLC sends them, until the FCS
/// checks.
pub fn hdlc_repair(frame: &mut [u8]) -> bool {
    if hdlc_fcs_ok(frame) {
        return true;
    }
    let bits = frame.len() * 8;
    let flip = |frame: &mut [u8], at: usize| frame[at / 8] ^= 1 << (at % 8);
    for span in 1..=MAX_REPAIR_BURST_BITS {
        for at in 0..bits.saturating_sub(span - 1) {
            (at..at + span).for_each(|b| flip(frame, b));
            if hdlc_fcs_ok(frame) {
                return true;
            }
            (at..at + span).for_each(|b| flip(frame, b));
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{super::test_data::flip_msb_first, *};

    #[test]
    fn one_bit_or_two_adjacent_bits_are_repaired_and_scattered_ones_are_not() {
        let mut frame = b"DL1ABC>APRS:hello world".to_vec();
        frame.extend_from_slice(&crc16_x25(&frame).to_le_bytes());
        let clean = frame.clone();
        for bit in [0usize, 13, 77, frame.len() * 8 - 1] {
            let mut damaged = clean.clone();
            damaged[bit / 8] ^= 1 << (bit % 8);
            assert!(hdlc_repair(&mut damaged), "bit {bit}");
            assert_eq!(damaged, clean, "bit {bit}");
            if bit + 1 < clean.len() * 8 {
                let mut pair = clean.clone();
                pair[bit / 8] ^= 1 << (bit % 8);
                pair[(bit + 1) / 8] ^= 1 << ((bit + 1) % 8);
                assert!(hdlc_repair(&mut pair), "pair at {bit}");
                assert_eq!(pair, clean, "pair at {bit}");
            }
        }
        let mut scattered = clean;
        scattered[2] ^= 0x11;
        assert!(!hdlc_repair(&mut scattered));
    }

    #[test]
    fn hdlc_fcs_round_trips_and_catches_a_bit_flip() {
        let payload = b"\x82\xa0\xa4\xa6@@`\x9c\x94n\xa0@@\xe1\x03\xf0hello";
        let mut frame = payload.to_vec();
        frame.extend_from_slice(&crc16_x25(payload).to_le_bytes());
        assert!(hdlc_fcs_ok(&frame));

        for bit in 0..frame.len() * 8 {
            let mut corrupt = frame.clone();
            flip_msb_first(&mut corrupt, bit);
            assert!(!hdlc_fcs_ok(&corrupt), "bit {bit} slipped past the FCS");
        }
    }

    #[test]
    fn hdlc_rejects_frames_too_short_to_hold_an_fcs() {
        assert!(!hdlc_fcs_ok(&[]));
        assert!(!hdlc_fcs_ok(&[0x00, 0x00]));
    }

    #[test]
    fn hdlc_repair_refuses_a_frame_with_no_bits_to_flip() {
        assert!(!hdlc_repair(&mut []));
        assert!(!hdlc_repair(&mut [0x00]));
        assert!(!hdlc_repair(&mut [0x00, 0x00]));
    }
}
