use std::ops::Range;

use super::detector::order_statistic;
use super::{CLUTTER_FILL, Cfar};
use crate::radar::threshold::CfarStatistic;

impl Cfar {
    pub(super) fn build_table(&mut self, power: &[f32]) {
        let (rows, gates) = (self.rows, self.gates);
        let pad = self.spec.pad();
        let stride = gates + 1;
        let padded = rows + 2 * pad;
        self.table[..stride].fill(0.0);
        for padded_row in 0..padded {
            let row = (padded_row as i64 - pad as i64).rem_euclid(rows as i64) as usize;
            let clutter = self.spec.is_clutter(row, rows);
            let line = &power[row * gates..(row + 1) * gates];
            let (above, here) = self.table.split_at_mut((padded_row + 1) * stride);
            let above = &above[padded_row * stride..];
            let here = &mut here[..stride];
            here[0] = 0.0;
            let mut running = 0.0f64;
            for gate in 0..gates {
                let value = if clutter { CLUTTER_FILL } else { line[gate] };
                running += f64::from(value);
                here[gate + 1] = above[gate + 1] + running;
            }
        }
    }

    fn rect(&self, rows: Range<usize>, gates: Range<usize>) -> f64 {
        let stride = self.gates + 1;
        let at = |row: usize, gate: usize| self.table[row * stride + gate];
        at(rows.end, gates.end) - at(rows.start, gates.end) - at(rows.end, gates.start)
            + at(rows.start, gates.start)
    }

    pub(super) fn plane_noise(&mut self, power: &[f32], row: usize, gate: usize) -> (f32, f32) {
        let spec = self.spec;
        let pad = spec.pad();
        let outer_rows = row..row + 2 * pad + 1;
        let guard_rows = row + pad - spec.guard_doppler..row + pad + spec.guard_doppler + 1;
        let reach = spec.guard_range + spec.train_range;
        let width = (2 * reach + 1).min(self.gates);
        let start = gate.saturating_sub(reach).min(self.gates - width);
        let outer = start..start + width;
        let guard = gate.saturating_sub(spec.guard_range).max(start)
            ..(gate + spec.guard_range + 1).min(outer.end);
        let count = outer_rows.len() * outer.len() - guard_rows.len() * guard.len();
        if count == 0 {
            return (0.0, spec.alpha);
        }
        let total = self.rect(outer_rows.clone(), outer.clone())
            - self.rect(guard_rows.clone(), guard.clone());
        let mean = (total / count as f64) as f32;
        let left = outer.start..guard.start;
        let right = guard.end..outer.end;
        let edge = left.len() < spec.train_range || right.len() < spec.train_range;
        match spec.stat {
            CfarStatistic::Ca => (mean, spec.alpha),
            _ if edge => (mean, spec.alpha_edge),
            CfarStatistic::Go => {
                let side = |gates: Range<usize>| {
                    (self.rect(outer_rows.clone(), gates.clone())
                        / (outer_rows.len() * gates.len()) as f64) as f32
                };
                (side(left.clone()).max(side(right.clone())), spec.alpha)
            }
            CfarStatistic::Os { rank } => {
                self.gather_plane(power, row, &outer, &guard);
                (order_statistic(&mut self.scratch, rank), spec.alpha)
            }
        }
    }

    fn gather_plane(
        &mut self,
        power: &[f32],
        row: usize,
        outer: &Range<usize>,
        guard: &Range<usize>,
    ) {
        let spec = self.spec;
        let pad = spec.pad() as i64;
        let guard_doppler = spec.guard_doppler as i64;
        self.scratch.clear();
        for offset in -pad..=pad {
            let source = (row as i64 + offset).rem_euclid(self.rows as i64) as usize;
            let clutter = spec.is_clutter(source, self.rows);
            let line = &power[source * self.gates..(source + 1) * self.gates];
            for gate in outer.clone() {
                if offset.abs() <= guard_doppler && guard.contains(&gate) {
                    continue;
                }
                self.scratch
                    .push(if clutter { CLUTTER_FILL } else { line[gate] });
            }
        }
    }
}
