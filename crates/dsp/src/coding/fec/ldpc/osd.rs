const WORD: usize = 64;

/// Ordered-statistics decoding of any binary linear code given by a basis of codewords.
///
/// The basis is reduced so its information set lies on the most reliable received bits; their
/// hard decisions, with every pattern of up to `order` flips, are re-encoded and the codeword
/// closest to the received word in soft distance wins. Log-likelihood ratios are positive for a
/// 0 bit, as for [`super::MinSum`].
#[derive(Clone, Debug)]
pub struct Osd {
    bits: usize,
    basis: Vec<Vec<u64>>,
    rows: Vec<Vec<u64>>,
    order: Vec<usize>,
    hard: Vec<u64>,
    weight: Vec<f32>,
}

/// The codeword ordered-statistics decoding settled on.
#[derive(Clone, Debug, PartialEq)]
pub struct OsdWord {
    pub bits: Vec<bool>,
    /// Sum of the reliabilities of the received bits the codeword disagrees with.
    pub distance: f32,
}

impl Osd {
    /// `basis` holds the code's generator rows, each a codeword of `bits` bits.
    #[must_use]
    pub fn new(bits: usize, basis: &[Vec<bool>]) -> Self {
        let words = bits.div_ceil(WORD);
        let basis = basis
            .iter()
            .map(|row| {
                let mut packed = vec![0u64; words];
                for (index, _) in row.iter().enumerate().filter(|&(_, &set)| set) {
                    packed[index / WORD] |= 1 << (index % WORD);
                }
                packed
            })
            .collect::<Vec<_>>();
        Self {
            bits,
            rows: basis.clone(),
            basis,
            order: (0..bits).collect(),
            hard: vec![0; words],
            weight: vec![0.0; bits],
        }
    }

    #[must_use]
    pub fn dimension(&self) -> usize {
        self.basis.len()
    }

    /// The closest codeword found with up to `order` flips of the most reliable
    /// basis, or `None` when the received word is too short or the reliable bits leave the
    /// basis singular.
    pub fn decode(&mut self, llrs: &[f32], order: usize) -> Option<OsdWord> {
        if llrs.len() < self.bits {
            return None;
        }
        self.sort_by_reliability(llrs);
        self.reduce()?;
        let words = self.hard.len();
        let mut base = vec![0u64; words];
        for (row, packed) in self.rows.iter().enumerate() {
            if get(&self.hard, row) {
                xor_into(&mut base, packed);
            }
        }
        let mut best = (self.distance(&base, f32::INFINITY), base.clone());
        let mut levels = vec![vec![0u64; words]; order + 1];
        levels[0].copy_from_slice(&base);
        self.flip(&mut levels, 0, 0, &mut best);
        let mut bits = vec![false; self.bits];
        for (position, &original) in self.order.iter().enumerate() {
            bits[original] = get(&best.1, position);
        }
        Some(OsdWord {
            bits,
            distance: best.0,
        })
    }

    /// Tries every way to flip one more basis row from `from` on, `depth` rows flipped so far.
    fn flip(&self, levels: &mut [Vec<u64>], depth: usize, from: usize, best: &mut (f32, Vec<u64>)) {
        if depth + 1 >= levels.len() {
            return;
        }
        for row in from..self.dimension() {
            let (done, rest) = levels.split_at_mut(depth + 1);
            let trial = &mut rest[0];
            trial.copy_from_slice(&done[depth]);
            xor_into(trial, &self.rows[row]);
            let distance = self.distance(trial, best.0);
            if distance < best.0 {
                best.0 = distance;
                best.1.copy_from_slice(trial);
            }
            self.flip(levels, depth + 1, row + 1, best);
        }
    }

    /// The soft distance from the received word, or `limit` once it is reached.
    fn distance(&self, word: &[u64], limit: f32) -> f32 {
        let mut total = 0.0;
        for (index, (&word, &hard)) in word.iter().zip(&self.hard).enumerate() {
            let mut differ = word ^ hard;
            while differ != 0 {
                let bit = differ.trailing_zeros() as usize;
                total += self.weight[index * WORD + bit];
                if total >= limit {
                    return limit;
                }
                differ &= differ - 1;
            }
        }
        total
    }

    fn sort_by_reliability(&mut self, llrs: &[f32]) {
        self.order = (0..self.bits).collect();
        self.order
            .sort_by(|&a, &b| llrs[b].abs().total_cmp(&llrs[a].abs()));
        self.hard.fill(0);
        for (position, &original) in self.order.iter().enumerate() {
            self.weight[position] = llrs[original].abs();
            if llrs[original] < 0.0 {
                self.hard[position / WORD] |= 1 << (position % WORD);
            }
        }
        for (row, basis) in self.rows.iter_mut().zip(&self.basis) {
            row.fill(0);
            for (position, &original) in self.order.iter().enumerate() {
                if get(basis, original) {
                    row[position / WORD] |= 1 << (position % WORD);
                }
            }
        }
    }

    fn reduce(&mut self) -> Option<()> {
        let dimension = self.dimension();
        for pivot in 0..dimension {
            let (row, column) = (pivot..self.bits).find_map(|column| {
                (pivot..dimension)
                    .find(|&row| get(&self.rows[row], column))
                    .map(|row| (row, column))
            })?;
            self.rows.swap(pivot, row);
            if column != pivot {
                self.swap_columns(pivot, column);
            }
            let lead = self.rows[pivot].clone();
            for (index, row) in self.rows.iter_mut().enumerate() {
                if index != pivot && get(row, pivot) {
                    xor_into(row, &lead);
                }
            }
        }
        Some(())
    }

    fn swap_columns(&mut self, a: usize, b: usize) {
        for row in &mut self.rows {
            swap_bits(row, a, b);
        }
        swap_bits(&mut self.hard, a, b);
        self.weight.swap(a, b);
        self.order.swap(a, b);
    }
}

fn get(word: &[u64], bit: usize) -> bool {
    word[bit / WORD] >> (bit % WORD) & 1 == 1
}

fn swap_bits(word: &mut [u64], a: usize, b: usize) {
    if get(word, a) != get(word, b) {
        word[a / WORD] ^= 1 << (a % WORD);
        word[b / WORD] ^= 1 << (b % WORD);
    }
}

fn xor_into(target: &mut [u64], source: &[u64]) {
    for (target, source) in target.iter_mut().zip(source) {
        *target ^= source;
    }
}

#[cfg(test)]
mod tests {
    use super::super::Systematic;
    use super::*;

    fn hamming() -> (Systematic, Vec<Vec<bool>>) {
        let code = Systematic::from_hex(4, &["d", "b", "7"]).expect("valid rows");
        let basis = (0..4)
            .map(|bit| {
                let mut message = [false; 4];
                message[bit] = true;
                let mut word = vec![false; 7];
                code.encode(&message, &mut word);
                word
            })
            .collect();
        (code, basis)
    }

    #[test]
    fn an_unreliable_flipped_bit_is_repaired() {
        let (code, basis) = hamming();
        let message = [true, false, true, true];
        let mut sent = [false; 7];
        code.encode(&message, &mut sent);
        let mut llrs: Vec<f32> = sent
            .iter()
            .map(|&bit| if bit { -4.0 } else { 4.0 })
            .collect();
        llrs[2] = -llrs[2] * 0.1;
        let mut osd = Osd::new(7, &basis);
        let found = osd.decode(&llrs, 1).expect("a codeword");
        assert_eq!(found.bits, sent);
        assert!((found.distance - 0.4).abs() < 1e-6);
    }

    #[test]
    fn order_zero_re_encodes_the_most_reliable_bits() {
        let (code, basis) = hamming();
        let mut sent = [false; 7];
        code.encode(&[false, true, true, false], &mut sent);
        let llrs: Vec<f32> = sent
            .iter()
            .enumerate()
            .map(|(index, &bit)| (if bit { -1.0 } else { 1.0 }) * (7 - index) as f32)
            .collect();
        let found = Osd::new(7, &basis).decode(&llrs, 0).expect("a codeword");
        assert_eq!(found.bits, sent);
        assert_eq!(found.distance, 0.0);
    }

    #[test]
    fn a_full_depth_search_finds_the_closest_codeword() {
        let mut state = 0x1234_5678_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) as u32
        };
        let basis: Vec<Vec<bool>> = (0..6)
            .map(|_| (0..16).map(|_| next() & 1 == 1).collect())
            .collect();
        let llrs: Vec<f32> = (0..16)
            .map(|_| (next() % 2_001) as f32 / 1_000.0 - 1.0)
            .collect();
        let closest = (0u32..1 << 6)
            .map(|message| {
                (0..16)
                    .map(|bit| {
                        let set = (0..6)
                            .filter(|&row| message >> row & 1 == 1)
                            .fold(false, |acc, row| acc ^ basis[row][bit]);
                        if set == (llrs[bit] < 0.0) {
                            0.0
                        } else {
                            llrs[bit].abs()
                        }
                    })
                    .sum::<f32>()
            })
            .fold(f32::INFINITY, f32::min);
        let mut osd = Osd::new(16, &basis);
        let found = osd.decode(&llrs, 6).expect("a codeword");
        assert!(
            (found.distance - closest).abs() < 1e-4,
            "{} vs {closest}",
            found.distance
        );
        assert!(osd.decode(&llrs, 1).expect("a codeword").distance >= found.distance);
    }

    #[test]
    fn a_short_word_is_refused() {
        let (_, basis) = hamming();
        assert!(Osd::new(7, &basis).decode(&[1.0; 6], 1).is_none());
    }
}
