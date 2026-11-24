use num_complex::Complex;

use super::{grid::Grid, log_sum::LogSum};
use crate::soft::Llr;

/// A grid whose every label bit is set by one axis alone, as Gray-coded square QAM is. The
/// distance to a point splits into an in-phase and a quadrature part, so each bit's likelihood
/// ratio needs only the levels along its own axis: 256 of them for a 65536-point grid instead of
/// every point.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct SeparableLabels {
    base: u32,
    columns: Axis,
    rows: Axis,
}

#[derive(Clone, Debug, PartialEq)]
struct Axis {
    levels: Vec<f64>,
    labels: Vec<u32>,
    mask: u32,
}

impl SeparableLabels {
    pub(super) fn detect(grid: &Grid, points: &[Complex<f32>], labels: &[u32]) -> Option<Self> {
        let side = grid.side();
        let base = labels[grid.point_at(0, 0)];
        let column = |col: usize| labels[grid.point_at(col, 0)] ^ base;
        let row = |r: usize| labels[grid.point_at(0, r)] ^ base;
        let columns = Axis::along(side, column, |col| points[grid.point_at(col, 0)].re);
        let rows = Axis::along(side, row, |r| points[grid.point_at(0, r)].im);
        if columns.mask & rows.mask != 0 {
            return None;
        }
        let consistent = (0..side).all(|r| {
            (0..side).all(|col| labels[grid.point_at(col, r)] == base ^ column(col) ^ row(r))
        });
        consistent.then_some(Self {
            base,
            columns,
            rows,
        })
    }

    pub(super) fn max_log(&self, y: Complex<f32>, noise_var: f64, out: &mut [Llr]) {
        for (axis, at) in [(&self.columns, y.re), (&self.rows, y.im)] {
            axis.max_log(self.base, f64::from(at), noise_var, out);
        }
        self.saturate_fixed_bits(out);
    }

    pub(super) fn exact(&self, y: Complex<f32>, noise_var: f64, out: &mut [Llr]) {
        for (axis, at) in [(&self.columns, y.re), (&self.rows, y.im)] {
            axis.exact(self.base, f64::from(at), noise_var, out);
        }
        self.saturate_fixed_bits(out);
    }

    /// A bit no axis moves is certain, as the full search would also find.
    fn saturate_fixed_bits(&self, out: &mut [Llr]) {
        let moved = self.columns.mask | self.rows.mask;
        for (k, slot) in out.iter_mut().enumerate() {
            if moved >> k & 1 == 0 {
                *slot = Llr(if self.base >> k & 1 == 1 {
                    f32::INFINITY
                } else {
                    f32::NEG_INFINITY
                });
            }
        }
    }
}

impl Axis {
    fn along(side: usize, label: impl Fn(usize) -> u32, level: impl Fn(usize) -> f32) -> Self {
        let labels: Vec<u32> = (0..side).map(label).collect();
        Self {
            levels: (0..side).map(|k| f64::from(level(k))).collect(),
            mask: labels.iter().fold(0, |mask, &l| mask | l),
            labels,
        }
    }

    fn max_log(&self, base: u32, at: f64, noise_var: f64, out: &mut [Llr]) {
        let mut min0 = [f64::INFINITY; 32];
        let mut min1 = [f64::INFINITY; 32];
        for (&level, &label) in self.levels.iter().zip(&self.labels) {
            let d2 = (at - level) * (at - level);
            let full = base ^ label;
            for_each_bit(self.mask, |k| {
                let side = if full >> k & 1 == 1 {
                    &mut min1
                } else {
                    &mut min0
                };
                side[k] = side[k].min(d2);
            });
        }
        for_each_bit(self.mask, |k| {
            out[k] = Llr(((min0[k] - min1[k]) / noise_var) as f32);
        });
    }

    fn exact(&self, base: u32, at: f64, noise_var: f64, out: &mut [Llr]) {
        let mut sum0 = [LogSum::EMPTY; 32];
        let mut sum1 = [LogSum::EMPTY; 32];
        for (&level, &label) in self.levels.iter().zip(&self.labels) {
            let exponent = -(at - level) * (at - level) / noise_var;
            let full = base ^ label;
            for_each_bit(self.mask, |k| {
                let side = if full >> k & 1 == 1 {
                    &mut sum1
                } else {
                    &mut sum0
                };
                side[k].add(exponent);
            });
        }
        for_each_bit(self.mask, |k| {
            out[k] = Llr((sum1[k].value() - sum0[k].value()) as f32);
        });
    }
}

fn for_each_bit(mask: u32, mut f: impl FnMut(usize)) {
    let mut rest = mask;
    while rest != 0 {
        f(rest.trailing_zeros() as usize);
        rest &= rest - 1;
    }
}
