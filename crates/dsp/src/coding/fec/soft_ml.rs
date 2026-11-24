//! Soft maximum likelihood decoding of short binary linear codes by trying every codeword.

use super::{
    acceptance::stands,
    conv::{CONFIDENT, Soft},
};

pub(crate) const MAX_BITS: usize = 64;
const BYTES: usize = MAX_BITS / 8;

/// Every codeword spanned by `rows`, as (message, codeword), each one row away from the last
/// in Gray order and starting with message zero.
pub(crate) fn span(rows: &[u64]) -> impl Iterator<Item = (u32, u64)> + '_ {
    (0..1u32 << rows.len()).scan(0u64, |codeword, step| {
        if step > 0 {
            *codeword ^= rows[step.trailing_zeros() as usize];
        }
        Some((step ^ step >> 1, *codeword))
    })
}

/// Soft maximum likelihood over the code `rows` span, of minimum distance `distance`: the
/// message, codeword and overturned decisions of the best correlated codeword when it
/// [`stands`]. `soft` holds one value a bit, first sent first, positive for a one.
pub(crate) fn decode(rows: &[u64], soft: &[f32], distance: u32) -> Option<(u32, u64, u32)> {
    let received = SoftWord::new(soft);
    let (message, codeword) = received.best(span(rows));
    received
        .stands(codeword, distance)
        .then(|| (message, codeword, received.overturned(codeword)))
}

/// `soft` clamped to [`CONFIDENT`] as the convolutional decoders read it, past 64 values cut.
pub(crate) fn confident(soft: &[Soft]) -> ([f32; MAX_BITS], usize) {
    let mut values = [0.0; MAX_BITS];
    let len = soft.len().min(MAX_BITS);
    for (value, &level) in values.iter_mut().zip(soft) {
        *value = f32::from(level.clamp(-CONFIDENT, CONFIDENT));
    }
    (values, len)
}

/// A received word of up to 64 soft bits, the first sent in the most significant place. Per
/// byte sums make scoring a codeword one lookup a byte.
pub(crate) struct SoftWord {
    ones: [[f32; 256]; BYTES],
    magnitudes: [f32; MAX_BITS],
    bytes: usize,
    len: usize,
    total: f32,
    hard: u64,
    erased: u64,
}

impl SoftWord {
    pub(crate) fn new(soft: &[f32]) -> Self {
        debug_assert!(soft.len() <= MAX_BITS, "longer than a u64");
        let mut values = [0.0f32; MAX_BITS];
        let mut word = Self {
            ones: [[0.0; 256]; BYTES],
            magnitudes: [0.0; MAX_BITS],
            bytes: soft.len().div_ceil(8).min(BYTES),
            len: soft.len().min(MAX_BITS),
            total: 0.0,
            hard: 0,
            erased: 0,
        };
        for (place, &value) in soft.iter().rev().take(MAX_BITS).enumerate() {
            values[place] = value;
            word.magnitudes[place] = value.abs();
            word.total += value;
            word.hard |= u64::from(value > 0.0) << place;
            word.erased |= u64::from(value == 0.0) << place;
        }
        for (byte, table) in word.ones.iter_mut().enumerate().take(word.bytes) {
            for pattern in 1..256usize {
                let place = byte * 8 + pattern.trailing_zeros() as usize;
                table[pattern] = table[pattern & (pattern - 1)] + values[place];
            }
        }
        word
    }

    /// The sum of every value, negated where `codeword` sends a zero.
    pub(crate) fn correlation(&self, codeword: u64) -> f32 {
        let ones: f32 = self
            .ones
            .iter()
            .enumerate()
            .map(|(byte, table)| table[usize::from((codeword >> (8 * byte)) as u8)])
            .sum();
        2.0 * ones - self.total
    }

    /// The channel's decisions `codeword` overturns; an erasure decides nothing.
    pub(crate) fn overturned(&self, codeword: u64) -> u32 {
        ((codeword ^ self.hard) & !self.erased).count_ones()
    }

    /// The best correlated of `codewords`, as (message, codeword), the earliest on a tie. The
    /// codewords are of a linear code, so message zero sends the zero word.
    pub(crate) fn best(&self, codewords: impl IntoIterator<Item = (u32, u64)>) -> (u32, u64) {
        let (message, codeword, _) =
            codewords
                .into_iter()
                .fold((0, 0, self.correlation(0)), |best, (message, codeword)| {
                    let correlation = self.correlation(codeword);
                    if correlation > best.2 {
                        (message, codeword, correlation)
                    } else {
                        best
                    }
                });
        (message, codeword)
    }

    /// Whether `codeword`, of a code of minimum distance `distance`, [`stands`] for this word.
    pub(crate) fn stands(&self, codeword: u64, distance: u32) -> bool {
        let overturned = codeword ^ self.hard;
        let bits =
            (0..self.len).map(|place| (self.magnitudes[place], overturned >> place & 1 == 1));
        stands(bits, distance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_walks_every_combination_of_the_rows() {
        let rows = [0b001, 0b010, 0b100];
        let mut seen: Vec<_> = span(&rows).collect();
        assert_eq!(seen[0], (0, 0));
        assert!(
            seen.iter()
                .all(|&(message, codeword)| u64::from(message) == codeword)
        );
        seen.sort_unstable();
        assert_eq!(seen, (0..8).map(|m| (m, u64::from(m))).collect::<Vec<_>>());
    }

    #[test]
    fn the_byte_tables_correlate_like_a_plain_sum() {
        let levels: Vec<Soft> = (0..37).map(|i| (i * 29 % 200 - 100) as Soft).collect();
        let (values, len) = confident(&levels);
        let word = SoftWord::new(&values[..len]);
        for codeword in [0u64, 1, 0x1F_FFFF_FFFF, 0x12_3456_789A, 1 << 36] {
            let plain: f32 = values[..len]
                .iter()
                .enumerate()
                .map(|(i, &s)| if codeword >> (36 - i) & 1 == 1 { s } else { -s })
                .sum();
            assert_eq!(word.correlation(codeword), plain, "{codeword:#x}");
        }
    }

    #[test]
    fn erasures_count_against_acceptance_but_overturn_nothing() {
        let word = SoftWord::new(&[1.0, 0.0, 0.0, -1.0]);
        assert_eq!(word.overturned(0b1001), 1);
        assert!(word.stands(0b1000, 3));
        assert!(!word.stands(0b1000, 2));
    }
}
