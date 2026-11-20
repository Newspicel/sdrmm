use std::ops::Mul;

use super::{
    kernel::{BLOCK_FLOATS, Sample},
    run::{Run, runs},
};

#[cfg(target_arch = "aarch64")]
#[derive(Clone, Debug)]
pub(crate) struct Row {
    pub(super) offset: usize,
    pub(super) taps: std::ops::Range<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct Plan<C> {
    #[cfg_attr(target_arch = "aarch64", allow(dead_code))]
    pub(super) runs: Vec<Run>,
    #[cfg_attr(target_arch = "aarch64", allow(dead_code))]
    pub(super) values: Vec<C>,
    #[cfg(target_arch = "aarch64")]
    pub(super) rows: Vec<Row>,
    #[cfg(target_arch = "aarch64")]
    pub(super) row_taps: Vec<C>,
    #[cfg(test)]
    pub(super) reach: usize,
    #[cfg_attr(
        not(any(target_arch = "aarch64", target_arch = "x86_64")),
        allow(dead_code, reason = "only the SIMD kernels walk the window unchecked")
    )]
    pub(super) floats: usize,
    pub(super) folded: bool,
}

impl<C> Plan<C>
where
    C: Copy + PartialEq + Default + Mul<f32, Output = C>,
{
    pub(crate) fn new<T: Sample>(taps: &[C], phases: usize, stride: usize, fold: bool) -> Self {
        let offset = |index: usize| ((index % phases) * stride + index / phases) * T::FLOATS;
        let reversed: Vec<C> = taps.iter().rev().copied().collect();
        let folded = fold && reversed.iter().eq(taps.iter());
        let last = taps.len() - 1;
        let farthest = (0..taps.len()).map(offset).max().unwrap_or(0);
        let reach = farthest / T::FLOATS + T::LANES;
        let count = if folded {
            taps.len().div_ceil(2)
        } else {
            taps.len()
        };
        let (runs, values) = runs(count, phases, folded, |index| {
            let mirror = if folded { last - index } else { index };
            let value = if folded && index == mirror {
                reversed[index] * 0.5
            } else {
                reversed[index]
            };
            (offset(index), offset(mirror), value)
        });
        #[cfg(target_arch = "aarch64")]
        let (rows, row_taps) = rows::<T, C>(&reversed, phases, stride);
        Self {
            #[cfg(test)]
            reach,
            floats: reach * T::FLOATS,
            runs,
            values,
            #[cfg(target_arch = "aarch64")]
            rows,
            #[cfg(target_arch = "aarch64")]
            row_taps,
            folded,
        }
    }

    pub(crate) fn folded(&self) -> bool {
        self.folded
    }
}

impl<C: Copy> Plan<C> {
    /// Every tap as its offset, its mirror's offset and its value, in run order. The mirror is
    /// only meaningful in a folded plan.
    #[cfg(any(test, not(any(target_arch = "aarch64", target_arch = "x86_64"))))]
    pub(super) fn taps(&self) -> impl Iterator<Item = (usize, usize, C)> + '_ {
        self.runs.iter().flat_map(move |run| {
            self.values[run.range()]
                .iter()
                .enumerate()
                .map(move |(at, &value)| {
                    let (front, back) = run.offsets(at);
                    (front, back, value)
                })
        })
    }
}

/// One row per phase, with the zero taps at either end of a row left out: a half-band
/// decimator's second phase shrinks to its centre tap.
#[cfg(target_arch = "aarch64")]
fn rows<T: Sample, C: Copy + PartialEq + Default>(
    reversed: &[C],
    phases: usize,
    stride: usize,
) -> (Vec<Row>, Vec<C>) {
    let mut rows = Vec::with_capacity(phases);
    let mut row_taps = Vec::with_capacity(reversed.len());
    for phase in 0..phases.min(reversed.len()) {
        let phase_taps: Vec<C> = reversed[phase..].iter().step_by(phases).copied().collect();
        let Some(first) = phase_taps.iter().position(|&tap| tap != C::default()) else {
            continue;
        };
        let last = phase_taps
            .iter()
            .rposition(|&tap| tap != C::default())
            .unwrap_or(first);
        let start = row_taps.len();
        row_taps.extend_from_slice(&phase_taps[first..=last]);
        rows.push(Row {
            offset: (phase * stride + first) * T::FLOATS,
            taps: start..row_taps.len(),
        });
    }
    (rows, row_taps)
}

/// The window floats `count` blocks of outputs read, checked once so the kernels can walk them
/// unchecked. `None` when there is no block to compute.
#[cfg_attr(
    not(any(target_arch = "aarch64", target_arch = "x86_64")),
    allow(dead_code, reason = "only the SIMD kernels walk the window unchecked")
)]
pub(super) fn reach_of<'a, C>(floats: &'a [f32], plan: &Plan<C>, out: &[f32]) -> Option<&'a [f32]> {
    let last = out.len().checked_sub(BLOCK_FLOATS)?;
    Some(&floats[..last + plan.floats])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folding_needs_mirrored_taps() {
        let symmetric = [0.25f32, 0.5, 1.0, 0.5, 0.25];
        assert!(Plan::new::<f32>(&symmetric, 1, 64, true).folded());
        assert!(!Plan::new::<f32>(&symmetric, 1, 64, false).folded());
        assert!(!Plan::new::<f32>(&[0.25, 0.5, 1.0, 0.5, 0.2], 1, 64, true).folded());
        assert_eq!(Plan::new::<f32>(&symmetric, 1, 64, true).values.len(), 3);
        assert_eq!(Plan::new::<f32>(&[0.5, 0.5], 1, 64, true).values.len(), 1);
    }
}
