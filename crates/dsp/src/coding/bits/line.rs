#[derive(Clone, Debug, Default)]
pub struct NrziDecoder {
    last: bool,
}

impl NrziDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decode(&mut self, level: bool) -> bool {
        let bit = level == self.last;
        self.last = level;
        bit
    }

    pub fn reset(&mut self) {
        self.last = false;
    }
}

#[derive(Clone, Debug, Default)]
pub struct DifferentialDecoder {
    last: bool,
}

impl DifferentialDecoder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decode(&mut self, bit: bool) -> bool {
        let out = bit ^ self.last;
        self.last = bit;
        out
    }

    pub fn reset(&mut self) {
        self.last = false;
    }
}

#[must_use]
pub fn manchester_decode(first: bool, second: bool) -> Option<bool> {
    (first != second).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nrzi_encode(bits: &[bool]) -> Vec<bool> {
        let mut level = false;
        bits.iter()
            .map(|&b| {
                if !b {
                    level = !level;
                }
                level
            })
            .collect()
    }

    #[test]
    fn nrzi_decodes_hand_encoded_levels() {
        let levels = [false, true, true, true, false, true, true];
        let mut nrzi = NrziDecoder::new();
        let bits: Vec<bool> = levels.iter().map(|&l| nrzi.decode(l)).collect();
        assert_eq!(bits, [true, false, true, true, false, false, true]);
    }

    #[test]
    fn nrzi_round_trips_and_resets() {
        let data: Vec<bool> = (0..64)
            .map(|i: u32| i.count_ones().is_multiple_of(3))
            .collect();
        let mut nrzi = NrziDecoder::new();
        let decoded: Vec<bool> = nrzi_encode(&data).iter().map(|&l| nrzi.decode(l)).collect();
        assert_eq!(decoded, data);

        nrzi.reset();
        assert!(nrzi.decode(false), "reset must restore the low idle level");
    }

    #[test]
    fn differential_decodes_against_previous_bit() {
        let mut diff = DifferentialDecoder::new();
        let input = [true, true, false, false, true];
        let out: Vec<bool> = input.iter().map(|&b| diff.decode(b)).collect();
        assert_eq!(out, [true, false, true, false, true]);

        diff.reset();
        assert!(diff.decode(true));
    }

    #[test]
    fn manchester_needs_a_mid_symbol_transition() {
        assert_eq!(manchester_decode(true, false), Some(true));
        assert_eq!(manchester_decode(false, true), Some(false));
        assert_eq!(manchester_decode(true, true), None);
        assert_eq!(manchester_decode(false, false), None);
    }
}
