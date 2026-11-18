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
        assert!(
            coded.len().is_multiple_of(self.outputs),
            "the code needs whole symbols"
        );
        let steps = coded.len() / self.outputs;
        self.decisions.clear();
        self.decisions.resize(steps, 0);
        self.metrics = [UNREACHED_METRIC; STATES];
        self.metrics[0] = 0;
        for (step, received) in coded.chunks_exact(self.outputs).enumerate() {
            let mut decisions = 0u16;
            for state in 0..STATES {
                let mut best = (i32::MIN, false);
                for prev_high in [false, true] {
                    let prev = state >> 1 | usize::from(prev_high) << OLDEST_STATE_BIT;
                    let reg = (prev << 1 | state & 1) as u8;
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

#[cfg(test)]
mod tests {
    use super::{super::test_data::random_bits as message, *};

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
}
