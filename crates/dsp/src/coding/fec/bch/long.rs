use super::divider::{self, Divider, MAX_PARITY, Remainder};

/// A binary BCH code over GF(2^m), shortened to any length up to 2^m - 1, that corrects up
/// to `correct` errors by Berlekamp-Massey and a Chien search. The field's primitive
/// polynomial and the error count are data, so the generator follows from them.
pub struct Bch {
    exp: Vec<u16>,
    log: Vec<u16>,
    order: usize,
    divider: Divider,
    generator: Vec<bool>,
    correct: usize,
}

/// Working memory for [`Bch::decode_with_scratch`], so decoding never allocates.
pub struct BchScratch {
    syndromes: Vec<u16>,
    locator: Vec<u16>,
    previous: Vec<u16>,
    saved: Vec<u16>,
}

impl Bch {
    /// `primitive` includes the x^`bits` term. `correct` is capped where the parity would
    /// pass 192 bits.
    ///
    /// # Panics
    /// When `bits` is outside 2..=16.
    #[must_use]
    pub fn new(bits: u32, primitive: u32, correct: usize) -> Self {
        assert!(
            (2..=16).contains(&bits),
            "field must be GF(2^2) to GF(2^16)"
        );
        let size = 1usize << bits;
        let order = size - 1;
        let mut exp = vec![0u16; 2 * order];
        let mut log = vec![0u16; size];
        let mut value = 1u32;
        for (index, slot) in exp[..order].iter_mut().enumerate() {
            *slot = value as u16;
            log[value as usize] = index as u16;
            value <<= 1;
            if value & size as u32 != 0 {
                value ^= primitive;
            }
        }
        exp.copy_within(..order, order);
        let correct = correct.min(MAX_PARITY / bits as usize);
        let generator = generator(&exp, &log, order, correct);
        Self {
            divider: Divider::new(&generator),
            exp,
            log,
            order,
            generator,
            correct,
        }
    }

    #[must_use]
    pub const fn parity(&self) -> usize {
        self.generator.len() - 1
    }

    #[must_use]
    pub const fn correct(&self) -> usize {
        self.correct
    }

    /// Generator coefficients, lowest power first.
    #[must_use]
    pub fn generator(&self) -> &[bool] {
        &self.generator
    }

    fn power(&self, exponent: usize) -> u16 {
        self.exp[exponent % self.order]
    }

    fn mul(&self, a: u16, b: u16) -> u16 {
        if a == 0 || b == 0 {
            return 0;
        }
        self.exp[usize::from(self.log[usize::from(a)]) + usize::from(self.log[usize::from(b)])]
    }

    fn inv(&self, a: u16) -> u16 {
        self.power(self.order - usize::from(self.log[usize::from(a)]))
    }

    /// Appends `message` and its parity, first bit the highest power.
    pub fn encode(&self, message: &[bool], out: &mut Vec<bool>) {
        let parity_len = self.parity();
        let mut remainder = vec![false; parity_len];
        for &bit in message {
            let feedback = bit ^ remainder[0];
            remainder.copy_within(1.., 0);
            remainder[parity_len - 1] = false;
            if feedback {
                for (index, slot) in remainder.iter_mut().enumerate() {
                    *slot ^= self.generator[parity_len - 1 - index];
                }
            }
        }
        out.extend_from_slice(message);
        out.extend_from_slice(&remainder);
    }

    #[must_use]
    pub fn scratch(&self) -> BchScratch {
        BchScratch {
            syndromes: vec![0; 2 * self.correct],
            locator: vec![0; 2 * self.correct + 1],
            previous: vec![0; 2 * self.correct + 1],
            saved: vec![0; 2 * self.correct + 1],
        }
    }

    fn syndromes(&self, remainder: &Remainder, out: &mut [u16]) {
        for (index, syndrome) in out.iter_mut().enumerate() {
            *syndrome = divider::set_bits(remainder)
                .fold(0, |sum, power| sum ^ self.power((index + 1) * power));
        }
    }

    fn locator(&self, scratch: &mut BchScratch) -> usize {
        let BchScratch {
            syndromes,
            locator,
            previous,
            saved,
        } = scratch;
        locator.fill(0);
        previous.fill(0);
        locator[0] = 1;
        previous[0] = 1;
        let mut discrepancy_at_update = 1;
        let mut shift = 1;
        let mut errors = 0;
        for step in 0..syndromes.len() {
            let mut discrepancy = syndromes[step];
            for index in 1..=errors {
                discrepancy ^= self.mul(locator[index], syndromes[step - index]);
            }
            if discrepancy == 0 {
                shift += 1;
                continue;
            }
            saved.copy_from_slice(locator);
            let scale = self.mul(discrepancy, self.inv(discrepancy_at_update));
            for index in shift..locator.len() {
                locator[index] ^= self.mul(scale, previous[index - shift]);
            }
            if 2 * errors <= step {
                errors = step + 1 - errors;
                previous.copy_from_slice(saved);
                discrepancy_at_update = discrepancy;
                shift = 1;
            } else {
                shift += 1;
            }
        }
        errors
    }

    /// Corrects `word` in place. Returns the bits flipped, or `None` when the word is beyond
    /// repair. Allocates its scratch; see [`Bch::decode_with_scratch`].
    pub fn decode(&self, word: &mut [bool]) -> Option<usize> {
        self.decode_with_scratch(word, &mut self.scratch())
    }

    pub fn decode_with_scratch(
        &self,
        word: &mut [bool],
        scratch: &mut BchScratch,
    ) -> Option<usize> {
        if word.is_empty() || word.len() > self.order || scratch.syndromes.len() != 2 * self.correct
        {
            return None;
        }
        let remainder = self.divider.remainder(word);
        if remainder == Remainder::default() {
            return Some(0);
        }
        self.syndromes(&remainder, &mut scratch.syndromes);
        let errors = self.locator(scratch);
        if errors == 0 || errors > self.correct {
            return None;
        }
        let last = word.len() - 1;
        let mut found = 0;
        for (position, slot) in word.iter_mut().enumerate() {
            let inverse = self.power(self.order - (last - position) % self.order);
            let mut sum = 0;
            let mut term = 1;
            for &coefficient in &scratch.locator[..=errors] {
                sum ^= self.mul(coefficient, term);
                term = self.mul(term, inverse);
            }
            if sum == 0 {
                *slot ^= true;
                found += 1;
            }
        }
        if found != errors {
            return None;
        }
        (self.divider.remainder(word) == Remainder::default()).then_some(errors)
    }
}

fn minimal_polynomial(exp: &[u16], log: &[u16], order: usize, power: usize) -> Vec<u16> {
    let mut roots = Vec::new();
    let mut current = power % order;
    loop {
        if roots.contains(&current) {
            break;
        }
        roots.push(current);
        current = current * 2 % order;
    }
    let mut polynomial = vec![1u16];
    for root in roots {
        let value = exp[root];
        let mut next = vec![0u16; polynomial.len() + 1];
        for (index, &coefficient) in polynomial.iter().enumerate() {
            next[index + 1] ^= coefficient;
            if coefficient != 0 && value != 0 {
                next[index] ^= exp[(usize::from(log[usize::from(coefficient)])
                    + usize::from(log[usize::from(value)]))
                    % order];
            }
        }
        polynomial = next;
    }
    polynomial
}

fn generator(exp: &[u16], log: &[u16], order: usize, correct: usize) -> Vec<bool> {
    let mut generator = vec![true];
    for step in 0..correct {
        let factor = minimal_polynomial(exp, log, order, 2 * step + 1);
        if !divides(&generator, &factor) {
            generator = multiply(&generator, &factor);
        }
    }
    generator
}

fn multiply(left: &[bool], right: &[u16]) -> Vec<bool> {
    let mut out = vec![false; left.len() + right.len() - 1];
    for (index, &a) in left.iter().enumerate() {
        if !a {
            continue;
        }
        for (offset, &b) in right.iter().enumerate() {
            if b != 0 {
                out[index + offset] ^= true;
            }
        }
    }
    out
}

fn divides(product: &[bool], factor: &[u16]) -> bool {
    let factor: Vec<bool> = factor.iter().map(|&value| value != 0).collect();
    if factor.len() > product.len() {
        return false;
    }
    let mut remainder = product.to_vec();
    for index in (factor.len() - 1..remainder.len()).rev() {
        if !remainder[index] {
            continue;
        }
        let base = index + 1 - factor.len();
        for (offset, &coefficient) in factor.iter().enumerate() {
            if coefficient {
                remainder[base + offset] ^= true;
            }
        }
    }
    remainder.iter().all(|&bit| !bit)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GF16: u32 = 0b1_0011;
    const GF2_14: u32 = 0x402B;

    fn message(len: usize, seed: u32) -> Vec<bool> {
        let mut state = seed | 1;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state & 1 == 1
            })
            .collect()
    }

    #[test]
    fn the_textbook_15_7_code_has_its_known_generator() {
        let code = Bch::new(4, GF16, 2);
        assert_eq!(code.parity(), 8);
        let expected = [true, false, false, false, true, false, true, true, true];
        assert_eq!(code.generator(), expected);
    }

    #[test]
    fn the_field_follows_its_primitive_polynomial() {
        let code = Bch::new(14, GF2_14, 12);
        assert_eq!(code.power(14), 0x2B);
        assert_eq!(code.power(0), 1);
        assert_eq!(code.mul(code.power(3), code.inv(code.power(3))), 1);
    }

    #[test]
    fn the_generator_vanishes_at_every_design_root() {
        let code = Bch::new(14, GF2_14, 12);
        for root in 1..=24 {
            let value = code.power(root);
            let mut sum = 0u16;
            let mut term = 1u16;
            for &coefficient in code.generator() {
                if coefficient {
                    sum ^= term;
                }
                term = code.mul(term, value);
            }
            assert_eq!(sum, 0, "root {root}");
        }
    }

    #[test]
    fn errors_up_to_the_design_distance_are_repaired() {
        for (bits, primitive, correct, length) in [
            (4, GF16, 1, 15),
            (4, GF16, 2, 15),
            (14, GF2_14, 12, 7_200),
            (14, GF2_14, 3, 300),
        ] {
            let code = Bch::new(bits, primitive, correct);
            let mut word = Vec::new();
            code.encode(&message(length - code.parity(), 5), &mut word);
            let clean = word.clone();
            let mut scratch = code.scratch();
            assert_eq!(code.decode_with_scratch(&mut word, &mut scratch), Some(0));
            for step in 0..correct {
                word[step * 5 + 1] ^= true;
            }
            assert_eq!(
                code.decode_with_scratch(&mut word, &mut scratch),
                Some(correct)
            );
            assert_eq!(word, clean);
        }
    }

    #[test]
    fn one_error_beyond_the_design_distance_is_refused() {
        let code = Bch::new(14, GF2_14, 4);
        let mut word = Vec::new();
        code.encode(&message(1_000, 9), &mut word);
        for step in 0..5 {
            word[step * 101 + 11] ^= true;
        }
        assert_eq!(code.decode(&mut word), None);
    }

    #[test]
    fn wrong_sizes_are_refused() {
        let code = Bch::new(4, GF16, 2);
        assert_eq!(code.decode(&mut []), None);
        assert_eq!(code.decode(&mut [false; 16]), None);
        let mut other = Bch::new(4, GF16, 1).scratch();
        assert_eq!(code.decode_with_scratch(&mut [false; 15], &mut other), None);
    }
}
