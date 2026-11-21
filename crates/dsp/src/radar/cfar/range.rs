use super::Cfar;
use super::detector::{Hit, count_gt, is_hit, order_statistic, os_probe};
use crate::radar::strongest::Strongest;
use crate::radar::threshold::{CfarStatistic, os_order};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    start: usize,
    len: usize,
}

impl Span {
    const fn end(self) -> usize {
        self.start + self.len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Window {
    left: Span,
    right: Span,
    edge: bool,
}

impl Cfar {
    pub(super) fn detect_range_row(
        &mut self,
        line: &[f32],
        row: usize,
        out: &mut Vec<Hit>,
        keep: &mut Strongest,
    ) {
        match self.spec.stat {
            CfarStatistic::Ca => self.detect_range_ca(line, row, out, keep),
            CfarStatistic::Go => self.detect_range_go(line, row, out, keep),
            CfarStatistic::Os { rank } => self.detect_range_os(line, row, rank, out, keep),
        }
    }

    fn detect_range_ca(&self, line: &[f32], row: usize, out: &mut Vec<Hit>, keep: &mut Strongest) {
        let spec = self.spec;
        let gates = self.gates;
        let reach = spec.guard_range + spec.train_range;
        let interior_start = spec.min_gate.max(reach).min(gates);
        let interior_end = gates.saturating_sub(reach).max(interior_start);
        let prefix = &self.prefix;
        let inv_full = 1.0 / (2 * spec.train_range) as f64;
        let factor = spec.alpha.max(spec.min_snr);
        for (gate, &cell) in line
            .iter()
            .enumerate()
            .skip(spec.min_gate)
            .take(interior_start.saturating_sub(spec.min_gate))
        {
            self.detect_range_ca_edge(gate, cell, row, out, keep);
        }
        if interior_start < interior_end {
            let count = interior_end - interior_start;
            let cells = &line[interior_start..interior_end];
            let l_hi = &prefix[interior_start - spec.guard_range..interior_end - spec.guard_range];
            let l_lo = &prefix[interior_start - reach..interior_end - reach];
            let r_hi = &prefix[interior_start + reach + 1..=interior_end + reach];
            let r_lo =
                &prefix[interior_start + spec.guard_range + 1..=interior_end + spec.guard_range];
            for idx in 0..count {
                let cell = cells[idx];
                let noise = (((l_hi[idx] - l_lo[idx]) + (r_hi[idx] - r_lo[idx])) * inv_full) as f32;
                if noise > 0.0 && cell >= factor * noise && cell > spec.alpha * noise {
                    offer_hit(out, keep, row, interior_start + idx, cell, noise);
                }
            }
        }
        for (gate, &cell) in line.iter().enumerate().skip(interior_end) {
            self.detect_range_ca_edge(gate, cell, row, out, keep);
        }
    }

    fn detect_range_ca_edge(
        &self,
        gate: usize,
        cell: f32,
        row: usize,
        out: &mut Vec<Hit>,
        keep: &mut Strongest,
    ) {
        let spec = self.spec;
        let w = range_window(gate, self.gates, spec.guard_range, spec.train_range);
        let count = w.left.len + w.right.len;
        if count == 0 {
            return;
        }
        let prefix = &self.prefix;
        let total = (prefix[w.left.end()] - prefix[w.left.start])
            + (prefix[w.right.end()] - prefix[w.right.start]);
        let noise = total as f32 / count as f32;
        if is_hit(cell, noise, spec.alpha, spec.min_snr) {
            offer_hit(out, keep, row, gate, cell, noise);
        }
    }

    fn detect_range_go(&self, line: &[f32], row: usize, out: &mut Vec<Hit>, keep: &mut Strongest) {
        let spec = self.spec;
        let gates = self.gates;
        let prefix = &self.prefix;
        for (gate, &cell) in line.iter().enumerate().skip(spec.min_gate) {
            let w = range_window(gate, gates, spec.guard_range, spec.train_range);
            let count = w.left.len + w.right.len;
            if count == 0 {
                continue;
            }
            let left_sum = prefix[w.left.end()] - prefix[w.left.start];
            let right_sum = prefix[w.right.end()] - prefix[w.right.start];
            let (noise, factor) = if w.edge {
                (
                    (left_sum + right_sum) as f32 / count as f32,
                    spec.alpha_edge,
                )
            } else {
                let left = left_sum as f32 / w.left.len as f32;
                let right = right_sum as f32 / w.right.len as f32;
                (left.max(right), spec.alpha)
            };
            if is_hit(cell, noise, factor, spec.min_snr) {
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

    fn detect_range_os(
        &mut self,
        line: &[f32],
        row: usize,
        rank: f32,
        out: &mut Vec<Hit>,
        keep: &mut Strongest,
    ) {
        let spec = self.spec;
        let gates = self.gates;
        for (gate, &cell) in line.iter().enumerate().skip(spec.min_gate) {
            if cell <= 0.0 || cell.is_nan() {
                continue;
            }
            let w = range_window(gate, gates, spec.guard_range, spec.train_range);
            let count = w.left.len + w.right.len;
            if count == 0 {
                continue;
            }
            let (noise, factor) = if w.edge {
                let total = (self.prefix[w.left.end()] - self.prefix[w.left.start])
                    + (self.prefix[w.right.end()] - self.prefix[w.right.start]);
                (total as f32 / count as f32, spec.alpha_edge)
            } else {
                let probe = os_probe(cell, spec.alpha, spec.min_snr);
                let max_gt = count - os_order(rank, count);
                let gt = count_gt(&line[w.left.start..w.left.end()], probe)
                    + count_gt(&line[w.right.start..w.right.end()], probe);
                if gt > max_gt {
                    continue;
                }
                self.scratch.clear();
                self.scratch
                    .extend_from_slice(&line[w.left.start..w.left.end()]);
                self.scratch
                    .extend_from_slice(&line[w.right.start..w.right.end()]);
                (order_statistic(&mut self.scratch, rank), spec.alpha)
            };
            if is_hit(cell, noise, factor, spec.min_snr) {
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

fn range_window(gate: usize, gates: usize, guard: usize, train: usize) -> Window {
    let left_avail = gate.saturating_sub(guard);
    let right_first = gate + guard + 1;
    let right_avail = gates.saturating_sub(right_first);
    let mut left = train.min(left_avail);
    let mut right = train.min(right_avail);
    if left < train {
        right = right_avail.min(2 * train - left);
    }
    if right < train {
        left = left_avail.min(2 * train - right);
    }
    Window {
        left: Span {
            start: left_avail - left,
            len: left,
        },
        right: Span {
            start: right_first.min(gates),
            len: right,
        },
        edge: left < train || right < train,
    }
}

#[cold]
fn offer_hit(
    out: &mut Vec<Hit>,
    keep: &mut Strongest,
    row: usize,
    gate: usize,
    power: f32,
    noise: f32,
) {
    keep.offer(
        out,
        Hit {
            row: row as u32,
            gate: gate as u32,
            power,
            noise,
        },
        |hit| hit.power,
    );
}

pub(super) fn fill_prefix(prefix: &mut [f64], line: &[f32]) {
    prefix[0] = 0.0;
    let mut running = 0.0f64;
    for (slot, value) in prefix[1..].iter_mut().zip(line) {
        running += f64::from(*value);
        *slot = running;
    }
}
