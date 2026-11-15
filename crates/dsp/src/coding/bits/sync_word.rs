#[derive(Clone, Debug)]
pub struct SyncDetector {
    reg: u64,
    word: u64,
    mask: u64,
    tolerance: u32,
}

impl SyncDetector {
    #[must_use]
    pub fn new(word: u64, bits: u32, tolerance: u32) -> Self {
        assert!((1..=64).contains(&bits), "sync word must be 1..=64 bits");
        assert!(tolerance <= bits, "tolerance exceeds sync word length");
        let mask = if bits == 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        };
        Self {
            reg: 0,
            word: word & mask,
            mask,
            tolerance,
        }
    }

    pub fn push(&mut self, bit: bool) -> bool {
        self.reg = ((self.reg << 1) | u64::from(bit)) & self.mask;
        hamming_distance(self.reg, self.word) <= self.tolerance
    }

    #[must_use]
    pub fn register(&self) -> u64 {
        self.reg
    }

    pub fn reset(&mut self) {
        self.reg = 0;
    }
}

#[must_use]
pub fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sync_detector_matches_within_tolerance() {
        const WORD: u64 = 0xabcd;
        let bits: Vec<bool> = (0..16).rev().map(|i| WORD >> i & 1 == 1).collect();

        for (errors, expect) in [(0usize, true), (1, true), (2, false)] {
            let mut detector = SyncDetector::new(WORD, 16, 1);
            let mut corrupted = bits.clone();
            for c in corrupted.iter_mut().take(errors) {
                *c = !*c;
            }
            let hits = corrupted.iter().filter(|&&b| detector.push(b)).count();
            assert_eq!(hits > 0, expect, "{errors} bit errors");
        }
    }

    #[test]
    fn sync_detector_slides_over_leading_noise() {
        const WORD: u64 = 0x7e;
        let mut detector = SyncDetector::new(WORD, 8, 0);
        let noise = [true, false, true, false, false];
        assert!(!noise.iter().any(|&b| detector.push(b)));
        let word: Vec<bool> = (0..8).rev().map(|i| WORD >> i & 1 == 1).collect();
        let hit_at = word.iter().position(|&b| detector.push(b));
        assert_eq!(
            hit_at,
            Some(7),
            "match only once the word is fully shifted in"
        );
        assert_eq!(detector.register(), WORD);

        detector.reset();
        assert_eq!(detector.register(), 0);
    }

    #[test]
    fn hamming_distance_counts_differing_bits() {
        assert_eq!(hamming_distance(0, 0), 0);
        assert_eq!(hamming_distance(0b1011, 0b0001), 2);
        assert_eq!(hamming_distance(u64::MAX, 0), 64);
    }
}
