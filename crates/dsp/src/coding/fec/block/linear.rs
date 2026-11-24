use crate::coding::fec::soft_ml::{MAX_BITS, decode};

const MAX_ROWS: usize = 16;

/// A short binary linear code given by its generator rows, decoded by soft maximum likelihood
/// over every codeword.
#[derive(Clone, Copy, Debug)]
pub struct LinearCode {
    rows: [u64; MAX_ROWS],
    k: usize,
    n: usize,
    distance: u32,
}

impl LinearCode {
    /// The code `rows` span, each row a codeword of `n` bits with the first sent most
    /// significant; message bit `i` adds row `i`. The minimum distance comes from walking
    /// every codeword, so build it once, as a constant or at startup. Panics past 16 rows or
    /// 64 bits, on a row wider than `n`, or on rows that are not independent.
    #[must_use]
    pub const fn new(rows: &[u64], n: usize) -> Self {
        assert!(
            rows.len() <= MAX_ROWS && n <= MAX_BITS,
            "too large to search"
        );
        let mut code = Self {
            rows: [0; MAX_ROWS],
            k: rows.len(),
            n,
            distance: u32::MAX,
        };
        let mut row = 0;
        while row < rows.len() {
            assert!(n == MAX_BITS || rows[row] >> n == 0, "a row wider than n");
            code.rows[row] = rows[row];
            row += 1;
        }
        let mut codeword = 0u64;
        let mut step = 1u32;
        while step < 1 << rows.len() {
            codeword ^= rows[step.trailing_zeros() as usize];
            if codeword.count_ones() < code.distance {
                code.distance = codeword.count_ones();
            }
            step += 1;
        }
        assert!(code.distance > 0, "rows not independent");
        code
    }

    #[must_use]
    pub const fn n(&self) -> usize {
        self.n
    }

    #[must_use]
    pub const fn k(&self) -> usize {
        self.k
    }

    #[must_use]
    pub const fn distance(&self) -> u32 {
        self.distance
    }

    /// The codeword of `message`, the first sent most significant.
    #[must_use]
    pub fn encode(&self, message: u32) -> u64 {
        self.rows[..self.k]
            .iter()
            .enumerate()
            .filter(|&(bit, _)| message >> bit & 1 == 1)
            .fold(0, |codeword, (_, &row)| codeword ^ row)
    }

    /// Soft maximum likelihood over every codeword: `soft` holds one value a bit, first sent
    /// first, positive for a one and zero for an erasure, in any scale. Returns the message and
    /// the decisions it overturns, or `None` when `soft` is not `n` long or the best codeword
    /// does not stand: on hard input exactly when a hard decoder of the code's distance would
    /// refuse, beyond its radius only through weak bits.
    #[must_use]
    pub fn decode_soft(&self, soft: &[f32]) -> Option<(u32, u32)> {
        if soft.len() != self.n {
            return None;
        }
        decode(&self.rows[..self.k], soft, self.distance)
            .map(|(message, _, overturned)| (message, overturned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The extended Hamming (8,4) code, data first.
    const HAMMING_8_4: LinearCode = LinearCode::new(&[0x1E, 0x2D, 0x4B, 0x87], 8);
    const HAMMING_7_4: LinearCode = LinearCode::new(&[0x0D, 0x17, 0x23, 0x46], 7);

    fn soften(code: &LinearCode, word: u64) -> Vec<f32> {
        (0..code.n())
            .map(|at| {
                if word >> (code.n() - 1 - at) & 1 == 1 {
                    1.0
                } else {
                    -1.0
                }
            })
            .collect()
    }

    fn nearest(code: &LinearCode, word: u64) -> Option<(u32, u32)> {
        let t = (code.distance() - 1) / 2;
        (0..1 << code.k())
            .map(|message| (message, (code.encode(message) ^ word).count_ones()))
            .find(|&(_, distance)| distance <= t)
    }

    #[test]
    fn the_distance_is_found_from_the_rows() {
        assert_eq!(HAMMING_7_4.distance(), 3);
        assert_eq!(HAMMING_8_4.distance(), 4);
        assert_eq!((HAMMING_8_4.n(), HAMMING_8_4.k()), (8, 4));
    }

    #[test]
    fn hard_input_decodes_as_bounded_distance_does() {
        for code in [HAMMING_7_4, HAMMING_8_4] {
            for word in 0..1u64 << code.n() {
                assert_eq!(
                    code.decode_soft(&soften(&code, word)),
                    nearest(&code, word),
                    "n {} word {word:#x}",
                    code.n()
                );
            }
        }
    }

    #[test]
    fn two_weak_errors_are_corrected_and_strangers_refused() {
        let code = HAMMING_8_4;
        let clean = code.encode(0b1011);
        let mut soft = soften(&code, clean ^ 0b1000_0001);
        assert_eq!(code.decode_soft(&soft), None);
        soft[0] *= 0.1;
        soft[7] *= 0.1;
        assert_eq!(code.decode_soft(&soft), Some((0b1011, 2)));
        assert_eq!(code.decode_soft(&soft[..7]), None);
    }

    #[test]
    fn erasures_fill_in_up_to_the_distance() {
        let code = HAMMING_8_4;
        let clean = code.encode(0b0110);
        let mut soft = soften(&code, clean);
        soft[1..4].fill(0.0);
        assert_eq!(code.decode_soft(&soft), Some((0b0110, 0)));
        soft[5] = 0.0;
        assert_eq!(code.decode_soft(&soft), None);
    }
}
