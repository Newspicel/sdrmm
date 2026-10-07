use super::code::{codeword_bit, codeword_len, gray, hamming_encode};

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
    likelihoods: Vec<f32>,
    values: Vec<f32>,
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
            likelihoods: vec![0.0; chips],
            values: vec![0.0; chips],
        }
    }

    pub(crate) fn llrs(
        &mut self,
        energies: &[f32],
        statistics: Statistics,
        reduced: bool,
        out: &mut [f32],
    ) {
        let n = energies.len();
        let gain = 2.0 * statistics.amplitude / statistics.noise;
        for (likelihood, &energy) in self.likelihoods.iter_mut().zip(energies) {
            *likelihood = ln_i0(gain * energy.sqrt());
        }
        let count = if reduced { n / 4 } else { n };
        for (m, value) in self.values[..count].iter_mut().enumerate() {
            *value = if reduced {
                let centre = 4 * m + 1;
                self.likelihoods[(centre + n - 1) % n]
                    .max(self.likelihoods[centre % n])
                    .max(self.likelihoods[(centre + 1) % n])
            } else {
                self.likelihoods[(m + 1) % n]
            };
        }
        bit_llrs(&self.values[..count], out);
    }
}

fn bit_llrs(values: &[f32], out: &mut [f32]) {
    let bits = out.len();
    let mut ones = [f32::MIN; MAX_BITS];
    let mut zeros = [f32::MIN; MAX_BITS];
    for (m, &value) in values.iter().enumerate() {
        let data = gray(m as u16);
        for b in 0..bits {
            let slot = if (data >> b) & 1 == 1 {
                &mut ones[b]
            } else {
                &mut zeros[b]
            };
            *slot = slot.max(value);
        }
    }
    for (j, llr) in out.iter_mut().enumerate() {
        let b = bits - 1 - j;
        *llr = ones[b] - zeros[b];
    }
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
        let data = gray((symbol - 1) as u16);
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
                .fold(0u16, |acc, &l| (acc << 1) | u16::from(l > 0.0));
            assert_eq!(decided, gray(m as u16), "error {error}");
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
