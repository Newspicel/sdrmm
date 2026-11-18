/// Chase-II soft decoding around any hard decoder of words up to 64 bits.
///
/// Bit `i` of `word` has confidence `reliability[i]`. Every pattern of flips over the
/// `flips` least reliable bits (ties broken by position) goes through `repair`, starting
/// with no flips; the candidate whose differing bits carry the least total reliability
/// wins, the earliest on a tie. `None` when no pattern repairs or `reliability` is longer
/// than 64.
pub fn chase(
    word: u64,
    reliability: &[f32],
    flips: usize,
    mut repair: impl FnMut(u64) -> Option<u64>,
) -> Option<u64> {
    let length = reliability.len();
    if length > 64 {
        return None;
    }
    let flips = flips.min(length);
    let mut order: [usize; 64] = std::array::from_fn(|bit| bit);
    let order = &mut order[..length];
    order.sort_unstable_by(|&a, &b| reliability[a].total_cmp(&reliability[b]).then(a.cmp(&b)));
    let weak = &order[..flips];
    let mut best: Option<(f32, u64)> = None;
    for pattern in 0..1u64 << flips {
        let mask = weak
            .iter()
            .enumerate()
            .filter(|(index, _)| pattern >> index & 1 == 1)
            .fold(0u64, |mask, (_, &bit)| mask | 1 << bit);
        let Some(candidate) = repair(word ^ mask) else {
            continue;
        };
        let distance = distance(word, candidate, reliability);
        if best.is_none_or(|(metric, _)| distance < metric) {
            best = Some((distance, candidate));
        }
    }
    best.map(|(_, candidate)| candidate)
}

fn distance(word: u64, candidate: u64, reliability: &[f32]) -> f32 {
    let differ = word ^ candidate;
    reliability
        .iter()
        .enumerate()
        .filter(|&(bit, _)| differ >> bit & 1 == 1)
        .map(|(_, &weight)| weight)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repetition(word: u64) -> Option<u64> {
        match word.count_ones() {
            0 | 1 => Some(0),
            4 | 5 => Some(0b11111),
            _ => None,
        }
    }

    #[test]
    fn a_word_the_hard_decoder_rejects_is_rescued_through_its_weak_bits() {
        let reliability = [1.0, 0.1, 1.0, 0.2, 1.0];
        assert_eq!(repetition(0b10101), None);
        assert_eq!(chase(0b10101, &reliability, 2, repetition), Some(0b11111));
    }

    #[test]
    fn the_least_costly_candidate_wins() {
        let cheap_ones = [0.15, 0.9, 0.1, 0.2, 0.5];
        assert_eq!(chase(0b00011, &cheap_ones, 3, repetition), Some(0b11111));
        let dear_ones = [0.15, 0.9, 0.1, 0.2, 0.9];
        assert_eq!(chase(0b00011, &dear_ones, 3, repetition), Some(0));
    }

    #[test]
    fn a_hopeless_or_oversized_word_gives_nothing() {
        assert_eq!(chase(0b10101, &[1.0; 5], 1, repetition), None);
        assert_eq!(chase(0, &[1.0; 65], 1, Some), None);
    }

    #[test]
    fn a_clean_word_is_kept() {
        assert_eq!(chase(0b11111, &[0.5; 5], 3, repetition), Some(0b11111));
    }
}
