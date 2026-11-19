//! The K=32, rate 1/2 Layland-Lushbaugh convolutional code WSPR and JT9 decode sequentially, with
//! the bit-reversal interleaver both spread its output with.

use sdrmm_dsp::coding::fec::fano::FanoCode;

/// The two generator polynomials, newest bit lowest.
pub const POLYNOMIALS: [u32; 2] = [0xf2d0_5351, 0xe461_3c47];

/// Zero bits that flush the register after the data.
pub const TAIL_BITS: usize = 31;

/// The code over `data` bits, flushed by [`TAIL_BITS`].
#[must_use]
pub const fn code(data: usize) -> FanoCode<'static> {
    FanoCode {
        polynomials: &POLYNOMIALS,
        data,
        tail: TAIL_BITS,
    }
}

/// Where each of the first `out.len()` coded bits goes on the channel: the 8-bit reversals of
/// 0..=255 in order, skipping those past the end.
pub fn bit_reversal(out: &mut [usize]) {
    let mut next = 0;
    for index in 0..=255u8 {
        let reversed = usize::from(index.reverse_bits());
        if reversed < out.len() && next < out.len() {
            out[next] = reversed;
            next += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_position_is_used_once() {
        for length in [162, 206] {
            let mut order = vec![0; length];
            bit_reversal(&mut order);
            let mut seen = vec![false; length];
            for position in order {
                assert!(!seen[position]);
                seen[position] = true;
            }
        }
    }

    #[test]
    fn the_first_positions_are_bit_reversed_counts() {
        let mut order = [0usize; 162];
        bit_reversal(&mut order);
        assert_eq!(order[..6], [0, 128, 64, 32, 160, 96]);
    }

    #[test]
    fn a_zero_message_encodes_to_zeros() {
        let mut out = [1u8; 206];
        code(72).encode(std::iter::repeat_n(false, 72), &mut out);
        assert!(out.iter().all(|&bit| bit == 0));
    }
}
