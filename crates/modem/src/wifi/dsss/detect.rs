use num_complex::Complex;
use sdrmm_dsp::{Decimator, MovingSum};

use super::SYMBOL;

pub(super) const DETECT_SYMBOLS: usize = 8;
pub(super) const DETECT_SPAN: usize = SYMBOL * DETECT_SYMBOLS;
const DETECT_THRESHOLD: f32 = 0.2;
const MIN_POWER: f32 = 1e-9;
const CHUNK: usize = 256;

/// The Barker correlation and its normalised power at every sample, computed block-wise (a SIMD
/// FIR and a moving sum), so the detector and the symbol reader look values up instead of
/// recomputing twenty taps per sample.
///
/// `values[n]` correlates `history[n..n + SYMBOL]`; `rho[n]` is its power over the window's
/// energy, 1 for a clean Barker symbol.
pub(super) struct Matched {
    filter: Decimator,
    power: MovingSum<f32>,
    fresh: Vec<Complex<f32>>,
    norms: Vec<f32>,
    energies: Vec<f32>,
    pub(super) values: Vec<Complex<f32>>,
    pub(super) rho: Vec<f32>,
    warmup: usize,
}

impl Matched {
    pub(super) fn new(template: &[f32; SYMBOL], capacity: usize) -> Self {
        let reversed: Vec<f32> = template.iter().rev().copied().collect();
        Self {
            filter: Decimator::new(&reversed, 1),
            power: MovingSum::new(SYMBOL),
            fresh: Vec::new(),
            norms: Vec::new(),
            energies: Vec::new(),
            values: Vec::with_capacity(capacity),
            rho: Vec::with_capacity(capacity),
            warmup: SYMBOL - 1,
        }
    }

    pub(super) fn reset(&mut self) {
        self.filter.reset();
        self.norms.clear();
        self.values.clear();
        self.rho.clear();
        self.warmup = SYMBOL - 1;
    }

    pub(super) fn push(&mut self, iq: &[Complex<f32>]) {
        self.filter.process(iq, &mut self.fresh);
        let carry = self.norms.len().saturating_sub(SYMBOL - 1);
        self.norms.drain(..carry);
        self.norms.extend(iq.iter().map(Complex::norm_sqr));
        self.power.process(&self.norms, &mut self.energies);
        let skip = self.warmup.min(self.fresh.len());
        self.warmup -= skip;
        let fresh = &self.fresh[skip..];
        self.values.extend_from_slice(fresh);
        self.rho
            .extend(fresh.iter().zip(&self.energies).map(|(m, &energy)| {
                if energy > MIN_POWER {
                    m.norm_sqr() / (energy * SYMBOL as f32)
                } else {
                    0.0
                }
            }));
    }

    pub(super) fn len(&self) -> usize {
        self.values.len()
    }

    pub(super) fn drain(&mut self, count: usize) {
        self.values.drain(..count);
        self.rho.drain(..count);
    }
}

/// The first sample in `from..to` whose phase holds a strong Barker correlation over the last
/// `DETECT_SYMBOLS` symbols and peaks against both neighbouring phases. `from` must leave a full
/// `DETECT_SPAN` of `rho` behind it.
pub(super) fn first_peak(rho: &[f32], from: usize, to: usize) -> Option<usize> {
    if from >= to {
        return None;
    }
    let threshold = DETECT_THRESHOLD * DETECT_SYMBOLS as f32;
    let mut comb = [0.0f32; CHUNK + SYMBOL];
    let low = (from + 1).checked_sub(SYMBOL)?;
    for (phase, slot) in comb[..SYMBOL].iter_mut().enumerate() {
        *slot = (0..DETECT_SYMBOLS)
            .map(|symbol| rho[low + phase - symbol * SYMBOL])
            .sum();
    }
    let mut base = from;
    while base < to {
        let len = (to - base).min(CHUNK);
        extend_comb(rho, base + 1, &mut comb[..len + SYMBOL]);
        let strong = comb[SYMBOL - 1..len + SYMBOL - 1]
            .iter()
            .fold(false, |any, &value| any | (value > threshold));
        if strong {
            let hit = (0..len).find(|&i| {
                let value = comb[i + SYMBOL - 1];
                value > threshold && value >= comb[i + SYMBOL - 2] && value >= comb[i]
            });
            if let Some(i) = hit {
                return Some(base + i);
            }
        }
        comb.copy_within(len..len + SYMBOL, 0);
        base += len;
    }
    None
}

/// Fills `comb[SYMBOL..]` with the comb sums from sample `first` on, each from the one a symbol
/// earlier: one sample enters, the one `DETECT_SPAN` back leaves.
fn extend_comb(rho: &[f32], first: usize, comb: &mut [f32]) {
    let count = comb.len() - SYMBOL;
    let entering = &rho[first..first + count];
    let leaving = &rho[first - DETECT_SPAN..first - DETECT_SPAN + count];
    for (i, (&enter, &leave)) in entering.iter().zip(leaving).enumerate() {
        comb[i + SYMBOL] = comb[i] + enter - leave;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wifi::dsss::{barker, template};

    fn signal(len: usize) -> Vec<Complex<f32>> {
        (0..len)
            .map(|k| Complex::new((k as f32 * 0.37).sin(), (k as f32 * 0.11 + 1.0).cos()))
            .collect()
    }

    #[test]
    fn the_lookups_equal_the_direct_correlation_across_ragged_blocks() {
        let template = template(&barker().unwrap());
        let x = signal(3_000);
        let mut matched = Matched::new(&template, 4_000);
        let mut at = 0;
        for len in [5, 7, 30, 1, 900, 64, 2_000] {
            let end = (at + len).min(x.len());
            matched.push(&x[at..end]);
            at = end;
        }
        assert_eq!(matched.len(), x.len() + 1 - SYMBOL);
        for n in 0..matched.len() {
            let window = &x[n..n + SYMBOL];
            let direct: Complex<f32> = window.iter().zip(&template).map(|(&s, &t)| s * t).sum();
            let energy: f32 = window.iter().map(Complex::norm_sqr).sum();
            assert!((matched.values[n] - direct).norm() < 1e-4, "{n}");
            let rho = direct.norm_sqr() / (energy * SYMBOL as f32);
            assert!((matched.rho[n] - rho).abs() < 1e-4, "{n}");
        }
    }

    #[test]
    fn the_peak_is_the_first_strong_local_maximum_of_the_comb() {
        let rho: Vec<f32> = (0..2_000)
            .map(|n| {
                if n > 900 && n % SYMBOL == 7 {
                    0.9
                } else {
                    0.01 * (n % 3) as f32
                }
            })
            .collect();
        let comb = |n: usize| -> f32 { (0..DETECT_SYMBOLS).map(|k| rho[n - k * SYMBOL]).sum() };
        let threshold = DETECT_THRESHOLD * DETECT_SYMBOLS as f32;
        let want = (DETECT_SPAN..1_900)
            .find(|&n| comb(n) > threshold && comb(n) >= comb(n - 1) && comb(n) >= comb(n - 19));
        assert!(want.is_some());
        assert_eq!(first_peak(&rho, DETECT_SPAN, 1_900), want);
        assert_eq!(first_peak(&rho, 1_900, 1_900), None);
    }
}
