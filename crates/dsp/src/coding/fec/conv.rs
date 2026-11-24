pub type Soft = i16;

pub const ERASURE: Soft = 0;

pub const CONFIDENT: Soft = 64;

const CONSTRAINT: u32 = 5;
const STATES: usize = 1 << (CONSTRAINT - 1);
const OLDEST_STATE_BIT: u32 = CONSTRAINT - 2;
const REGISTER_MASK: u8 = (1 << CONSTRAINT) - 1;
/// Where every state but the start begins: far below any reachable metric yet clear of
/// overflow when branches are added.
const UNREACHED_METRIC: i32 = i32::MIN / 4;
const MAX_OUTPUTS: usize = 4;
const G1: u8 = 0b1_1001;
const G2: u8 = 0b1_0111;

#[must_use]
pub fn soft(bit: bool) -> Soft {
    if bit { CONFIDENT } else { -CONFIDENT }
}

pub fn encode(bits: &[bool], out: &mut Vec<bool>) {
    encode_with(G1, G2, bits, out);
}

pub fn encode_with(g1: u8, g2: u8, bits: &[bool], out: &mut Vec<bool>) {
    encode_polys(&[g1, g2], bits, out);
}

/// Rate 1/n K=5 encoding: one output per generator, newest input in the low bit.
pub fn encode_polys(polys: &[u8], bits: &[bool], out: &mut Vec<bool>) {
    let mut reg = 0u8;
    for &bit in bits {
        reg = (reg << 1 | u8::from(bit)) & REGISTER_MASK;
        out.extend(polys.iter().map(|&g| (reg & g).count_ones() % 2 == 1));
    }
}

/// Soft Viterbi for rate 1/n K=5 codes, n up to four.
#[derive(Clone, Debug)]
pub struct Viterbi5 {
    polys: [u8; MAX_OUTPUTS],
    outputs: usize,
    decisions: Vec<u16>,
    forward: Vec<[i32; STATES]>,
    metrics: [i32; STATES],
    next: [i32; STATES],
}

impl Default for Viterbi5 {
    fn default() -> Self {
        Self::with_generators(G1, G2)
    }
}

impl Viterbi5 {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_generators(g1: u8, g2: u8) -> Self {
        Self::with_polys(&[g1, g2])
    }

    #[must_use]
    pub fn with_polys(polys: &[u8]) -> Self {
        assert!(
            (1..=MAX_OUTPUTS).contains(&polys.len()),
            "one to four generators"
        );
        let mut masked = [0u8; MAX_OUTPUTS];
        for (slot, &g) in masked.iter_mut().zip(polys) {
            *slot = g & REGISTER_MASK;
        }
        Self {
            polys: masked,
            outputs: polys.len(),
            decisions: Vec::new(),
            forward: Vec::new(),
            metrics: [0; STATES],
            next: [0; STATES],
        }
    }

    /// Decodes and traces back from the best final state.
    pub fn decode(&mut self, coded: &[Soft], out: &mut Vec<bool>) -> i32 {
        self.run(coded, out, None)
    }

    /// Decodes a block flushed by a zero tail, tracing back from state 0.
    pub fn decode_terminated(&mut self, coded: &[Soft], out: &mut Vec<bool>) -> i32 {
        self.run(coded, out, Some(0))
    }

    /// Max-log MAP over the same trellis, free at the end like [`Self::decode`]: one soft value a
    /// bit, its sign the decision [`Self::decode`] makes and its size half the metric the best
    /// path with the other bit loses by, so a lone channel bit at `v` would weigh the same.
    /// Clipped to [`CONFIDENT`]; a bit both paths tie on is an erasure. Returns the best metric.
    pub fn decode_soft_output(&mut self, coded: &[Soft], out: &mut Vec<Soft>) -> i32 {
        let steps = self.steps(coded);
        self.forward.clear();
        self.forward.resize(steps + 1, [UNREACHED_METRIC; STATES]);
        self.forward[0][0] = 0;
        for (step, received) in coded.chunks_exact(self.outputs).enumerate() {
            let mut next = [UNREACHED_METRIC; STATES];
            for (state, slot) in next.iter_mut().enumerate() {
                for prev_high in [false, true] {
                    let (prev, reg) = predecessor(state, prev_high);
                    let metric =
                        self.forward[step][prev].saturating_add(self.branch(reg, received));
                    *slot = (*slot).max(metric);
                }
            }
            self.forward[step + 1] = next;
        }
        let start = out.len();
        out.resize(start + steps, ERASURE);
        let mut backward = [0i32; STATES];
        for (step, received) in coded.chunks_exact(self.outputs).enumerate().rev() {
            let mut best = [UNREACHED_METRIC; 2];
            let mut earlier = [UNREACHED_METRIC; STATES];
            for state in 0..STATES {
                for prev_high in [false, true] {
                    let (prev, reg) = predecessor(state, prev_high);
                    let through = backward[state].saturating_add(self.branch(reg, received));
                    earlier[prev] = earlier[prev].max(through);
                    let path = self.forward[step][prev].saturating_add(through);
                    best[state & 1] = best[state & 1].max(path);
                }
            }
            out[start + step] = reliability(best[1].saturating_sub(best[0]));
            backward = earlier;
        }
        self.forward[steps]
            .iter()
            .copied()
            .max()
            .unwrap_or(UNREACHED_METRIC)
    }

    fn steps(&self, coded: &[Soft]) -> usize {
        assert!(
            coded.len().is_multiple_of(self.outputs),
            "the code needs whole symbols"
        );
        coded.len() / self.outputs
    }

    fn branch(&self, reg: u8, received: &[Soft]) -> i32 {
        self.polys[..self.outputs]
            .iter()
            .zip(received)
            .map(|(&g, &r)| {
                let r = i32::from(r);
                if (reg & g).count_ones() % 2 == 1 {
                    r
                } else {
                    -r
                }
            })
            .sum()
    }

    fn run(&mut self, coded: &[Soft], out: &mut Vec<bool>, end: Option<usize>) -> i32 {
        let steps = self.steps(coded);
        self.decisions.clear();
        self.decisions.resize(steps, 0);
        self.metrics = [UNREACHED_METRIC; STATES];
        self.metrics[0] = 0;
        for (step, received) in coded.chunks_exact(self.outputs).enumerate() {
            let mut decisions = 0u16;
            for state in 0..STATES {
                let mut best = (i32::MIN, false);
                for prev_high in [false, true] {
                    let (prev, reg) = predecessor(state, prev_high);
                    let metric = self.metrics[prev].saturating_add(self.branch(reg, received));
                    if metric > best.0 {
                        best = (metric, prev_high);
                    }
                }
                self.next[state] = best.0;
                decisions |= u16::from(best.1) << state;
            }
            self.decisions[step] = decisions;
            self.metrics = self.next;
        }
        let mut state = end.unwrap_or_else(|| {
            (0..STATES)
                .max_by_key(|&s| self.metrics[s])
                .unwrap_or_default()
        });
        let metric = self.metrics[state];
        let start = out.len();
        for step in (0..steps).rev() {
            out.push(state & 1 == 1);
            let prev_high = self.decisions[step] >> state & 1 == 1;
            state = state >> 1 | usize::from(prev_high) << OLDEST_STATE_BIT;
        }
        out[start..].reverse();
        metric
    }
}

/// The state before `state` whose oldest bit is `high`, and the register the step between
/// them holds.
const fn predecessor(state: usize, high: bool) -> (usize, u8) {
    let prev = state >> 1 | (high as usize) << OLDEST_STATE_BIT;
    (prev, (prev << 1 | state & 1) as u8)
}

/// A path metric gap as the soft value of one channel bit, rounded away from zero so only a
/// tie erases.
fn reliability(gap: i32) -> Soft {
    let half = gap.saturating_add(gap.signum()) / 2;
    half.clamp(-i32::from(CONFIDENT), i32::from(CONFIDENT)) as Soft
}

#[cfg(test)]
mod tests {
    use super::{super::test_data::random_bits as message, *};
    use crate::math::rng::Rng;

    fn softs(bits: &[bool]) -> Vec<Soft> {
        bits.iter().copied().map(soft).collect()
    }

    #[test]
    fn round_trips_a_flushed_frame() {
        let mut bits = message(96, 5);
        bits.extend([false; 4]);
        let mut coded = Vec::new();
        encode(&bits, &mut coded);
        assert_eq!(coded.len(), bits.len() * 2);

        let mut out = Vec::new();
        Viterbi5::new().decode(&softs(&coded), &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn round_trips_other_generators() {
        let mut bits = message(224, 11);
        bits.extend([false; 4]);
        let mut coded = Vec::new();
        encode_with(0b1_1001, 0b1_1011, &bits, &mut coded);
        let mut received = softs(&coded);
        for bit in [7usize, 100, 300] {
            received[bit] = -received[bit];
        }
        let mut out = Vec::new();
        Viterbi5::with_generators(0b1_1001, 0b1_1011).decode(&received, &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn a_rate_one_quarter_terminated_block_repairs_errors() {
        let polys = [0b1_0011, 0b1_1101, 0b1_0111, 0b1_1011];
        let mut bits = message(120, 17);
        bits.extend([false; 4]);
        let mut coded = Vec::new();
        encode_polys(&polys, &bits, &mut coded);
        assert_eq!(coded.len(), bits.len() * 4);
        let mut received = softs(&coded);
        for bit in [5usize, 6, 7, 200, 333, 460] {
            received[bit] = -received[bit];
        }
        for bit in (1..received.len()).step_by(3) {
            received[bit] = ERASURE;
        }
        let mut out = Vec::new();
        Viterbi5::with_polys(&polys).decode_terminated(&received, &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn a_terminated_decode_ends_in_state_zero() {
        let mut coded = Vec::new();
        encode(&[true, true, false, true], &mut coded);
        let mut free = Vec::new();
        Viterbi5::new().decode(&softs(&coded), &mut free);
        assert_eq!(free, [true, true, false, true]);
        let mut ended = Vec::new();
        Viterbi5::new().decode_terminated(&softs(&coded), &mut ended);
        assert_eq!(ended.len(), 4);
        assert!(!ended[3], "the last bit must leave the register at zero");
    }

    #[test]
    fn repairs_scattered_channel_errors() {
        let mut bits = message(72, 9);
        bits.extend([false; 4]);
        let mut coded = Vec::new();
        encode(&bits, &mut coded);
        let mut received = softs(&coded);
        for bit in [3usize, 40, 41, 90, 130] {
            received[bit] = -received[bit];
        }
        let mut out = Vec::new();
        Viterbi5::new().decode(&received, &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn erasures_stand_in_for_punctured_bits() {
        let mut bits = message(48, 3);
        bits.extend([false; 4]);
        let mut coded = Vec::new();
        encode(&bits, &mut coded);
        let mut received = softs(&coded);
        for (i, value) in received.iter_mut().enumerate() {
            if i % 8 == 7 {
                *value = ERASURE;
            }
        }
        let mut out = Vec::new();
        Viterbi5::new().decode(&received, &mut out);
        assert_eq!(out, bits);
    }

    #[test]
    fn weak_bits_lose_to_confident_ones() {
        let mut bits = message(64, 21);
        bits.extend([false; 4]);
        let mut coded = Vec::new();
        encode(&bits, &mut coded);
        let mut received = softs(&coded);
        for bit in [10usize, 11, 12, 13] {
            received[bit] = if received[bit] > 0 { -1 } else { 1 };
        }
        let mut out = Vec::new();
        Viterbi5::new().decode(&received, &mut out);
        assert_eq!(out, bits);
    }

    fn noisy(coded: &[bool], sigma: f64, rng: &mut Rng) -> Vec<Soft> {
        let limit = f64::from(CONFIDENT);
        coded
            .iter()
            .map(|&bit| {
                let clean = if bit { limit } else { -limit };
                (clean + sigma * limit * rng.normal()).clamp(-limit, limit) as Soft
            })
            .collect()
    }

    fn coded_block(seed: u64) -> (Vec<bool>, Vec<bool>) {
        let mut bits = message(96, seed);
        bits.extend([false; 4]);
        let mut coded = Vec::new();
        encode(&bits, &mut coded);
        (bits, coded)
    }

    #[test]
    fn soft_output_decides_as_the_viterbi_decoder() {
        let mut rng = Rng::mixed(7);
        let mut decoder = Viterbi5::new();
        let mut ties = 0;
        for seed in 0..40 {
            let (_, coded) = coded_block(seed);
            let received = noisy(&coded, 0.9, &mut rng);
            let mut hard = Vec::new();
            let metric = decoder.decode(&received, &mut hard);
            let mut reliable = Vec::new();
            assert_eq!(decoder.decode_soft_output(&received, &mut reliable), metric);
            for (&value, &bit) in reliable.iter().zip(&hard) {
                if value == ERASURE {
                    ties += 1;
                } else {
                    assert_eq!(value > 0, bit, "seed {seed}");
                }
            }
        }
        assert!(ties < 10, "{ties} ties");
    }

    #[test]
    fn a_clean_block_is_confident_throughout() {
        let (bits, coded) = coded_block(3);
        let mut reliable = Vec::new();
        Viterbi5::new().decode_soft_output(&softs(&coded), &mut reliable);
        let expected: Vec<Soft> = bits.iter().copied().map(soft).collect();
        assert_eq!(reliable, expected);
    }

    #[test]
    fn errors_sit_where_reliability_is_low() {
        let mut rng = Rng::mixed(11);
        let mut decoder = Viterbi5::new();
        let mut ranked = Vec::new();
        for seed in 0..300 {
            let (bits, coded) = coded_block(seed);
            let mut reliable = Vec::new();
            decoder.decode_soft_output(&noisy(&coded, 0.7, &mut rng), &mut reliable);
            ranked.extend(
                reliable
                    .iter()
                    .zip(&bits)
                    .map(|(&value, &bit)| (value.abs(), (value > 0) != bit)),
            );
        }
        ranked.sort_by_key(|&(magnitude, _)| magnitude);
        let decile = ranked.len() / 10;
        let errors = |slice: &[(Soft, bool)]| slice.iter().filter(|(_, wrong)| *wrong).count();
        let weakest = errors(&ranked[..decile]);
        let surest = errors(&ranked[ranked.len() - decile..]);
        let total = errors(&ranked);
        assert!(total > 100, "{total} errors");
        assert!(weakest * 2 > total, "weakest decile {weakest} of {total}");
        assert!(surest * 50 < weakest, "surest {surest}, weakest {weakest}");
    }

    #[test]
    fn soft_output_reuses_its_buffers() {
        let (_, coded) = coded_block(5);
        let received = softs(&coded);
        let mut decoder = Viterbi5::new();
        let mut reliable = Vec::with_capacity(100);
        decoder.decode_soft_output(&received, &mut reliable);
        let (forward, capacity) = (decoder.forward.as_ptr(), decoder.forward.capacity());
        let out = reliable.as_ptr();
        reliable.clear();
        decoder.decode_soft_output(&received, &mut reliable);
        assert_eq!(decoder.forward.as_ptr(), forward);
        assert_eq!(decoder.forward.capacity(), capacity);
        assert_eq!(reliable.as_ptr(), out);
    }
}
