use super::{BlePhy, access_address_bits};

pub(crate) const G0: u8 = 0b1111;
pub(crate) const G1: u8 = 0b1011;
pub(crate) const TERM_BITS: usize = 3;
pub(crate) const CI_BITS: usize = 2;
pub(crate) const S8_SYMBOLS: usize = 4;
pub(crate) const AA_BLOCKS: usize = 64;
pub(crate) const INDICATOR_BLOCKS: usize = (CI_BITS + TERM_BITS) * 2;
pub(crate) const PREAMBLE_PERIOD: [bool; 8] = [false, false, true, true, true, true, false, false];
pub(crate) const PREAMBLE_REPEATS: usize = 10;
const STATES: usize = 8;
const MAX_STEPS: usize = (super::MAX_PDU_BYTES + super::CRC_BYTES) * 8 + TERM_BITS + 64;

#[derive(Clone, Copy, Default)]
pub(crate) struct Encoder(u8);

impl Encoder {
    pub(crate) fn push(&mut self, bit: bool) -> [bool; 2] {
        let register = u8::from(bit) << 3 | self.0;
        self.0 = (self.0 >> 1) | u8::from(bit) << 2;
        [
            (register & G0).count_ones() & 1 == 1,
            (register & G1).count_ones() & 1 == 1,
        ]
    }
}

pub(crate) fn s8_pattern(coded: bool) -> [bool; S8_SYMBOLS] {
    if coded {
        [true, true, false, false]
    } else {
        [false, false, true, true]
    }
}

pub(crate) fn ci_bits(phy: BlePhy) -> [bool; CI_BITS] {
    [phy == BlePhy::CodedS2, false]
}

pub(crate) fn coded_access_address() -> ([bool; 2 * 32], Encoder) {
    let mut encoder = Encoder::default();
    let mut coded = [false; 2 * 32];
    for (pair, bit) in coded
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip(access_address_bits())
    {
        *pair = encoder.push(bit);
    }
    (coded, encoder)
}

pub(crate) fn indicator(after_access_address: Encoder, phy: BlePhy) -> [bool; INDICATOR_BLOCKS] {
    let mut encoder = after_access_address;
    let mut coded = [false; INDICATOR_BLOCKS];
    let bits = ci_bits(phy)
        .into_iter()
        .chain(std::iter::repeat_n(false, TERM_BITS));
    for (pair, bit) in coded.as_chunks_mut::<2>().0.iter_mut().zip(bits) {
        *pair = encoder.push(bit);
    }
    coded
}

pub(crate) struct Viterbi {
    metrics: [f32; STATES],
    next: [f32; STATES],
    decisions: Vec<u16>,
}

impl Viterbi {
    pub(crate) fn new() -> Self {
        Self {
            metrics: [0.0; STATES],
            next: [0.0; STATES],
            decisions: Vec::with_capacity(MAX_STEPS),
        }
    }

    pub(crate) fn decode(&mut self, soft: &[f32], terminated: bool, out: &mut Vec<bool>) {
        self.metrics = [f32::NEG_INFINITY; STATES];
        self.metrics[0] = 0.0;
        self.decisions.clear();
        for pair in soft.as_chunks::<2>().0 {
            self.step(pair[0], pair[1]);
        }
        let mut state = if terminated {
            0
        } else {
            (0..STATES)
                .max_by(|&a, &b| self.metrics[a].total_cmp(&self.metrics[b]))
                .unwrap_or(0)
        };
        let start = out.len();
        for &decision in self.decisions.iter().rev() {
            out.push(state >> 2 & 1 == 1);
            state = (state & 0b011) << 1 | usize::from(decision >> state & 1 == 1);
        }
        out[start..].reverse();
    }

    fn step(&mut self, first: f32, second: f32) {
        let mut decision = 0u16;
        for target in 0..STATES {
            let input = target >> 2 & 1;
            let mut best = f32::NEG_INFINITY;
            let mut choice = 0;
            for dropped in 0..2 {
                let state = (target << 1 & 0b110) | dropped;
                let previous = self.metrics[state];
                if previous == f32::NEG_INFINITY {
                    continue;
                }
                let register = (input << 3 | state) as u8;
                let branch = signed((register & G0).count_ones() & 1 == 1, first)
                    + signed((register & G1).count_ones() & 1 == 1, second);
                if previous + branch > best {
                    best = previous + branch;
                    choice = dropped;
                }
            }
            self.next[target] = best;
            decision |= (choice as u16) << target;
        }
        std::mem::swap(&mut self.metrics, &mut self.next);
        self.decisions.push(decision);
    }
}

fn signed(bit: bool, value: f32) -> f32 {
    if bit { value } else { -value }
}

#[cfg(test)]
mod tests {
    use super::{Encoder, Viterbi};

    fn encode(bits: &[bool]) -> Vec<f32> {
        let mut encoder = Encoder::default();
        bits.iter()
            .flat_map(|&bit| encoder.push(bit))
            .map(|bit| if bit { 1.0 } else { -1.0 })
            .collect()
    }

    #[test]
    fn a_terminated_block_decodes_through_errors() {
        let mut bits: Vec<bool> = (0..200).map(|k| (k * 37 + 11) % 7 < 3).collect();
        bits.extend([false; 3]);
        let mut soft = encode(&bits);
        for k in (5..soft.len()).step_by(23) {
            soft[k] = -soft[k];
        }
        let mut out = Vec::new();
        Viterbi::new().decode(&soft, true, &mut out);
        assert_eq!(out, bits);
    }
}
