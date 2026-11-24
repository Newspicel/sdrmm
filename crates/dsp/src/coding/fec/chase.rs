//! Chase-II soft decoding around a hard decoder.

use std::ops::BitXor;

use super::acceptance::{stands, within};

const MAX_BITS: usize = 128;

/// A word Chase search flips bit by bit: `u32`, `u64` or `u128`.
pub trait ChaseWord: Copy + BitXor<Output = Self> + sealed::Sealed {
    const BITS: usize;
    const ZERO: Self;

    #[must_use]
    fn bit(index: usize) -> Self;

    #[must_use]
    fn has(self, index: usize) -> bool;
}

mod sealed {
    pub trait Sealed {}
}

macro_rules! chase_word {
    ($($word:ty),*) => {$(
        impl sealed::Sealed for $word {}

        impl ChaseWord for $word {
            const BITS: usize = <$word>::BITS as usize;
            const ZERO: Self = 0;

            fn bit(index: usize) -> Self {
                1 << index
            }

            fn has(self, index: usize) -> bool {
                self >> index & 1 == 1
            }
        }
    )*};
}

chase_word!(u32, u64, u128);

/// When the least costly candidate stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acceptance {
    /// Always, for a repair whose own reach already bounds the result, as burst correction's
    /// does.
    Nearest,
    /// When a hard decoder correcting this many errors would take it, or it overturns only
    /// weak bits worth no more than that many typical ones. For decoders a later check backs.
    Radius(u32),
    /// When it is the only codeword the word can mean for a code of this minimum distance, the
    /// rule soft maximum likelihood decoding keeps. Stricter than [`Self::Radius`].
    Distance(u32),
}

/// Chase-II soft decoding around any hard decoder.
///
/// Bit `i` of `word` has confidence `reliability[i]`, zero for an erasure. Every pattern of
/// flips over the `flips` least reliable bits (ties broken by position) goes through `repair`,
/// starting with no flips; the candidate whose differing bits carry the least total
/// reliability wins, the earliest on a tie, and comes back when `acceptance` takes it. Under
/// [`Acceptance::Radius`] or [`Acceptance::Distance`] hard input decodes exactly as the hard
/// decoder does and only weak bits buy reach beyond it. `None`
/// when nothing repairs, the winner is refused, or `reliability` is longer than the word.
pub fn chase<W: ChaseWord>(
    word: W,
    reliability: &[f32],
    flips: usize,
    acceptance: Acceptance,
    mut repair: impl FnMut(W) -> Option<W>,
) -> Option<W> {
    let length = reliability.len();
    if length > W::BITS {
        return None;
    }
    let flips = flips.min(length).min(u32::BITS as usize - 1);
    let mut order: [usize; MAX_BITS] = std::array::from_fn(|bit| bit);
    let order = &mut order[..length];
    order.sort_unstable_by(|&a, &b| reliability[a].total_cmp(&reliability[b]).then(a.cmp(&b)));
    let weak = &order[..flips];
    let mut best: Option<(f32, W)> = None;
    for pattern in 0..1u32 << flips {
        let mask = weak
            .iter()
            .enumerate()
            .filter(|(index, _)| pattern >> index & 1 == 1)
            .fold(W::ZERO, |mask, (_, &bit)| mask ^ W::bit(bit));
        let Some(candidate) = repair(word ^ mask) else {
            continue;
        };
        let cost = cost(word ^ candidate, reliability);
        if best.is_none_or(|(least, _)| cost < least) {
            best = Some((cost, candidate));
        }
    }
    let (_, candidate) = best?;
    let overturned = word ^ candidate;
    let bits = reliability
        .iter()
        .enumerate()
        .map(|(bit, &value)| (value, overturned.has(bit)));
    let stood = match acceptance {
        Acceptance::Nearest => true,
        Acceptance::Radius(radius) => within(bits, radius),
        Acceptance::Distance(distance) => stands(bits, distance),
    };
    stood.then_some(candidate)
}

fn cost<W: ChaseWord>(differ: W, reliability: &[f32]) -> f32 {
    reliability
        .iter()
        .enumerate()
        .filter(|&(bit, _)| differ.has(bit))
        .map(|(_, &weight)| weight)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repetition(word: u64) -> Option<u64> {
        match word.count_ones() {
            0..=2 => Some(0),
            3..=5 => Some(0b11111),
            _ => None,
        }
    }

    const NEAREST: Acceptance = Acceptance::Nearest;
    const HARD_RADIUS: Acceptance = Acceptance::Radius(2);

    #[test]
    fn a_word_the_hard_decoder_rejects_is_rescued_through_its_weak_bits() {
        let reliability = [1.0, 0.1, 1.0, 0.2, 1.0];
        let strict = |word: u64| repetition(word).filter(|&fixed| (fixed ^ word).count_ones() < 2);
        assert_eq!(strict(0b10101), None);
        assert_eq!(
            chase(0b10101, &reliability, 2, Acceptance::Radius(1), strict),
            Some(0b11111)
        );
    }

    #[test]
    fn the_least_costly_candidate_wins() {
        let cheap_ones = [0.15, 0.9, 0.1, 0.2, 0.5];
        assert_eq!(
            chase(0b00011, &cheap_ones, 3, NEAREST, repetition),
            Some(0b11111)
        );
        let dear_ones = [0.15, 0.9, 0.1, 0.2, 0.9];
        assert_eq!(chase(0b00011, &dear_ones, 3, NEAREST, repetition), Some(0));
    }

    #[test]
    fn hard_input_decodes_as_the_hard_decoder() {
        let sure = [1.0; 5];
        let strict = |word: u64| repetition(word).filter(|&fixed| (fixed ^ word).count_ones() < 2);
        for word in 0..32u64 {
            assert_eq!(
                chase(word, &sure, 3, Acceptance::Radius(1), strict),
                strict(word),
                "{word:#07b}"
            );
            assert_eq!(
                chase(word, &sure, 3, HARD_RADIUS, repetition),
                repetition(word)
            );
        }
    }

    #[test]
    fn a_hopeless_or_oversized_word_gives_nothing() {
        assert_eq!(chase(0b10101u64, &[1.0; 5], 1, NEAREST, |_| None), None);
        assert_eq!(chase(0u64, &[1.0; 65], 1, NEAREST, Some), None);
        assert_eq!(chase(0u32, &[1.0; 33], 1, NEAREST, Some), None);
    }

    #[test]
    fn a_clean_word_is_kept() {
        assert_eq!(
            chase(0b11111, &[0.5; 5], 3, HARD_RADIUS, repetition),
            Some(0b11111)
        );
    }

    #[test]
    fn words_past_64_bits_decode_whole() {
        let parity = |word: u128| word.count_ones().is_multiple_of(2).then_some(word);
        let mut reliability = [1.0; 100];
        reliability[90] = 0.1;
        let sent = 1u128 << 99 | 1;
        let parity_check = Acceptance::Distance(2);
        assert_eq!(
            chase(sent ^ 1 << 90, &reliability, 1, parity_check, parity),
            Some(sent)
        );
        assert_eq!(
            chase(sent ^ 1 << 90, &[1.0; 100], 1, parity_check, parity),
            None
        );
    }
}
