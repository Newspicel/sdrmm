use super::Cfar;
use super::detector::order_statistic;
use crate::radar::threshold::CfarStatistic;

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
    pub(super) fn range_noise(&mut self, line: &[f32], gate: usize) -> (f32, f32) {
        let spec = self.spec;
        let window = range_window(gate, self.gates, spec.guard_range, spec.train_range);
        let sum = |span: Span| self.prefix[span.end()] - self.prefix[span.start];
        let total = (sum(window.left) + sum(window.right)) as f32;
        let count = window.left.len + window.right.len;
        if count == 0 {
            return (0.0, spec.alpha);
        }
        let mean = total / count as f32;
        match spec.stat {
            CfarStatistic::Ca => (mean, spec.alpha),
            _ if window.edge => (mean, spec.alpha_edge),
            CfarStatistic::Go => {
                let left = sum(window.left) as f32 / window.left.len as f32;
                let right = sum(window.right) as f32 / window.right.len as f32;
                (left.max(right), spec.alpha)
            }
            CfarStatistic::Os { rank } => {
                self.scratch.clear();
                for span in [window.left, window.right] {
                    self.scratch
                        .extend_from_slice(&line[span.start..span.end()]);
                }
                (order_statistic(&mut self.scratch, rank), spec.alpha)
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

pub(super) fn fill_prefix(prefix: &mut [f64], line: &[f32]) {
    prefix[0] = 0.0;
    let mut running = 0.0f64;
    for (slot, value) in prefix[1..].iter_mut().zip(line) {
        running += f64::from(*value);
        *slot = running;
    }
}
