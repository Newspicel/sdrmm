use std::ops::Range;

#[cfg(not(target_arch = "aarch64"))]
use super::detector::count_gt;
use super::detector::{Hit, is_hit, order_statistic, os_probe};
use super::{CLUTTER_FILL, Cfar, MAX_PAD};
use crate::radar::strongest::Strongest;
use crate::radar::threshold::{CfarStatistic, os_order};

#[derive(Clone, Copy, Default)]
struct PlaneRow {
    base: usize,
    clutter: bool,
    guarded: bool,
}

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

    pub(super) fn detect_plane_os_row(
        &mut self,
        power: &[f32],
        row: usize,
        out: &mut Vec<Hit>,
        keep: &mut Strongest,
    ) {
        let spec = self.spec;
        let CfarStatistic::Os { rank } = spec.stat else {
            return;
        };
        let pad = spec.pad();
        let row_count = 2 * pad + 1;
        let guard_doppler = spec.guard_doppler as i64;
        let mut plane_rows = [PlaneRow::default(); 2 * MAX_PAD + 1];
        for (slot, offset) in plane_rows[..row_count]
            .iter_mut()
            .zip(-(pad as i64)..=pad as i64)
        {
            let source = (row as i64 + offset).rem_euclid(self.rows as i64) as usize;
            *slot = PlaneRow {
                base: source * self.gates,
                clutter: spec.is_clutter(source, self.rows),
                guarded: offset.abs() <= guard_doppler,
            };
        }
        let rows = &plane_rows[..row_count];
        let mut unguarded = [0usize; 2 * MAX_PAD + 1];
        let mut unguarded_len = 0usize;
        let mut guarded = [0usize; 2 * MAX_PAD + 1];
        let mut guarded_len = 0usize;
        let mut clutter_unguarded = 0usize;
        let mut clutter_guarded = 0usize;
        for &r in rows {
            if r.clutter {
                if r.guarded {
                    clutter_guarded += 1;
                } else {
                    clutter_unguarded += 1;
                }
            } else if r.guarded {
                guarded[guarded_len] = r.base;
                guarded_len += 1;
            } else {
                unguarded[unguarded_len] = r.base;
                unguarded_len += 1;
            }
        }
        let outer_rows = row..row + row_count;
        let guard_rows = row + pad - spec.guard_doppler..row + pad + spec.guard_doppler + 1;
        let reach = spec.guard_range + spec.train_range;
        let width = (2 * reach + 1).min(self.gates);
        let guard_row_count = 2 * spec.guard_doppler + 1;
        let line = &power[row * self.gates..(row + 1) * self.gates];
        let interior_start = spec.min_gate.max(reach).min(self.gates);
        let interior_end = self.gates.saturating_sub(reach).max(interior_start);
        for (gate, &cell) in line
            .iter()
            .enumerate()
            .skip(spec.min_gate)
            .take(interior_start.saturating_sub(spec.min_gate))
        {
            self.detect_plane_os_edge_cell(
                &outer_rows,
                &guard_rows,
                row,
                gate,
                cell,
                reach,
                width,
                row_count,
                guard_row_count,
                out,
                keep,
            );
        }
        if interior_start < interior_end {
            let full_guard = 2 * spec.guard_range + 1;
            let count = row_count * width - guard_row_count * full_guard;
            if count > 0 {
                let max_gt = count - os_order(rank, count);
                let clutter_cells =
                    clutter_unguarded * width + clutter_guarded * (2 * spec.train_range);
                let unguarded_bases = &unguarded[..unguarded_len];
                let guarded_bases = &guarded[..guarded_len];
                assert!(
                    unguarded_bases
                        .iter()
                        .chain(guarded_bases)
                        .all(|base| base + self.gates <= power.len())
                );
                for (gate, &cell) in line[interior_start..interior_end].iter().enumerate() {
                    let gate = gate + interior_start;
                    if cell <= 0.0 || cell.is_nan() {
                        continue;
                    }
                    let probe = os_probe(cell, spec.alpha, spec.min_snr);
                    let outer_start = gate - reach;
                    let right_start = gate + spec.guard_range + 1;
                    let crowded = unsafe {
                        exceeds_gt_split(
                            power,
                            unguarded_bases,
                            guarded_bases,
                            clutter_cells,
                            outer_start,
                            width,
                            right_start,
                            spec.train_range,
                            probe,
                            max_gt,
                        )
                    };
                    if crowded {
                        continue;
                    }
                    let outer = outer_start..outer_start + width;
                    let left = outer_start..gate - spec.guard_range;
                    let right = right_start..outer_start + width;
                    self.gather_plane_rows(power, rows, &outer, &left, &right);
                    let noise = order_statistic(&mut self.scratch, rank);
                    if is_hit(cell, noise, spec.alpha, spec.min_snr) {
                        keep.offer(
                            out,
                            Hit {
                                row: row as u32,
                                gate: gate as u32,
                                power: cell,
                                noise,
                            },
                            |hit| hit.power,
                        );
                    }
                }
            }
        }
        for (gate, &cell) in line.iter().enumerate().skip(interior_end) {
            self.detect_plane_os_edge_cell(
                &outer_rows,
                &guard_rows,
                row,
                gate,
                cell,
                reach,
                width,
                row_count,
                guard_row_count,
                out,
                keep,
            );
        }
    }

    #[expect(clippy::too_many_arguments, reason = "edge cell shares row state")]
    fn detect_plane_os_edge_cell(
        &self,
        outer_rows: &Range<usize>,
        guard_rows: &Range<usize>,
        row: usize,
        gate: usize,
        cell: f32,
        reach: usize,
        width: usize,
        row_count: usize,
        guard_row_count: usize,
        out: &mut Vec<Hit>,
        keep: &mut Strongest,
    ) {
        let spec = self.spec;
        if cell <= 0.0 || cell.is_nan() {
            return;
        }
        let start = gate.saturating_sub(reach).min(self.gates - width);
        let outer = start..start + width;
        let guard = gate.saturating_sub(spec.guard_range).max(start)
            ..(gate + spec.guard_range + 1).min(outer.end);
        let count = row_count * outer.len() - guard_row_count * guard.len();
        if count == 0 {
            return;
        }
        let total = self.rect(outer_rows.clone(), outer) - self.rect(guard_rows.clone(), guard);
        let mean = (total / count as f64) as f32;
        if is_hit(cell, mean, spec.alpha_edge, spec.min_snr) {
            keep.offer(
                out,
                Hit {
                    row: row as u32,
                    gate: gate as u32,
                    power: cell,
                    noise: mean,
                },
                |hit| hit.power,
            );
        }
    }

    fn gather_plane_rows(
        &mut self,
        power: &[f32],
        rows: &[PlaneRow],
        outer: &Range<usize>,
        left: &Range<usize>,
        right: &Range<usize>,
    ) {
        self.scratch.clear();
        for &plane_row in rows {
            if plane_row.clutter {
                let cells = if plane_row.guarded {
                    left.len() + right.len()
                } else {
                    outer.len()
                };
                self.scratch
                    .extend(std::iter::repeat_n(CLUTTER_FILL, cells));
            } else if plane_row.guarded {
                self.scratch.extend_from_slice(
                    &power[plane_row.base + left.start..plane_row.base + left.end],
                );
                self.scratch.extend_from_slice(
                    &power[plane_row.base + right.start..plane_row.base + right.end],
                );
            } else {
                self.scratch.extend_from_slice(
                    &power[plane_row.base + outer.start..plane_row.base + outer.end],
                );
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

/// Whether more than `max_gt` training cells exceed `probe`.
///
/// # Safety
/// Every base must leave `outer_start + width` and `right_start + train_range` cells inside
/// `power`.
#[expect(clippy::too_many_arguments, reason = "hot inner OS-CFAR probe")]
#[inline]
unsafe fn exceeds_gt_split(
    power: &[f32],
    unguarded_bases: &[usize],
    guarded_bases: &[usize],
    clutter_cells: usize,
    outer_start: usize,
    width: usize,
    right_start: usize,
    train_range: usize,
    probe: f32,
    max_gt: usize,
) -> bool {
    let mut gt = if CLUTTER_FILL > probe {
        clutter_cells
    } else {
        0
    };
    if gt > max_gt {
        return true;
    }
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vaddvq_u32, vcgtq_f32, vdupq_n_f32, vdupq_n_u32, vld1q_f32, vsubq_u32,
        };
        let (quads_w, tail_w) = (width / 4, width & 3);
        let (quads_t, tail_t) = (train_range / 4, train_range & 3);
        let cells = power.as_ptr();
        unsafe {
            let limit = vdupq_n_f32(probe);
            let mut above = vdupq_n_u32(0);
            let mut seen = gt;
            for &base in unguarded_bases {
                let row = cells.add(base + outer_start);
                for quad in 0..quads_w {
                    above = vsubq_u32(above, vcgtq_f32(vld1q_f32(row.add(4 * quad)), limit));
                }
                for cell in 0..tail_w {
                    gt += usize::from(*row.add(4 * quads_w + cell) > probe);
                }
                seen += width;
                if seen > max_gt {
                    gt += vaddvq_u32(above) as usize;
                    if gt > max_gt {
                        return true;
                    }
                    above = vdupq_n_u32(0);
                    seen = gt;
                }
            }
            for &base in guarded_bases {
                let left = cells.add(base + outer_start);
                let right = cells.add(base + right_start);
                for quad in 0..quads_t {
                    above = vsubq_u32(above, vcgtq_f32(vld1q_f32(left.add(4 * quad)), limit));
                    above = vsubq_u32(above, vcgtq_f32(vld1q_f32(right.add(4 * quad)), limit));
                }
                for cell in 0..tail_t {
                    gt += usize::from(*left.add(4 * quads_t + cell) > probe);
                    gt += usize::from(*right.add(4 * quads_t + cell) > probe);
                }
                seen += 2 * train_range;
                if seen > max_gt {
                    gt += vaddvq_u32(above) as usize;
                    if gt > max_gt {
                        return true;
                    }
                    above = vdupq_n_u32(0);
                    seen = gt;
                }
            }
            gt += vaddvq_u32(above) as usize;
        }
        gt > max_gt
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        for &base in unguarded_bases {
            gt += count_gt(
                &power[base + outer_start..base + outer_start + width],
                probe,
            );
            if gt > max_gt {
                return true;
            }
        }
        for &base in guarded_bases {
            gt += count_gt(
                &power[base + outer_start..base + outer_start + train_range],
                probe,
            );
            gt += count_gt(
                &power[base + right_start..base + right_start + train_range],
                probe,
            );
            if gt > max_gt {
                return true;
            }
        }
        false
    }
}
