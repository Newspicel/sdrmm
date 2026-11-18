use sdrmm_dsp::{Prbs, PrbsSpec};

/// 1 + x + x^15 from the seed 110100101011001, first stage first.
pub const SCRAMBLER: PrbsSpec = PrbsSpec {
    stages: 15,
    taps: 0x4001,
    seed: 0x4D4B,
};

pub fn scrambler() -> Prbs {
    Prbs::new(SCRAMBLER)
}

#[cfg(test)]
mod tests {
    use super::scrambler;

    #[test]
    fn first_three_bits_keep_reserved_symbol_zero() {
        let mut s = scrambler();
        assert!((0..3).all(|_| !s.next_bit()));
    }

    #[test]
    fn keystream_matches_spec_derivation() {
        let expected = "000100110001101111000100001001010000111110001100";
        let mut s = scrambler();
        let got: String = (0..48)
            .map(|_| if s.next_bit() { '1' } else { '0' })
            .collect();
        assert_eq!(got, expected);
    }
}
