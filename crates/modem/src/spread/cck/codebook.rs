use num_complex::Complex;

use super::{CHIPS, CckMode, MAX_BITS};
use crate::soft::Llr;

#[derive(Clone, Debug, PartialEq)]
pub struct Codebook {
    mode: CckMode,
    words: Vec<[Complex<f32>; CHIPS]>,
}

impl Codebook {
    #[must_use]
    pub fn new(mode: CckMode) -> Self {
        let words = (0..mode.words() as u32)
            .map(|index| Self::word(mode, index))
            .collect();
        Self { mode, words }
    }

    fn word(mode: CckMode, index: u32) -> [Complex<f32>; CHIPS] {
        let (p2, p3, p4) = match mode {
            CckMode::Bits8 => (
                quadrant(index & 3),
                quadrant((index >> 2) & 3),
                quadrant((index >> 4) & 3),
            ),
            CckMode::Bits4 => (
                quadrant(1 + 2 * (index & 1)),
                quadrant(0),
                quadrant(2 * ((index >> 1) & 1)),
            ),
        };
        [
            p2 * p3 * p4,
            p3 * p4,
            p2 * p4,
            -p4,
            p2 * p3,
            p3,
            -p2,
            Complex::new(1.0, 0.0),
        ]
    }

    #[must_use]
    pub fn mode(&self) -> CckMode {
        self.mode
    }

    #[must_use]
    pub fn words(&self) -> &[[Complex<f32>; CHIPS]] {
        &self.words
    }

    pub fn chips(&self, label: u32, symbol: usize, out: &mut [Complex<f32>; CHIPS]) {
        let word = &self.words[(label >> 2) as usize % self.words.len()];
        let rotation = quadrant(label + 2 * (symbol as u32 & 1)) * (CHIPS as f32).sqrt().recip();
        for (slot, &chip) in out.iter_mut().zip(word) {
            *slot = chip * rotation;
        }
    }

    /// Appends the chips of `labels`, the first sent as symbol `first_symbol`.
    pub(super) fn push_chips(
        &self,
        labels: &[u32],
        first_symbol: usize,
        out: &mut Vec<Complex<f32>>,
    ) {
        let mut word = [Complex::new(0.0, 0.0); CHIPS];
        for (k, &label) in labels.iter().enumerate() {
            self.chips(label, first_symbol + k, &mut word);
            out.extend_from_slice(&word);
        }
    }

    #[must_use]
    pub fn reference(&self, label: u32, symbol: usize) -> Complex<f32> {
        quadrant(label + 2 * (symbol as u32 & 1))
    }

    /// Correlates eight chips against every word, `out[i] = Σ y·conj(word_i) / √8`.
    ///
    /// Every chip of a word is a power of `j` in `φ2`, `φ3` and `φ4`, so the bank factors like a
    /// fast Walsh transform: partial sums over `φ2`, then `φ3`, then `φ4`, each step only
    /// quarter turns and additions.
    pub fn correlate(&self, chips: &[Complex<f32>; CHIPS], out: &mut [Complex<f32>]) {
        assert!(
            out.len() >= self.words.len(),
            "the bank has {} words; {} slots cannot hold it",
            self.words.len(),
            out.len()
        );
        let scale = (CHIPS as f32).sqrt().recip();
        let y: [Complex<f32>; CHIPS] = std::array::from_fn(|k| chips[k] * scale);
        match self.mode {
            CckMode::Bits8 => {
                for p2 in 0..4 {
                    let u = turn(y[0], p2) + y[1];
                    let v = turn(y[2], p2) - y[3];
                    let s = turn(y[4], p2) + y[5];
                    let t = y[7] - turn(y[6], p2);
                    for p3 in 0..4 {
                        let a = turn(u, p3) + v;
                        let b = turn(s, p3) + t;
                        for p4 in 0..4 {
                            out[(p2 | p3 << 2 | p4 << 4) as usize] = turn(a, p4) + b;
                        }
                    }
                }
            }
            CckMode::Bits4 => {
                for (half, p2) in [1, 3].into_iter().enumerate() {
                    let a = turn(y[0], p2) + y[1] + turn(y[2], p2) - y[3];
                    let b = turn(y[4], p2) + y[5] + y[7] - turn(y[6], p2);
                    out[half] = a + b;
                    out[half | 2] = b - a;
                }
            }
        }
    }

    #[must_use]
    pub fn decide(&self, bank: &[Complex<f32>], symbol: usize) -> u32 {
        let mut best = (0u32, f32::NEG_INFINITY);
        for label in 0..self.mode.alphabet() {
            let metric = self.metric(bank, label, symbol);
            if metric > best.1 {
                best = (label, metric);
            }
        }
        best.0
    }

    #[must_use]
    pub fn metric(&self, bank: &[Complex<f32>], label: u32, symbol: usize) -> f32 {
        let r = bank[(label >> 2) as usize % self.words.len()];
        match (label + 2 * (symbol as u32 & 1)) & 3 {
            0 => r.re,
            1 => r.im,
            2 => -r.re,
            _ => -r.im,
        }
    }

    pub fn llrs(&self, bank: &[Complex<f32>], symbol: usize, noise_var: f64, out: &mut [Llr]) {
        let bits = self.mode.bits_per_symbol();
        assert_eq!(out.len(), bits, "one LLR slot per codeword bit");
        assert!(
            noise_var.is_finite() && noise_var > 0.0,
            "noise_var is a measured variance; {noise_var} is not one"
        );
        let mut max0 = [f32::NEG_INFINITY; MAX_BITS];
        let mut max1 = [f32::NEG_INFINITY; MAX_BITS];
        for label in 0..self.mode.alphabet() {
            let metric = self.metric(bank, label, symbol);
            for k in 0..bits {
                if (label >> k) & 1 == 1 {
                    max1[k] = max1[k].max(metric);
                } else {
                    max0[k] = max0[k].max(metric);
                }
            }
        }
        let scale = 2.0 / noise_var;
        for (k, slot) in out.iter_mut().enumerate() {
            *slot = Llr((f64::from(max1[k] - max0[k]) * scale) as f32);
        }
    }
}

/// `z · conj(jⁿ)`: a quarter turn costs a swap and a sign.
fn turn(z: Complex<f32>, n: u32) -> Complex<f32> {
    match n & 3 {
        0 => z,
        1 => Complex::new(z.im, -z.re),
        2 => -z,
        _ => Complex::new(-z.im, z.re),
    }
}

fn quadrant(index: u32) -> Complex<f32> {
    match index & 3 {
        0 => Complex::new(1.0, 0.0),
        1 => Complex::new(0.0, 1.0),
        2 => Complex::new(-1.0, 0.0),
        _ => Complex::new(0.0, -1.0),
    }
}
