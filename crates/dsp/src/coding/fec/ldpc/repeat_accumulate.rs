use super::ParityCheck;

/// A systematic repeat-accumulate LDPC code: parity bit `j` is parity bit `j - 1` plus the data
/// bits row `j` names, so the parity part of the matrix is a staircase.
///
/// This is the shape of the codec2 `HRA` codes (Wenet, `FreeDV` data modes): the code is a table
/// of data positions per parity bit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepeatAccumulate {
    data_bits: usize,
    rows: Vec<Vec<usize>>,
}

impl RepeatAccumulate {
    /// A code over `data_bits` data bits whose parity bit `j` sums the data bits `rows[j]` names
    /// (zero-based). A position named twice in one row cancels, as it does in the sum.
    #[must_use]
    pub fn new(data_bits: usize, rows: Vec<Vec<usize>>) -> Self {
        let rows = rows
            .into_iter()
            .map(|mut row| {
                row.sort_unstable();
                odd_once(&row)
            })
            .collect();
        Self { data_bits, rows }
    }

    #[must_use]
    pub const fn data_bits(&self) -> usize {
        self.data_bits
    }

    #[must_use]
    pub fn parity_bits(&self) -> usize {
        self.rows.len()
    }

    #[must_use]
    pub fn length(&self) -> usize {
        self.data_bits + self.rows.len()
    }

    /// Fills `parity`, one entry per parity bit, from `data`.
    pub fn encode(&self, data: &[bool], parity: &mut [bool]) {
        let mut previous = false;
        for (row, slot) in self.rows.iter().zip(parity.iter_mut()) {
            let sum = row
                .iter()
                .fold(previous, |acc, &position| acc ^ data[position]);
            *slot = sum;
            previous = sum;
        }
    }

    /// The parity-check matrix over the codeword: data bits first, then parity bits.
    #[must_use]
    pub fn parity_check(&self) -> ParityCheck {
        let mut edges = Vec::new();
        for (check, row) in self.rows.iter().enumerate() {
            edges.extend(row.iter().map(|&position| (check, position)));
            edges.push((check, self.data_bits + check));
            if check > 0 {
                edges.push((check, self.data_bits + check - 1));
            }
        }
        ParityCheck::from_edges(self.rows.len(), self.length(), &edges)
    }
}

fn odd_once(sorted: &[usize]) -> Vec<usize> {
    let mut kept = Vec::with_capacity(sorted.len());
    for &position in sorted {
        if kept.last() == Some(&position) {
            kept.pop();
        } else {
            kept.push(position);
        }
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding::fec::ldpc::{MinSum, Schedule};

    fn code() -> RepeatAccumulate {
        let rows = (0..8)
            .map(|row| (0..3).map(|k| (row * 5 + k * 7) % 24).collect())
            .collect();
        RepeatAccumulate::new(24, rows)
    }

    fn word(code: &RepeatAccumulate, seed: usize) -> Vec<bool> {
        let data: Vec<bool> = (0..code.data_bits())
            .map(|i| (i * 7 + seed).is_multiple_of(3))
            .collect();
        let mut parity = vec![false; code.parity_bits()];
        code.encode(&data, &mut parity);
        data.into_iter().chain(parity).collect()
    }

    #[test]
    fn codewords_satisfy_the_staircase_matrix() {
        let code = code();
        let check = code.parity_check();
        assert_eq!(check.variables(), 32);
        assert_eq!(check.checks(), 8);
        for seed in 0..5 {
            assert!(check.satisfies(&word(&code, seed)));
        }
    }

    #[test]
    fn a_repeated_position_cancels() {
        let code = RepeatAccumulate::new(4, vec![vec![1, 1, 2]]);
        let mut parity = [false];
        code.encode(&[false, true, true, false], &mut parity);
        assert!(parity[0]);
        assert_eq!(code.parity_check().check(0), &[2, 4]);
    }

    #[test]
    fn min_sum_repairs_a_flipped_bit() {
        let code = code();
        let sent = word(&code, 1);
        let mut llrs: Vec<f32> = sent.iter().map(|&b| if b { -4.0 } else { 4.0 }).collect();
        llrs[5] = -llrs[5] * 0.5;
        let mut decoder = MinSum::new(code.parity_check(), Schedule::Layered, 0.75);
        assert!(decoder.decode(&llrs, 20).is_some());
        assert_eq!(decoder.hard(), sent.as_slice());
    }
}
