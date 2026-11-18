/// Soft Viterbi decoding of a short tail-biting rate 1/n convolutional code.
///
/// A tail-biting encoder starts in the state its own last bits leave, so the decoder runs
/// once from each of the `STATES` start states and keeps the path that comes back to where
/// it began with the best metric. Words up to `MAX_BITS` long decode on the stack.
///
/// The shift register is `log2(STATES) + 1` bits, the newest bit highest. Output bit `j` of a
/// step is the parity of the register masked by `polynomials[j]`. Soft values are positive
/// for a 0.
#[derive(Clone, Copy, Debug)]
pub struct TailBitingViterbi<'a, const STATES: usize, const MAX_BITS: usize> {
    polynomials: &'a [u8],
}

impl<'a, const STATES: usize, const MAX_BITS: usize> TailBitingViterbi<'a, STATES, MAX_BITS> {
    const MEMORY: u32 = STATES.trailing_zeros();

    /// # Panics
    /// When `STATES` is not a power of two from 2 to 128.
    #[must_use]
    pub const fn new(polynomials: &'a [u8]) -> Self {
        assert!(
            STATES.is_power_of_two() && STATES >= 2 && STATES <= 128,
            "the register must fit a byte"
        );
        Self { polynomials }
    }

    const fn output(&self, register: u8) -> u8 {
        let mut word = 0u8;
        let mut index = 0;
        while index < self.polynomials.len() {
            word = word << 1 | ((register & self.polynomials[index]).count_ones() % 2) as u8;
            index += 1;
        }
        word
    }

    /// Encodes `bits` as one tail-biting block into `out`, `n` bits per input bit.
    pub fn encode(&self, bits: &[bool], out: &mut [bool]) {
        let count = bits.len();
        let n = self.polynomials.len();
        for (i, step) in out.chunks_exact_mut(n).take(count).enumerate() {
            let register = (0..=Self::MEMORY as usize).fold(0u8, |word, delay| {
                word << 1 | u8::from(bits[(i + count - delay) % count])
            });
            let word = self.output(register);
            for (j, bit) in step.iter_mut().enumerate() {
                *bit = word >> (n - 1 - j) & 1 == 1;
            }
        }
    }

    /// Decodes `bits.len()` bits, at most `MAX_BITS`, from `n` soft values each. Returns the
    /// winning path's metric: the sum of the soft values, each signed by the bit the path
    /// expects.
    pub fn decode(&self, soft: &[f32], bits: &mut [bool]) -> f32 {
        let n = self.polynomials.len();
        let count = bits.len().min(MAX_BITS).min(soft.len() / n.max(1));
        let mut best = f32::NEG_INFINITY;
        let mut trial = [false; MAX_BITS];
        for start in 0..STATES {
            let metric = self.run(soft, count, start, &mut trial);
            if metric > best {
                best = metric;
                bits[..count].copy_from_slice(&trial[..count]);
            }
        }
        best
    }

    fn branch(&self, soft: &[f32], register: u8) -> f32 {
        let n = self.polynomials.len();
        let word = self.output(register);
        soft.iter()
            .enumerate()
            .map(|(j, &value)| {
                if word >> (n - 1 - j) & 1 == 1 {
                    -value
                } else {
                    value
                }
            })
            .sum()
    }

    fn run(&self, soft: &[f32], count: usize, start: usize, bits: &mut [bool]) -> f32 {
        let n = self.polynomials.len();
        let mut metric = [f32::NEG_INFINITY; STATES];
        let mut history = [[0u8; STATES]; MAX_BITS];
        metric[start] = 0.0;
        for (step, row) in history.iter_mut().enumerate().take(count) {
            let mut next = [f32::NEG_INFINITY; STATES];
            for (state, &value) in metric.iter().enumerate() {
                if value == f32::NEG_INFINITY {
                    continue;
                }
                for bit in 0..2u8 {
                    let register = bit << Self::MEMORY | state as u8;
                    let target = usize::from(register >> 1);
                    let candidate = value + self.branch(&soft[n * step..n * step + n], register);
                    if candidate > next[target] {
                        next[target] = candidate;
                        row[target] = state as u8;
                    }
                }
            }
            metric = next;
        }
        let mut state = start;
        for step in (0..count).rev() {
            bits[step] = state >> (Self::MEMORY - 1) & 1 == 1;
            state = usize::from(history[step][state]);
        }
        metric[start]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLYNOMIALS: [u8; 5] = [0b10101, 0b10111, 0b11011, 0b11111, 0b11001];
    const CODE: TailBitingViterbi<'static, 16, 16> = TailBitingViterbi::new(&POLYNOMIALS);

    fn soft(coded: &[bool]) -> Vec<f32> {
        coded
            .iter()
            .map(|&bit| if bit { -1.0 } else { 1.0 })
            .collect()
    }

    #[test]
    fn every_start_state_round_trips() {
        for word in [0u16, 1, 0x8000, 0xbeef, 0xffff] {
            let bits: Vec<bool> = (0..16).map(|i| word >> (15 - i) & 1 == 1).collect();
            let mut coded = [false; 80];
            CODE.encode(&bits, &mut coded);
            let mut decoded = [false; 16];
            let metric = CODE.decode(&soft(&coded), &mut decoded);
            assert_eq!(decoded.as_slice(), bits.as_slice(), "{word:#x}");
            assert_eq!(metric, 80.0);
        }
    }

    #[test]
    fn errors_are_repaired() {
        let bits: Vec<bool> = (0..14).map(|i| i % 3 == 0).collect();
        let mut coded = [false; 70];
        CODE.encode(&bits, &mut coded);
        let mut received = soft(&coded);
        for at in [2, 19, 33, 60] {
            received[at] = -received[at];
        }
        let mut decoded = [false; 14];
        let metric = CODE.decode(&received, &mut decoded);
        assert_eq!(decoded.as_slice(), bits.as_slice());
        assert_eq!(metric, 62.0);
    }

    #[test]
    fn a_tail_biting_word_wraps_its_register() {
        let mut coded = [false; 10];
        let small: TailBitingViterbi<'_, 4, 4> = TailBitingViterbi::new(&[0b111, 0b101]);
        small.encode(&[true, false, false, false, true], &mut coded);
        assert_eq!(coded[..2], [false, true]);
        assert_eq!(coded[8..], [true, true]);
    }
}
