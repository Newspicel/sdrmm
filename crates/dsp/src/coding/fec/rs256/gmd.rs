use super::{ReedSolomon, SIZE};

impl ReedSolomon {
    /// Forney's generalized minimum distance decoding, `reliability` one value a symbol, higher
    /// for surer: errors only first, then the least reliable two, four and so on erased, until
    /// one repairs. Each erasure spends one parity symbol a hard decoder needs two for, so weak
    /// symbols past the error radius still come back. Erasures stop at half the parity: past
    /// that a wrong word passes too easily and the erased symbols it rewrites were often right.
    /// On success `word` holds the codeword and the count of repaired symbols comes back; a
    /// refused word is left as it was.
    pub fn decode_gmd(&self, word: &mut [u8], reliability: &[f32]) -> Option<u32> {
        self.decode_gmd_with(word, reliability, &[], self.parity / 2, |_| true)
    }

    /// [`Self::decode_gmd`] with the symbols at `known` always erased, as punctured ones are, at
    /// most `most` others erased by reliability, and a trial kept only when `accept` takes its
    /// codeword, as a CRC does. More erasures need a stronger `accept`.
    pub fn decode_gmd_with(
        &self,
        word: &mut [u8],
        reliability: &[f32],
        known: &[usize],
        most: usize,
        mut accept: impl FnMut(&[u8]) -> bool,
    ) -> Option<u32> {
        let len = word.len();
        if len >= SIZE || reliability.len() < len || known.len() > len {
            return None;
        }
        let mut erasures = [0usize; SIZE];
        erasures[..known.len()].copy_from_slice(known);
        let weakest = weakest(reliability, len, known, most, &mut erasures[known.len()..]);
        let mut trial = [0u8; SIZE];
        let trial = &mut trial[..len];
        for extra in (0..=weakest).step_by(2) {
            trial.copy_from_slice(word);
            let erased = &erasures[..known.len() + extra];
            let repaired = if erased.is_empty() {
                self.decode(trial)
            } else {
                self.decode_with_erasures(trial, erased)
            };
            if let Some(count) = repaired
                && accept(trial)
            {
                word.copy_from_slice(trial);
                return Some(count);
            }
        }
        None
    }
}

/// Fills `out` with the least reliable of the first `len` symbols not in `known`, weakest
/// first and at most `most` of them, and returns how many.
fn weakest(
    reliability: &[f32],
    len: usize,
    known: &[usize],
    most: usize,
    out: &mut [usize],
) -> usize {
    let mut order: [usize; SIZE] = std::array::from_fn(|index| index);
    let order = &mut order[..len];
    order.sort_unstable_by(|&a, &b| reliability[a].total_cmp(&reliability[b]).then(a.cmp(&b)));
    let mut count = 0;
    for &index in order.iter().filter(|index| !known.contains(index)) {
        if count == most.min(out.len()) {
            break;
        }
        out[count] = index;
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARITY: usize = 16;

    fn sent() -> (ReedSolomon, Vec<u8>) {
        let code = ReedSolomon::new(0x11D, 0, PARITY);
        let data: Vec<u8> = (0..40u8).map(|n| n.wrapping_mul(37) ^ 0x5A).collect();
        let mut word = Vec::new();
        code.encode(&data, &mut word);
        (code, word)
    }

    #[test]
    fn weak_symbols_past_the_error_radius_are_erased_and_repaired() {
        let (code, clean) = sent();
        let mut word = clean.clone();
        let mut reliability = vec![1.0; word.len()];
        for at in [1, 6, 9, 17, 22, 30, 33, 41, 47, 52] {
            word[at] ^= 0xC3;
            reliability[at] = 0.1;
        }
        let mut hard = word.clone();
        assert_eq!(code.decode(&mut hard), None);
        assert_eq!(code.decode_gmd(&mut word, &reliability), Some(10));
        assert_eq!(word, clean);
    }

    #[test]
    fn a_word_past_every_trial_is_left_alone() {
        let (code, clean) = sent();
        let mut word = clean;
        for at in (0..word.len()).step_by(3) {
            word[at] ^= 0x11;
        }
        let before = word.clone();
        let reliability = vec![1.0; word.len()];
        assert_eq!(code.decode_gmd(&mut word, &reliability), None);
        assert_eq!(word, before);
    }

    #[test]
    fn known_erasures_are_always_in_and_accept_has_the_last_word() {
        let (code, clean) = sent();
        let mut word = clean.clone();
        let punctured = [50, 51, 52, 53, 54, 55];
        for &at in &punctured {
            word[at] = 0;
        }
        let mut reliability = vec![1.0; word.len()];
        for at in [2, 8, 14, 20, 26, 32] {
            word[at] ^= 0x3C;
            reliability[at] = 0.1;
        }
        let mut hard = word.clone();
        assert_eq!(code.decode_with_erasures(&mut hard, &punctured), None);
        let mut refused = word.clone();
        let none = code.decode_gmd_with(&mut refused, &reliability, &punctured, 4, |_| false);
        assert_eq!(none, None);
        assert_eq!(refused, word);
        let mut trials = 0;
        let fixed = code.decode_gmd_with(&mut word, &reliability, &punctured, 4, |_| {
            trials += 1;
            true
        });
        assert!(fixed.is_some());
        assert_eq!(word, clean);
        assert_eq!(trials, 1);
    }
}
