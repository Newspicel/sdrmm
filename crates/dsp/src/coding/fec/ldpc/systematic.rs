use super::ParityCheck;

const WORD: usize = 64;

/// A systematic binary block code: the codeword is the message followed by one parity bit per
/// generator row, each the parity of the message bits that row selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Systematic {
    message_bits: usize,
    rows: Vec<Vec<u64>>,
}

impl Systematic {
    /// Reads generator rows written as hexadecimal, most significant bit first, the way WSJT-X
    /// tables give them. Each row covers at least `message_bits` bits; later bits are ignored.
    #[must_use]
    pub fn from_hex(message_bits: usize, rows: &[&str]) -> Option<Self> {
        let rows = rows
            .iter()
            .map(|row| hex_row(message_bits, row))
            .collect::<Option<Vec<_>>>()?;
        Some(Self { message_bits, rows })
    }

    #[must_use]
    pub const fn message_bits(&self) -> usize {
        self.message_bits
    }

    #[must_use]
    pub fn codeword_bits(&self) -> usize {
        self.message_bits + self.rows.len()
    }

    /// Writes `message` and its parity bits into `codeword`, which holds
    /// [`Systematic::codeword_bits`] bits.
    pub fn encode(&self, message: &[bool], codeword: &mut [bool]) {
        let mut packed = vec![0u64; self.message_bits.div_ceil(WORD)];
        for (bit, &set) in message.iter().take(self.message_bits).enumerate() {
            if set {
                packed[bit / WORD] |= 1 << (bit % WORD);
            }
        }
        for (slot, &bit) in codeword
            .iter_mut()
            .zip(message.iter().take(self.message_bits))
        {
            *slot = bit;
        }
        for (slot, row) in codeword[self.message_bits..].iter_mut().zip(&self.rows) {
            let ones: u32 = row
                .iter()
                .zip(&packed)
                .map(|(row, word)| (row & word).count_ones())
                .sum();
            *slot = ones % 2 == 1;
        }
    }
}

fn hex_row(message_bits: usize, row: &str) -> Option<Vec<u64>> {
    let mut packed = vec![0u64; message_bits.div_ceil(WORD)];
    let mut bit = 0;
    for digit in row.chars() {
        let value = digit.to_digit(16)?;
        for shift in (0..4).rev() {
            if bit < message_bits && value >> shift & 1 == 1 {
                packed[bit / WORD] |= 1 << (bit % WORD);
            }
            bit += 1;
        }
    }
    (bit >= message_bits).then_some(packed)
}

impl ParityCheck {
    /// Builds the matrix from the checks each variable joins, numbered from one, the `Mn`
    /// layout of WSJT-X tables.
    #[must_use]
    pub fn from_one_based_columns<const W: usize>(checks: usize, columns: &[[u16; W]]) -> Self {
        let edges: Vec<(usize, usize)> = columns
            .iter()
            .enumerate()
            .flat_map(|(variable, rows)| {
                rows.iter()
                    .filter(|&&row| row > 0)
                    .map(move |&row| (usize::from(row) - 1, variable))
            })
            .collect();
        Self::from_edges(checks, columns.len(), &edges)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parity_follows_the_generator_rows() {
        let code = Systematic::from_hex(5, &["c0", "a8", "f8"]).expect("valid rows");
        let message = [true, false, true, true, false];
        let mut codeword = [false; 8];
        code.encode(&message, &mut codeword);
        assert_eq!(&codeword[..5], &message);
        assert_eq!(&codeword[5..], &[true, false, true]);
    }

    #[test]
    fn a_short_or_invalid_row_is_refused() {
        assert!(Systematic::from_hex(9, &["ff"]).is_none());
        assert!(Systematic::from_hex(4, &["g0"]).is_none());
    }

    #[test]
    fn one_based_columns_name_their_checks() {
        let code = ParityCheck::from_one_based_columns(2, &[[1, 2], [2, 0], [1, 0]]);
        assert_eq!(code.check(0), &[0, 2]);
        assert_eq!(code.check(1), &[0, 1]);
        assert!(code.satisfies(&[true, true, true]));
        assert!(!code.satisfies(&[true, false, false]));
    }
}
