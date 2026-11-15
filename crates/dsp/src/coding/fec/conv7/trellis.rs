use super::{
    ConvCode, MEMORY, STATE_MASK, STATES,
    butterfly::{self, Signs},
};
use crate::coding::fec::conv::Soft;

const HALF_STATE_MASK: usize = STATE_MASK >> 1;
const BRANCH_PATTERNS: usize = 1 << super::MAX_OUTPUTS;
const NORMALIZE_EVERY: u16 = 256;

fn scalar_step(
    code: &ConvCode,
    metrics: &[i32; STATES],
    next: &mut [i32; STATES],
    symbol: &[Soft],
) -> u64 {
    let mut branches = [0i32; BRANCH_PATTERNS];
    branches[0] = -symbol.iter().map(|&value| i32::from(value)).sum::<i32>();
    for (bit, &value) in symbol.iter().enumerate() {
        for mask in 0..1 << bit {
            branches[mask | (1 << bit)] = branches[mask] + 2 * i32::from(value);
        }
    }
    let mut decisions = 0u64;
    for state in 0..STATES {
        let previous = (state & HALF_STATE_MASK) * 2;
        let first = metrics[previous].saturating_add(branches[usize::from(code.branch[state * 2])]);
        let second =
            metrics[previous + 1].saturating_add(branches[usize::from(code.branch[state * 2 + 1])]);
        next[state] = first.max(second);
        decisions |= u64::from(second > first) << state;
    }
    decisions
}

/// Every generator taps both ends of the register, so the two branches into a state pair carry
/// opposite correlations and one vector butterfly serves the whole trellis step.
#[inline(always)]
fn butterfly_step<const N: usize>(
    metrics: &[i32; STATES],
    next: &mut [i32; STATES],
    signs: &Signs,
    symbol: &[Soft],
) -> u64 {
    butterfly::step::<N>(
        metrics,
        next,
        signs,
        std::array::from_fn(|index| i32::from(symbol[index])),
    )
}

pub(super) struct Trellis {
    survivors: [[i32; STATES]; 2],
    front: usize,
    normalization_steps: u16,
}

impl Trellis {
    const FLOOR: i32 = -(1 << 24);
    const CEILING: i32 = 1 << 28;

    pub(super) fn new() -> Self {
        Self {
            survivors: [[Self::FLOOR; STATES]; 2],
            front: 0,
            normalization_steps: 0,
        }
    }

    pub(super) fn metrics(&self) -> &[i32; STATES] {
        &self.survivors[self.front]
    }

    pub(super) fn restart(&mut self) {
        self.survivors[self.front] = [Self::FLOOR; STATES];
        self.survivors[self.front][0] = 0;
    }

    pub(super) fn open(&mut self) {
        self.survivors[self.front] = [0; STATES];
    }

    /// The survivors alternate between the two halves rather than being copied back, because a
    /// 256-byte move per trellis step costs as much as the butterfly that produced it.
    pub(super) fn step(&mut self, code: &ConvCode, symbol: &[Soft]) -> u64 {
        let [even, odd] = &mut self.survivors;
        let (metrics, next) = if self.front == 0 {
            (&*even, odd)
        } else {
            (&*odd, even)
        };
        let decisions = match (&code.signs, symbol.len()) {
            (Some(signs), 2) => butterfly_step::<2>(metrics, next, signs, symbol),
            (Some(signs), 3) => butterfly_step::<3>(metrics, next, signs, symbol),
            (Some(signs), 4) => butterfly_step::<4>(metrics, next, signs, symbol),
            _ => scalar_step(code, metrics, next, symbol),
        };
        self.front ^= 1;
        decisions
    }

    pub(super) fn normalize(&mut self) {
        self.normalization_steps += 1;
        if self.normalization_steps < NORMALIZE_EVERY {
            return;
        }
        self.normalization_steps = 0;
        let peak = self.metrics().iter().copied().max().unwrap_or(0);
        if peak > Self::CEILING {
            for metric in &mut self.survivors[self.front] {
                *metric -= peak;
            }
        }
    }

    pub(super) fn best_state(&self) -> usize {
        let metrics = self.metrics();
        (0..STATES)
            .max_by_key(|&state| metrics[state])
            .unwrap_or_default()
    }
}

pub(super) fn trace(decisions: &[u64], from: usize, mut visit: impl FnMut(bool)) -> usize {
    let mut state = from;
    for &word in decisions.iter().rev() {
        visit(state >> (MEMORY - 1) == 1);
        state = (state & HALF_STATE_MASK) << 1 | usize::from(word >> state & 1 == 1);
    }
    state & STATE_MASK
}
