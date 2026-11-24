use super::conv::Soft;

/// Where every state but the start begins: far below any reachable metric yet clear of
/// overflow when branches are added.
const UNREACHED: i32 = i32::MIN / 4;

/// A small trellis given as tables: from `state` the input `i` moves to `next[state][i]` and
/// sends the `N` channel symbols `output[state][i]`, as in trellis coded modulation.
/// Decoding is Viterbi, hard on symbol mismatches or soft on bit correlations, terminated in a
/// known state.
#[derive(Clone, Copy, Debug)]
pub struct TrellisCode<const S: usize, const I: usize, const N: usize> {
    next: [[u8; I]; S],
    output: [[[u8; N]; I]; S],
}

impl<const S: usize, const I: usize, const N: usize> TrellisCode<S, I, N> {
    #[must_use]
    pub const fn new(next: [[u8; I]; S], output: [[[u8; N]; I]; S]) -> Self {
        assert!(S <= 256 && I <= 256, "states and inputs fit a byte");
        Self { next, output }
    }

    /// A trellis whose next state is the input just sent.
    #[must_use]
    pub const fn input_state(output: [[[u8; N]; I]; S]) -> Self {
        let mut next = [[0u8; I]; S];
        let mut state = 0;
        while state < S {
            let mut input = 0;
            while input < I {
                next[state][input] = input as u8;
                input += 1;
            }
            state += 1;
        }
        Self::new(next, output)
    }

    /// Writes the symbols of `inputs` sent from state 0.
    pub fn encode(&self, inputs: &[u8], out: &mut [[u8; N]]) {
        let mut state = 0usize;
        for (&input, slot) in inputs.iter().zip(out) {
            let input = usize::from(input);
            *slot = self.output[state][input];
            state = usize::from(self.next[state][input]);
        }
    }

    /// The inputs of the path from state 0 to `end` closest to `received`, with its distance.
    pub fn decode<const STEPS: usize>(
        &self,
        received: &[[u8; N]; STEPS],
        end: usize,
        inputs: &mut [u8; STEPS],
    ) -> u16 {
        let score = self.search(received, end, inputs, |sent, observed| {
            -i32::from(distance(sent, observed))
        });
        u16::try_from(-score).unwrap_or(u16::MAX)
    }

    /// As [`Self::decode`] from soft bits: each symbol is `B` values, its most significant bit
    /// first, positive for a one. Branches score the correlation of those values with the bits
    /// they send, so a weak bit costs little to overturn. Returns the decisions the path
    /// overturns.
    pub fn decode_soft<const B: usize, const STEPS: usize>(
        &self,
        received: &[[[Soft; B]; N]; STEPS],
        end: usize,
        inputs: &mut [u8; STEPS],
    ) -> u16 {
        const { assert!(B <= 8, "a symbol fits a byte") };
        self.search(received, end, inputs, correlation);
        self.overturned(received, inputs)
    }

    fn search<R, const STEPS: usize>(
        &self,
        received: &[R; STEPS],
        end: usize,
        inputs: &mut [u8; STEPS],
        branch: impl Fn(&[u8; N], &R) -> i32,
    ) -> i32 {
        let mut metric = [UNREACHED; S];
        metric[0] = 0;
        let mut history = [[(0u8, 0u8); S]; STEPS];
        for (observed, survivors) in received.iter().zip(history.iter_mut()) {
            let mut next = [UNREACHED; S];
            for (state, &score) in metric.iter().enumerate() {
                if score == UNREACHED {
                    continue;
                }
                for input in 0..I {
                    let target = usize::from(self.next[state][input]);
                    let candidate = score + branch(&self.output[state][input], observed);
                    if candidate > next[target] {
                        next[target] = candidate;
                        survivors[target] = (state as u8, input as u8);
                    }
                }
            }
            metric = next;
        }
        let mut state = end;
        for (slot, survivors) in inputs.iter_mut().zip(&history).rev() {
            let (previous, input) = survivors[state];
            *slot = input;
            state = usize::from(previous);
        }
        metric[end]
    }

    fn overturned<const B: usize, const STEPS: usize>(
        &self,
        received: &[[[Soft; B]; N]; STEPS],
        inputs: &[u8; STEPS],
    ) -> u16 {
        let mut state = 0usize;
        let mut count = 0u16;
        for (&input, observed) in inputs.iter().zip(received) {
            let input = usize::from(input);
            for (&symbol, values) in self.output[state][input].iter().zip(observed) {
                for (place, &value) in values.iter().enumerate() {
                    let sent = symbol >> (B - 1 - place) & 1 == 1;
                    count = count.saturating_add(u16::from(value != 0 && (value > 0) != sent));
                }
            }
            state = usize::from(self.next[state][input]);
        }
        count
    }
}

fn correlation<const B: usize, const N: usize>(sent: &[u8; N], observed: &[[Soft; B]; N]) -> i32 {
    let mut sum = 0;
    for (&symbol, values) in sent.iter().zip(observed) {
        for (place, &value) in values.iter().enumerate() {
            let value = i32::from(value);
            sum += if symbol >> (B - 1 - place) & 1 == 1 {
                value
            } else {
                -value
            };
        }
    }
    sum
}

fn distance<const N: usize>(sent: &[u8; N], observed: &[u8; N]) -> u16 {
    sent.iter()
        .zip(observed)
        .map(|(a, b)| u16::from(a != b))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding::fec::conv::{CONFIDENT, soft};

    const HALF_RATE: TrellisCode<4, 4, 2> = TrellisCode::input_state([
        [[0, 2], [3, 0], [0, 1], [3, 3]],
        [[3, 2], [0, 0], [3, 1], [0, 3]],
        [[2, 1], [1, 3], [2, 2], [1, 0]],
        [[1, 1], [2, 3], [1, 2], [2, 0]],
    ]);

    const SHIFT: TrellisCode<2, 2, 2> =
        TrellisCode::new([[0, 1], [0, 1]], [[[0, 0], [1, 1]], [[1, 0], [0, 1]]]);

    fn message(len: usize, alphabet: u8) -> Vec<u8> {
        (0..len).map(|k| (k * 7 + k / 3) as u8 % alphabet).collect()
    }

    #[test]
    fn a_clean_terminated_block_round_trips() {
        let mut inputs = message(48, 4);
        inputs.push(0);
        let mut sent = [[0u8; 2]; 49];
        HALF_RATE.encode(&inputs, &mut sent);
        let mut decoded = [0u8; 49];
        assert_eq!(HALF_RATE.decode(&sent, 0, &mut decoded), 0);
        assert_eq!(decoded[..], inputs[..]);
    }

    #[test]
    fn scattered_symbol_errors_are_corrected() {
        let mut inputs = message(48, 4);
        inputs.push(0);
        let mut sent = [[0u8; 2]; 49];
        HALF_RATE.encode(&inputs, &mut sent);
        for step in [3usize, 17, 30, 44] {
            sent[step][1] ^= 1;
        }
        let mut decoded = [0u8; 49];
        assert_eq!(HALF_RATE.decode(&sent, 0, &mut decoded), 4);
        assert_eq!(decoded[..], inputs[..]);
    }

    #[test]
    fn an_explicit_next_state_table_decodes() {
        let mut inputs = message(30, 2);
        inputs.push(0);
        let mut sent = [[0u8; 2]; 31];
        SHIFT.encode(&inputs, &mut sent);
        sent[12][0] ^= 1;
        let mut decoded = [0u8; 31];
        SHIFT.decode(&sent, 0, &mut decoded);
        assert_eq!(decoded[..], inputs[..]);
    }

    fn soften<const STEPS: usize>(sent: &[[u8; 2]; STEPS]) -> [[[Soft; 2]; 2]; STEPS] {
        sent.map(|step| step.map(|symbol| [soft(symbol & 2 != 0), soft(symbol & 1 != 0)]))
    }

    fn sent_message() -> ([u8; 49], [[u8; 2]; 49]) {
        let mut inputs = [0u8; 49];
        inputs[..48].copy_from_slice(&message(48, 4));
        let mut sent = [[0u8; 2]; 49];
        HALF_RATE.encode(&inputs, &mut sent);
        (inputs, sent)
    }

    #[test]
    fn soft_decoding_of_hard_input_matches_the_hard_decoder() {
        let (inputs, mut sent) = sent_message();
        for step in [3usize, 17, 30, 44] {
            sent[step][1] ^= 1;
        }
        let mut hard = [0u8; 49];
        let mut soft_decoded = [0u8; 49];
        assert_eq!(HALF_RATE.decode(&sent, 0, &mut hard), 4);
        assert_eq!(
            HALF_RATE.decode_soft(&soften(&sent), 0, &mut soft_decoded),
            4
        );
        assert_eq!(soft_decoded, hard);
        assert_eq!(soft_decoded, inputs);
    }

    #[test]
    fn a_run_of_weak_errors_the_hard_decoder_misses_is_corrected() {
        let (inputs, sent) = sent_message();
        let mut received = soften(&sent);
        let mut hard = sent;
        for step in 18..26 {
            let (symbol, bit) = (step % 2, step / 2 % 2);
            hard[step][symbol] ^= 2 >> bit;
            received[step][symbol][bit] = -received[step][symbol][bit] / 4;
        }
        let mut decoded = [0u8; 49];
        HALF_RATE.decode(&hard, 0, &mut decoded);
        assert_ne!(decoded, inputs);
        assert_eq!(HALF_RATE.decode_soft(&received, 0, &mut decoded), 8);
        assert_eq!(decoded, inputs);
    }

    #[test]
    fn erased_symbols_are_bridged_and_overturn_nothing() {
        let (inputs, sent) = sent_message();
        let mut received = soften(&sent);
        for step in [5usize, 6, 25, 40] {
            received[step] = [[0; 2]; 2];
        }
        received[12][0][1] = -received[12][0][1];
        let mut decoded = [0u8; 49];
        assert_eq!(HALF_RATE.decode_soft(&received, 0, &mut decoded), 1);
        assert_eq!(decoded, inputs);
        assert_eq!(received[0][0][0].abs(), CONFIDENT);
    }
}
