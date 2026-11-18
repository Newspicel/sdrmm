/// A small trellis given as tables: from `state` the input `i` moves to `next[state][i]` and
/// sends the `N` channel symbols `output[state][i]`, as in trellis coded modulation.
/// Decoding is hard decision Viterbi on symbol mismatches, terminated in a known state.
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
        let mut metric = [u16::MAX; S];
        metric[0] = 0;
        let mut history = [[(0u8, 0u8); S]; STEPS];
        for (observed, survivors) in received.iter().zip(history.iter_mut()) {
            let mut next = [u16::MAX; S];
            for (state, &cost) in metric.iter().enumerate() {
                if cost == u16::MAX {
                    continue;
                }
                for input in 0..I {
                    let target = usize::from(self.next[state][input]);
                    let candidate = cost + distance(&self.output[state][input], observed);
                    if candidate < next[target] {
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
}
