const SIZE: usize = 256;
const ORDER: usize = SIZE - 1;

/// x^8 + x^4 + x^3 + x^2 + 1, the field most byte-wide Reed-Solomon codes use.
pub const GF256_PRIMITIVE: u16 = 0x11D;

/// Reed-Solomon over GF(2^m), m up to 8, with any first consecutive root: errors and erasures,
/// shortened codewords included.
#[derive(Clone, Debug)]
pub struct ReedSolomon {
    order: usize,
    exp: [u8; 2 * ORDER],
    log: [u8; SIZE],
    generator: Vec<u8>,
    parity: usize,
    first_root: u8,
    root_multiples: Vec<[u8; SIZE]>,
}

impl ReedSolomon {
    /// A code over GF(256).
    #[must_use]
    pub fn new(primitive: u16, first_root: u8, parity: usize) -> Self {
        Self::with_field(u8::BITS, primitive, first_root, parity)
    }

    /// A code over GF(2^`bits`) built on `primitive`, whose roots are consecutive powers of x
    /// from x^`first_root`.
    #[must_use]
    pub fn with_field(bits: u32, primitive: u16, first_root: u8, parity: usize) -> Self {
        assert!((2..=u8::BITS).contains(&bits), "symbols of 2 to 8 bits");
        let size = 1usize << bits;
        let order = size - 1;
        let mut exp = [0u8; 2 * ORDER];
        let mut log = [0u8; SIZE];
        let mut value = 1u16;
        for index in 0..order {
            exp[index] = value as u8;
            exp[index + order] = value as u8;
            log[value as usize] = index as u8;
            value <<= 1;
            if value & size as u16 != 0 {
                value ^= primitive;
            }
        }
        let mut code = Self {
            order,
            exp,
            log,
            generator: Vec::new(),
            parity,
            first_root,
            root_multiples: Vec::new(),
        };
        assert!(parity < size, "parity longer than the field");
        code.generator = code.build_generator();
        code.root_multiples = (0..parity)
            .map(|index| {
                let root = code.power(u32::from(first_root) + index as u32);
                std::array::from_fn(|value| code.mul(value as u8, root))
            })
            .collect();
        code
    }

    fn build_generator(&self) -> Vec<u8> {
        let mut ascending = vec![1u8];
        for index in 0..self.parity {
            let root = self.power(u32::from(self.first_root) + index as u32);
            let mut next = vec![0u8; ascending.len() + 1];
            for (position, &coefficient) in ascending.iter().enumerate() {
                next[position] ^= self.mul(coefficient, root);
                next[position + 1] ^= coefficient;
            }
            ascending = next;
        }
        ascending.reverse();
        ascending
    }

    fn power(&self, exponent: u32) -> u8 {
        self.exp[(exponent % self.order as u32) as usize]
    }

    fn mul(&self, a: u8, b: u8) -> u8 {
        if a == 0 || b == 0 {
            return 0;
        }
        self.exp[usize::from(self.log[usize::from(a)]) + usize::from(self.log[usize::from(b)])]
    }

    fn inv(&self, a: u8) -> u8 {
        self.exp[self.order - usize::from(self.log[usize::from(a)])]
    }

    fn evaluate(&self, ascending: &[u8], at: u8) -> u8 {
        let mut sum = 0u8;
        let mut term = 1u8;
        for &coefficient in ascending {
            sum ^= self.mul(coefficient, term);
            term = self.mul(term, at);
        }
        sum
    }

    /// The longest codeword the field allows.
    #[must_use]
    pub const fn max_len(&self) -> usize {
        self.order
    }

    #[must_use]
    pub const fn parity(&self) -> usize {
        self.parity
    }

    #[must_use]
    pub fn correctable(&self) -> usize {
        self.parity / 2
    }

    pub fn encode(&self, data: &[u8], out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(data);
        out.resize(start + data.len() + self.parity, 0);
        for index in 0..data.len() {
            let coefficient = out[start + index];
            if coefficient == 0 {
                continue;
            }
            for (offset, &factor) in self.generator.iter().enumerate().skip(1) {
                out[start + index + offset] ^= self.mul(factor, coefficient);
            }
        }
        out[start..start + data.len()].copy_from_slice(data);
    }

    fn syndromes(&self, codeword: &[u8]) -> [u8; SIZE] {
        let mut result = [0u8; SIZE];
        for (syndrome, table) in result.iter_mut().zip(&self.root_multiples) {
            *syndrome = codeword
                .iter()
                .fold(0u8, |value, &symbol| table[usize::from(value)] ^ symbol);
        }
        result
    }

    fn berlekamp_massey(&self, syndromes: &[u8]) -> ([u8; SIZE], usize) {
        let mut locator = [0u8; SIZE];
        locator[0] = 1;
        let mut previous = locator;
        let mut locator_len = 1;
        let mut previous_len = 1;
        let mut discrepancy_at_update = 1u8;
        let mut shift = 1usize;
        let mut errors = 0usize;
        for step in 0..syndromes.len() {
            let mut discrepancy = syndromes[step];
            for index in 1..=errors.min(step) {
                if index < locator_len {
                    discrepancy ^= self.mul(locator[index], syndromes[step - index]);
                }
            }
            if discrepancy == 0 {
                shift += 1;
                continue;
            }
            let saved = locator;
            let saved_len = locator_len;
            let scale = self.mul(discrepancy, self.inv(discrepancy_at_update));
            locator_len = locator_len.max(previous_len + shift).min(SIZE);
            for (index, &coefficient) in previous
                .iter()
                .take(previous_len.min(SIZE - shift))
                .enumerate()
            {
                locator[index + shift] ^= self.mul(scale, coefficient);
            }
            if 2 * errors <= step {
                errors = step + 1 - errors;
                previous = saved;
                previous_len = saved_len;
                discrepancy_at_update = discrepancy;
                shift = 1;
            } else {
                shift += 1;
            }
        }
        (locator, errors)
    }

    fn chien(&self, locator: &[u8], length: usize, errors: usize) -> Option<[usize; SIZE]> {
        let mut positions = [0usize; SIZE];
        let mut count = 0;
        for position in 0..length {
            let inverse = self.power((self.order - position % self.order) as u32);
            if self.evaluate(locator, inverse) == 0 {
                positions[count] = position;
                count += 1;
            }
        }
        (count == errors).then_some(positions)
    }

    fn evaluator(&self, syndromes: &[u8], locator: &[u8]) -> [u8; SIZE] {
        let mut product = [0u8; SIZE];
        for (index, &coefficient) in locator.iter().enumerate() {
            for (offset, &syndrome) in syndromes.iter().enumerate() {
                if index + offset < self.parity {
                    product[index + offset] ^= self.mul(coefficient, syndrome);
                }
            }
        }
        product
    }

    fn magnitude(&self, evaluator: &[u8], derivative: &[u8], position: usize) -> Option<u8> {
        let inverse = self.power((self.order - position % self.order) as u32);
        let denominator = self.evaluate(derivative, inverse);
        if denominator == 0 {
            return None;
        }
        let ratio = self.mul(self.evaluate(evaluator, inverse), self.inv(denominator));
        let order = self.order as u32;
        let exponent =
            (order * SIZE as u32 - u32::from(self.first_root) * position as u32 % order) % order;
        Some(self.mul(ratio, self.power(exponent)))
    }

    pub fn decode(&self, codeword: &mut [u8]) -> Option<u32> {
        if codeword.len() > self.order || codeword.len() < self.parity {
            return None;
        }
        let storage = self.syndromes(codeword);
        let syndromes = &storage[..self.parity];
        if syndromes.iter().all(|&value| value == 0) {
            return Some(0);
        }
        let (locator, errors) = self.berlekamp_massey(syndromes);
        if errors == 0 || errors > self.correctable() {
            return None;
        }
        let locator = &locator[..=errors];
        let positions = self.chien(locator, codeword.len(), errors)?;
        let evaluator = self.evaluator(syndromes, locator);
        let mut derivative = [0u8; SIZE];
        for (index, &coefficient) in locator.iter().enumerate() {
            if index % 2 == 1 {
                derivative[index] = coefficient;
            }
        }
        for &position in positions.iter().take(errors) {
            let magnitude = self.magnitude(
                &evaluator[..self.parity],
                &derivative[..locator.len()],
                position,
            )?;
            let index = codeword.len().checked_sub(1 + position)?;
            codeword[index] ^= magnitude;
        }
        self.syndromes(codeword)
            .iter()
            .all(|&value| value == 0)
            .then_some(errors as u32)
    }
}

impl ReedSolomon {
    pub fn decode_with_erasures(&self, codeword: &mut [u8], erasures: &[usize]) -> Option<u32> {
        let len = codeword.len();
        if len > self.order
            || len < self.parity
            || erasures.len() > self.parity
            || erasures.iter().any(|&index| index >= len)
        {
            return None;
        }
        let storage = self.syndromes(codeword);
        let syndromes = &storage[..self.parity];
        if syndromes.iter().all(|&value| value == 0) {
            return Some(0);
        }
        let erased = erasures.len();
        let gamma = self.erasure_locator(erasures, len);
        let forney = self.evaluator(syndromes, &gamma[..=erased]);
        let (lambda, errors) = self.berlekamp_massey(&forney[erased..self.parity]);
        if errors > (self.parity - erased) / 2 {
            return None;
        }
        let degree = errors + erased;
        let errata = self.product(&lambda[..=errors], &gamma[..=erased]);
        let locator = &errata[..=degree];
        let positions = self.chien(locator, len, degree)?;
        let evaluator = self.evaluator(syndromes, locator);
        let mut derivative = [0u8; SIZE];
        for (index, &coefficient) in locator.iter().enumerate() {
            if index % 2 == 1 {
                derivative[index] = coefficient;
            }
        }
        for &position in positions.iter().take(degree) {
            let magnitude = self.magnitude(
                &evaluator[..self.parity],
                &derivative[..locator.len()],
                position,
            )?;
            codeword[len - 1 - position] ^= magnitude;
        }
        self.syndromes(codeword)[..self.parity]
            .iter()
            .all(|&value| value == 0)
            .then_some(degree as u32)
    }

    fn erasure_locator(&self, erasures: &[usize], len: usize) -> [u8; SIZE] {
        let mut gamma = [0u8; SIZE];
        gamma[0] = 1;
        for (count, &index) in erasures.iter().enumerate() {
            let root = self.power((len - 1 - index) as u32);
            for degree in (1..=count + 1).rev() {
                gamma[degree] ^= self.mul(root, gamma[degree - 1]);
            }
        }
        gamma
    }

    fn product(&self, a: &[u8], b: &[u8]) -> [u8; SIZE] {
        let mut out = [0u8; SIZE];
        for (i, &x) in a.iter().enumerate() {
            for (j, &y) in b.iter().enumerate() {
                if i + j < out.len() {
                    out[i + j] ^= self.mul(x, y);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{super::test_data::random_bytes as payload, *};

    fn dvb() -> ReedSolomon {
        ReedSolomon::new(GF256_PRIMITIVE, 0, 16)
    }

    fn dab_plus() -> ReedSolomon {
        ReedSolomon::new(GF256_PRIMITIVE, 0, 10)
    }

    #[test]
    fn the_field_matches_the_dvb_primitive_polynomial() {
        let code = dvb();
        assert_eq!(code.power(8), 0x1D);
        assert_eq!(code.power(0), 1);
        assert_eq!(code.mul(code.power(200), code.inv(code.power(200))), 1);
    }

    #[test]
    fn the_generator_is_monic_and_vanishes_at_every_root() {
        for code in [dvb(), dab_plus(), ReedSolomon::new(GF256_PRIMITIVE, 1, 8)] {
            assert_eq!(code.generator[0], 1);
            assert_eq!(code.generator.len(), code.parity() + 1);
            let ascending: Vec<u8> = code.generator.iter().rev().copied().collect();
            for index in 0..code.parity() {
                let root = code.power(u32::from(code.first_root) + index as u32);
                assert_eq!(code.evaluate(&ascending, root), 0, "root {index}");
            }
        }
    }

    #[test]
    fn a_clean_dvb_codeword_reports_no_errors() {
        let code = dvb();
        let mut codeword = Vec::new();
        code.encode(&payload(188, 3), &mut codeword);
        assert_eq!(codeword.len(), 204);
        assert_eq!(code.decode(&mut codeword), Some(0));
    }

    #[test]
    fn eight_symbol_errors_are_repaired() {
        let code = dvb();
        let mut codeword = Vec::new();
        code.encode(&payload(188, 5), &mut codeword);
        let clean = codeword.clone();
        for (offset, position) in [0usize, 7, 40, 91, 130, 150, 187, 203]
            .into_iter()
            .enumerate()
        {
            codeword[position] ^= 0x5A ^ offset as u8;
        }
        assert_eq!(code.decode(&mut codeword), Some(8));
        assert_eq!(codeword, clean);
    }

    #[test]
    fn nine_symbol_errors_are_refused_rather_than_mangled() {
        let code = dvb();
        let mut codeword = Vec::new();
        code.encode(&payload(188, 9), &mut codeword);
        for position in [1usize, 5, 9, 44, 70, 99, 111, 160, 190] {
            codeword[position] ^= 0xC3;
        }
        assert_eq!(code.decode(&mut codeword), None);
    }

    #[test]
    fn the_shortened_dab_plus_codeword_repairs_five_errors() {
        let code = dab_plus();
        let mut codeword = Vec::new();
        code.encode(&payload(110, 17), &mut codeword);
        assert_eq!(codeword.len(), 120);
        let clean = codeword.clone();
        for position in [3usize, 30, 55, 100, 117] {
            codeword[position] ^= 0x9E;
        }
        assert_eq!(code.decode(&mut codeword), Some(5));
        assert_eq!(codeword, clean);
    }

    #[test]
    fn a_single_error_at_either_end_is_repaired() {
        let code = dab_plus();
        let mut codeword = Vec::new();
        code.encode(&payload(110, 23), &mut codeword);
        let clean = codeword.clone();
        for position in [0, clean.len() - 1] {
            let mut damaged = clean.clone();
            damaged[position] ^= 0x01;
            assert_eq!(code.decode(&mut damaged), Some(1));
            assert_eq!(damaged, clean);
        }
    }

    #[test]
    fn a_code_with_a_non_zero_first_root_still_corrects() {
        let code = ReedSolomon::new(GF256_PRIMITIVE, 1, 8);
        let mut codeword = Vec::new();
        code.encode(&payload(100, 41), &mut codeword);
        let clean = codeword.clone();
        for position in [2usize, 44, 77, 101] {
            codeword[position] ^= 0x7B;
        }
        assert_eq!(code.decode(&mut codeword), Some(4));
        assert_eq!(codeword, clean);
    }

    fn erasure_trial(code: &ReedSolomon, len: usize, errors: &[usize], erasures: &[usize]) {
        let data = payload(len - code.parity(), 0xE5A5);
        let mut clean = Vec::new();
        code.encode(&data, &mut clean);
        let mut received = clean.clone();
        for &index in errors.iter().chain(erasures) {
            received[index] ^= 0x5B;
        }
        let fixed = code.decode_with_erasures(&mut received, erasures);
        assert_eq!(fixed, Some((errors.len() + erasures.len()) as u32));
        assert_eq!(received, clean);
    }

    #[test]
    fn erasures_double_the_correctable_count() {
        let code = dvb();
        let erasures: Vec<usize> = (0..16).map(|k| k * 13).collect();
        erasure_trial(&code, 255, &[], &erasures);
        erasure_trial(&code, 204, &[3, 77, 150], &erasures[..10]);
        erasure_trial(&code, 120, &[5, 6, 7, 8, 9, 10, 11, 12], &[]);
    }

    #[test]
    fn a_six_bit_code_corrects_to_each_codes_capacity() {
        for (data_symbols, parity_symbols) in [(12, 12), (16, 8), (20, 16)] {
            let code = ReedSolomon::with_field(6, 0x43, 1, parity_symbols);
            let data: Vec<u8> = (0..data_symbols)
                .map(|index| (index * 7 + 3) as u8 & 0x3F)
                .collect();
            let mut clean = Vec::new();
            code.encode(&data, &mut clean);
            assert!(clean.iter().all(|&symbol| symbol < 64));
            assert_eq!(code.decode(&mut clean.clone()), Some(0));
            let mut damaged = clean.clone();
            for index in 0..parity_symbols / 2 {
                damaged[index * 2] ^= (index as u8 + 1) & 0x3F;
            }
            assert_eq!(code.decode(&mut damaged), Some((parity_symbols / 2) as u32));
            assert_eq!(damaged, clean);
        }
    }

    #[test]
    fn a_six_bit_code_with_a_high_first_root_round_trips() {
        let code = ReedSolomon::with_field(6, 0x43, 54, 10);
        assert_eq!(code.max_len(), 63);
        let data: Vec<u8> = (0..42).map(|index| (index * 11 + 5) as u8 & 0x3F).collect();
        let mut clean = Vec::new();
        code.encode(&data, &mut clean);
        let mut damaged = clean.clone();
        for (k, position) in [0usize, 9, 23, 40, 51].into_iter().enumerate() {
            damaged[position] ^= 0x15 + k as u8;
        }
        assert_eq!(code.decode(&mut damaged), Some(5));
        assert_eq!(damaged, clean);
        let mut too_many = clean;
        for position in [1usize, 2, 3, 4, 5, 6] {
            too_many[position] ^= 0x2A;
        }
        assert_ne!(code.decode(&mut too_many), Some(6));
    }

    #[test]
    fn a_codeword_longer_than_the_field_is_refused() {
        let code = ReedSolomon::with_field(6, 0x43, 1, 8);
        assert_eq!(code.decode(&mut [0u8; 64]), None);
    }

    #[test]
    fn erasures_work_with_a_non_zero_first_root() {
        let code = ReedSolomon::new(0x187, 120, 6);
        erasure_trial(&code, 255, &[40], &[3, 200, 254, 0]);
    }

    #[test]
    fn too_many_errata_are_refused() {
        let code = dvb();
        let data = payload(239, 7);
        let mut clean = Vec::new();
        code.encode(&data, &mut clean);
        let mut received = clean.clone();
        for index in (0..30).map(|k| k * 7) {
            received[index] ^= 0x33;
        }
        let erasures: Vec<usize> = (0..10).map(|k| k * 7).collect();
        let result = code.decode_with_erasures(&mut received, &erasures);
        assert_ne!(result, Some(30));
    }
}
