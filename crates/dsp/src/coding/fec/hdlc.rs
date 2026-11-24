use super::crc::crc16_x25;

const FCS_BYTES: usize = 2;
const MIN_FRAME_BYTES: usize = FCS_BYTES + 1;
/// Bits a repair may flip, the least reliable of the frame. Every subset of them is tried, so
/// a CRC-16 passes a wrong frame about once in 64 failed ones.
pub const REPAIR_BITS: usize = 10;

#[must_use]
pub fn hdlc_fcs_ok(frame: &[u8]) -> bool {
    frame.len() >= MIN_FRAME_BYTES && fcs_syndrome(frame) == 0
}

fn fcs_syndrome(frame: &[u8]) -> u32 {
    let (payload, fcs) = frame.split_at(frame.len() - FCS_BYTES);
    let [lo, hi] = fcs else { return u32::MAX };
    u32::from(crc16_x25(payload) ^ u16::from_le_bytes([*lo, *hi]))
}

/// [`crc_repair`] against the HDLC FCS.
pub fn hdlc_repair(frame: &mut [u8], reliability: &[f32]) -> bool {
    frame.len() >= MIN_FRAME_BYTES && crc_repair(frame, reliability, fcs_syndrome)
}

/// Chase-style repair against a `syndrome` that is affine in the frame's bits and zero when
/// the frame is whole: every pattern of flips over the [`REPAIR_BITS`] least reliable bits is
/// tried, and the one that checks flipping the least reliability is kept. Bit `i` is bit
/// `i % 8` of byte `i / 8`, the order HDLC and ACARS send them, and `reliability[i]` its
/// magnitude. `false`, the frame untouched, when no pattern checks.
pub fn crc_repair(frame: &mut [u8], reliability: &[f32], syndrome: impl Fn(&[u8]) -> u32) -> bool {
    let start = syndrome(frame);
    if start == 0 {
        return true;
    }
    let reliability = &reliability[..reliability.len().min(frame.len() * 8)];
    let (weak, count) = weakest(reliability);
    let weak = &weak[..count];
    let mut deltas = [0u32; REPAIR_BITS];
    for (delta, &bit) in deltas.iter_mut().zip(weak) {
        flip(frame, bit);
        *delta = syndrome(frame) ^ start;
        flip(frame, bit);
    }
    let Some(pattern) = cheapest_pattern(start, &deltas[..count], |pattern| {
        flipped(weak, pattern).map(|bit| reliability[bit]).sum()
    }) else {
        return false;
    };
    flipped(weak, pattern).for_each(|bit| flip(frame, bit));
    true
}

/// Walks every pattern in Gray order, one syndrome XOR a step, and returns the cheapest that
/// clears `start`.
fn cheapest_pattern(start: u32, deltas: &[u32], cost: impl Fn(u32) -> f32) -> Option<u32> {
    let (mut syndrome, mut pattern) = (start, 0u32);
    let mut best: Option<(f32, u32)> = None;
    for step in 1..1u32 << deltas.len() {
        let changed = step.trailing_zeros() as usize;
        pattern ^= 1 << changed;
        syndrome ^= deltas[changed];
        if syndrome == 0 {
            let spent = cost(pattern);
            if best.is_none_or(|(least, _)| spent < least) {
                best = Some((spent, pattern));
            }
        }
    }
    best.map(|(_, pattern)| pattern)
}

fn flipped(weak: &[usize], pattern: u32) -> impl Iterator<Item = usize> + '_ {
    weak.iter()
        .enumerate()
        .filter(move |&(index, _)| pattern >> index & 1 == 1)
        .map(|(_, &bit)| bit)
}

fn flip(frame: &mut [u8], bit: usize) {
    frame[bit / 8] ^= 1 << (bit % 8);
}

/// The positions of the [`REPAIR_BITS`] smallest values, the earlier on a tie.
fn weakest(reliability: &[f32]) -> ([usize; REPAIR_BITS], usize) {
    let mut weak = [0usize; REPAIR_BITS];
    let mut count = 0;
    for (bit, &value) in reliability.iter().enumerate() {
        if count == REPAIR_BITS && value >= reliability[weak[count - 1]] {
            continue;
        }
        let mut at = count.min(REPAIR_BITS - 1);
        count = (count + 1).min(REPAIR_BITS);
        while at > 0 && reliability[weak[at - 1]] > value {
            weak[at] = weak[at - 1];
            at -= 1;
        }
        weak[at] = bit;
    }
    (weak, count)
}

#[cfg(test)]
mod tests {
    use super::{super::test_data::flip_msb_first, *};

    fn framed() -> Vec<u8> {
        let mut frame = b"DL1ABC>APRS:hello world".to_vec();
        frame.extend_from_slice(&crc16_x25(&frame).to_le_bytes());
        frame
    }

    fn damaged(clean: &[u8], bits: &[usize]) -> (Vec<u8>, Vec<f32>) {
        let mut frame = clean.to_vec();
        let mut reliability = vec![1.0; frame.len() * 8];
        for (rank, &bit) in bits.iter().enumerate() {
            flip(&mut frame, bit);
            reliability[bit] = 0.1 + 0.01 * rank as f32;
        }
        (frame, reliability)
    }

    #[test]
    fn weak_wrong_bits_are_repaired_wherever_they_lie() {
        let clean = framed();
        let last = clean.len() * 8 - 1;
        for bits in [
            vec![0],
            vec![77, 78],
            vec![3, 90, 140, last],
            vec![5, 17, 29, 41, 53, 65],
        ] {
            let (mut frame, mut reliability) = damaged(&clean, &bits);
            reliability[100] = 0.05;
            reliability[101] = 0.3;
            assert!(hdlc_repair(&mut frame, &reliability), "{bits:?}");
            assert_eq!(frame, clean, "{bits:?}");
        }
    }

    #[test]
    fn confident_wrong_bits_are_left_alone() {
        let clean = framed();
        let (mut frame, mut reliability) = damaged(&clean, &[13, 77]);
        reliability[13] = 1.0;
        let before = frame.clone();
        assert!(!hdlc_repair(&mut frame, &reliability));
        assert_eq!(frame, before);
    }

    #[test]
    fn the_weakest_bits_are_picked_in_order() {
        let mut values = vec![1.0f32; 40];
        for (bit, value) in [(30, 0.3), (2, 0.1), (17, 0.2), (39, 0.0)] {
            values[bit] = value;
        }
        let (weak, count) = weakest(&values);
        assert_eq!(count, REPAIR_BITS);
        assert_eq!(&weak[..6], &[39, 2, 17, 30, 0, 1]);
        assert_eq!(weakest(&values[..3]), ([2, 0, 1, 0, 0, 0, 0, 0, 0, 0], 3));
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
        assert!(!hdlc_repair(&mut [], &[]));
        assert!(!hdlc_repair(&mut [0x00], &[1.0; 8]));
        assert!(!hdlc_repair(&mut [0x00, 0x00], &[1.0; 16]));
    }
}
