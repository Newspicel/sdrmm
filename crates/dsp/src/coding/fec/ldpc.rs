//! Low-density parity-check decoding.
//!
//! A code is data: [`ParityCheck`] holds any sparse parity-check matrix, and
//! [`QcLayout`] the circulant description of a quasi-cyclic one. [`MinSum`] decodes any
//! [`ParityCheck`] in floating point with a layered or flooding schedule. [`QuasiCyclic`]
//! runs the same normalized min-sum layer by layer on 16-bit lanes, one lane per row of a
//! circulant, for the long codes where that parallelism pays. [`Systematic`] encodes a code
//! given by generator rows, and [`Osd`] decodes any code by ordered statistics.
//! [`RepeatAccumulate`] encodes and describes the staircase codes codec2 modems use.

mod lanes;
mod min_sum;
mod osd;
mod quasi_cyclic;
mod repeat_accumulate;
mod systematic;

pub use lanes::Kernel;
pub use min_sum::{MinSum, Schedule};
pub use osd::{Osd, OsdWord};
pub use quasi_cyclic::{Circulant, QcLayout, QuasiCyclic};
pub use repeat_accumulate::RepeatAccumulate;
pub use systematic::Systematic;

/// A sparse binary parity-check matrix, stored by check (row) and by variable (column).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParityCheck {
    variables: usize,
    check_offsets: Vec<usize>,
    check_variables: Vec<usize>,
    variable_offsets: Vec<usize>,
    variable_edges: Vec<usize>,
}

impl ParityCheck {
    /// Builds the matrix from `(check, variable)` pairs. Pairs are sorted, so each check lists
    /// its variables in ascending order. `checks` and `variables` are lower bounds; the matrix
    /// grows to hold every pair.
    #[must_use]
    pub fn from_edges(checks: usize, variables: usize, edges: &[(usize, usize)]) -> Self {
        let checks = edges
            .iter()
            .map(|&(check, _)| check + 1)
            .fold(checks, usize::max);
        let variables = edges
            .iter()
            .map(|&(_, variable)| variable + 1)
            .fold(variables, usize::max);
        let mut sorted = edges.to_vec();
        sorted.sort_unstable();
        let (check_offsets, check_variables) = compress(checks, &sorted);
        let mirrored: Vec<(usize, usize)> = sorted
            .iter()
            .enumerate()
            .map(|(edge, &(_, variable))| (variable, edge))
            .collect();
        let mut by_variable = mirrored;
        by_variable.sort_unstable();
        let (variable_offsets, variable_edges) = compress(variables, &by_variable);
        Self {
            variables,
            check_offsets,
            check_variables,
            variable_offsets,
            variable_edges,
        }
    }

    #[must_use]
    pub const fn variables(&self) -> usize {
        self.variables
    }

    #[must_use]
    pub const fn checks(&self) -> usize {
        self.check_offsets.len() - 1
    }

    #[must_use]
    pub const fn edges(&self) -> usize {
        self.check_variables.len()
    }

    /// The variables of `check`, ascending.
    #[must_use]
    pub fn check(&self, check: usize) -> &[usize] {
        &self.check_variables[self.check_edges(check)]
    }

    /// The edge indices of `check`; edge `e` joins the check to `check_variables[e]`.
    #[must_use]
    pub fn check_edges(&self, check: usize) -> std::ops::Range<usize> {
        self.check_offsets[check]..self.check_offsets[check + 1]
    }

    /// The edges touching `variable`.
    #[must_use]
    pub fn variable_edges(&self, variable: usize) -> &[usize] {
        &self.variable_edges[self.variable_offsets[variable]..self.variable_offsets[variable + 1]]
    }

    /// Every `(check, variable)` pair in check order.
    pub fn pairs(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        (0..self.checks()).flat_map(|check| self.check(check).iter().map(move |&v| (check, v)))
    }

    #[must_use]
    pub fn unsatisfied(&self, word: &[bool]) -> usize {
        (0..self.checks())
            .filter(|&check| {
                self.check(check)
                    .iter()
                    .fold(false, |parity, &variable| parity ^ word[variable])
            })
            .count()
    }

    #[must_use]
    pub fn satisfies(&self, word: &[bool]) -> bool {
        (0..self.checks()).all(|check| {
            !self
                .check(check)
                .iter()
                .fold(false, |parity, &variable| parity ^ word[variable])
        })
    }
}

fn compress(count: usize, sorted: &[(usize, usize)]) -> (Vec<usize>, Vec<usize>) {
    let mut offsets = vec![0; count + 1];
    for &(key, _) in sorted {
        offsets[key + 1] += 1;
    }
    for index in 0..count {
        offsets[index + 1] += offsets[index];
    }
    (offsets, sorted.iter().map(|&(_, value)| value).collect())
}

#[cfg(test)]
mod tests;
