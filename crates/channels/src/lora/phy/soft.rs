use super::code::{codeword_bit, codeword_len, hamming_encode};

pub(crate) const MAX_BITS: usize = 12;

#[must_use]
fn ln_i0(x: f32) -> f32 {
    if x < 3.75 {
        let t = (x / 3.75) * (x / 3.75);
        let series = 1.0
            + t * (3.515_623
                + t * (3.089_942_4
                    + t * (1.206_749_2 + t * (0.265_973_2 + t * (0.036_076_8 + t * 0.004_581_3)))));
        series.ln()
    } else {
        let t = 3.75 / x;
        let series = 0.398_942_3
            + t * (0.013_285_92
                + t * (0.002_253_19
                    + t * (-0.001_575_65
                        + t * (0.009_162_81
                            + t * (-0.020_577_06
                                + t * (0.026_355_37 + t * (-0.016_476_33 + t * 0.003_923_77)))))));
        x - 0.5 * x.ln() + series.ln()
    }
}

pub(crate) struct Demapper {
    values: Vec<f32>,
    spare: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Statistics {
    pub amplitude: f32,
    pub noise: f32,
}

impl Demapper {
    #[must_use]
    pub(crate) fn new(chips: usize) -> Self {
        Self {
            values: vec![0.0; chips],
            spare: vec![0.0; chips.div_ceil(2)],
        }
    }

    /// Max-log bit LLRs from one symbol's bin energies. The likelihood `ln I0` of a bin grows
    /// with its energy, so the strongest energy on each side of a bit decides it and only those
    /// few winners need the logarithm.
    pub(crate) fn llrs(
        &mut self,
        energies: &[f32],
        statistics: Statistics,
        reduced: bool,
        out: &mut [f32],
    ) {
        let n = energies.len();
        let count = if reduced { n / 4 } else { n };
        for (m, value) in self.values[..count].iter_mut().enumerate() {
            *value = if reduced {
                let centre = 4 * m + 1;
                energies[(centre + n - 1) % n]
                    .max(energies[centre % n])
                    .max(energies[(centre + 1) % n])
            } else {
                energies[(m + 1) % n]
            };
        }
        let gain = 2.0 * statistics.amplitude / statistics.noise;
        bit_llrs(&mut self.values[..count], &mut self.spare, out, |energy| {
            ln_i0(gain * energy.sqrt())
        });
    }
}

/// Max-log LLRs of every bit, using `values` and `spare` as scratch. Bit `b` of the Gray code of
/// `m` is set where `m >> b` is 1 or 2 modulo 4, so once every block of `2^b` values is reduced
/// to its maximum, each side of bit `b` takes every block once, and the same pass merges
/// neighbours for the next bit. All bits together cost about two passes over the values.
fn bit_llrs(
    values: &mut [f32],
    spare: &mut [f32],
    out: &mut [f32],
    likelihood: impl Fn(f32) -> f32,
) {
    let bits = out.len();
    let mut len = values.len();
    let (mut blocks, mut merged) = (values, spare);
    for b in 0..bits {
        let next = len.div_ceil(2).min(merged.len());
        let (ones, zeros) = level(&blocks[..len], &mut merged[..next]);
        out[bits - 1 - b] = likelihood(ones.max(0.0)) - likelihood(zeros.max(0.0));
        len = next;
        std::mem::swap(&mut blocks, &mut merged);
    }
}

fn larger(a: f32, b: f32) -> f32 {
    if a > b { a } else { b }
}

const QUADS: usize = 8;

/// The largest block whose index is 1 or 2 modulo 4 and the largest of the others, kept in
/// independent lanes so the comparisons overlap, while neighbouring blocks merge into `merged`.
fn level(blocks: &[f32], merged: &mut [f32]) -> (f32, f32) {
    let mut ones = [f32::MIN; QUADS];
    let mut zeros = [f32::MIN; QUADS];
    let (groups, rest) = blocks.as_chunks::<{ 4 * QUADS }>();
    let (whole, tail) = merged.split_at_mut((2 * QUADS * groups.len()).min(merged.len()));
    for (group, pairs) in groups.iter().zip(whole.as_chunks_mut::<{ 2 * QUADS }>().0) {
        let quads = group.as_chunks::<4>().0;
        for (lane, (quad, pair)) in quads.iter().zip(pairs.as_chunks_mut::<2>().0).enumerate() {
            ones[lane] = larger(ones[lane], larger(quad[1], quad[2]));
            zeros[lane] = larger(zeros[lane], larger(quad[0], quad[3]));
            *pair = [larger(quad[0], quad[1]), larger(quad[2], quad[3])];
        }
    }
    for (k, &block) in rest.iter().enumerate() {
        let side = if matches!(k % 4, 1 | 2) {
            &mut ones[0]
        } else {
            &mut zeros[0]
        };
        *side = larger(*side, block);
        if let Some(slot) = tail.get_mut(k / 2) {
            *slot = if k % 2 == 0 {
                block
            } else {
                larger(*slot, block)
            };
        }
    }
    (
        ones.into_iter().fold(f32::MIN, larger),
        zeros.into_iter().fold(f32::MIN, larger),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Decision {
    pub nibble: u8,
    pub corrected: bool,
}

#[must_use]
pub(crate) fn decode_codeword(llrs: &[f32], parity_bits: u8) -> Decision {
    let len = codeword_len(parity_bits);
    let mut best = (0u8, f32::MIN);
    for nibble in 0..16u8 {
        let codeword = hamming_encode(nibble, parity_bits);
        let score: f32 = (0..len)
            .map(|i| {
                if codeword_bit(codeword, len, i) {
                    llrs[i]
                } else {
                    -llrs[i]
                }
            })
            .sum();
        if score > best.1 {
            best = (nibble, score);
        }
    }
    let codeword = hamming_encode(best.0, parity_bits);
    let corrected = (0..len).any(|i| codeword_bit(codeword, len, i) != (llrs[i] > 0.0));
    Decision {
        nibble: best.0,
        corrected,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sdrmm_modem::constellation::tables::gray;

    #[test]
    fn ln_i0_is_continuous_and_matches_known_values() {
        assert!(ln_i0(0.0).abs() < 1e-6);
        assert!((ln_i0(1.0) - 1.266_066_f32.ln()).abs() < 1e-4);
        assert!((ln_i0(3.749) - ln_i0(3.751)).abs() < 2e-3);
        assert!((ln_i0(10.0) - 2_815.717_f32.ln()).abs() < 1e-3);
    }

    #[test]
    fn a_clean_symbol_yields_confident_bits_of_its_gray_value() {
        let n = 128;
        let mut energies = vec![1.0f32; n];
        let symbol = 77usize;
        energies[symbol] = 400.0;
        let mut demapper = Demapper::new(n);
        let mut llrs = [0.0f32; 7];
        let statistics = Statistics {
            amplitude: 20.0,
            noise: 1.0,
        };
        demapper.llrs(&energies, statistics, false, &mut llrs);
        let data = gray((symbol - 1) as u32);
        for (j, llr) in llrs.iter().enumerate() {
            let set = (data >> (6 - j)) & 1 == 1;
            assert_eq!(*llr > 0.0, set, "bit {j}");
            assert!(llr.abs() > 5.0);
        }
    }

    #[test]
    fn a_reduced_symbol_tolerates_a_bin_of_error() {
        let n = 256;
        let mut demapper = Demapper::new(n);
        let statistics = Statistics {
            amplitude: 20.0,
            noise: 1.0,
        };
        let m = 21usize;
        for error in [-1i32, 0, 1] {
            let mut energies = vec![1.0f32; n];
            energies[(4 * m as i32 + 1 + error) as usize] = 400.0;
            let mut llrs = [0.0f32; 6];
            demapper.llrs(&energies, statistics, true, &mut llrs);
            let decided = llrs
                .iter()
                .fold(0u32, |acc, &l| (acc << 1) | u32::from(l > 0.0));
            assert_eq!(decided, gray(m as u32), "error {error}");
        }
    }

    #[test]
    fn each_bit_weighs_the_strongest_value_on_either_side_of_it() {
        let energies: Vec<f32> = (0..256u32).map(|m| ((m * 37 + 11) % 257) as f32).collect();
        let mut out = [0.0f32; 8];
        let mut spare = vec![0.0f32; energies.len() / 2];
        bit_llrs(&mut energies.clone(), &mut spare, &mut out, |energy| energy);
        for (j, llr) in out.iter().enumerate() {
            let bit = 7 - j;
            let side = |set: bool| {
                energies
                    .iter()
                    .enumerate()
                    .filter(|&(m, _)| ((gray(m as u32) >> bit) & 1 == 1) == set)
                    .map(|(_, &e)| e)
                    .fold(f32::MIN, f32::max)
            };
            assert_eq!(*llr, side(true) - side(false), "bit {bit}");
        }
    }

    #[test]
    fn soft_decoding_fixes_a_weak_wrong_bit() {
        for parity in 1..=4u8 {
            let len = codeword_len(parity);
            let codeword = hamming_encode(0b1011, parity);
            let mut llrs: Vec<f32> = (0..len)
                .map(|i| {
                    if codeword_bit(codeword, len, i) {
                        4.0
                    } else {
                        -4.0
                    }
                })
                .collect();
            llrs[len - 1] = -llrs[len - 1] * 0.25;
            let decision = decode_codeword(&llrs, parity);
            assert_eq!(decision.nibble, 0b1011, "parity {parity}");
            assert!(decision.corrected);
        }
    }
}
