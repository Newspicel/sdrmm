use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::fft::Transform;

#[must_use]
pub(crate) fn base_upchirp(chips: usize) -> Vec<Complex<f32>> {
    let n = chips as f64;
    (0..chips)
        .map(|k| {
            let k = k as f64;
            let turns = k * k / (2.0 * n) - k / 2.0;
            Complex::from_polar(1.0, (TAU * (turns - turns.floor())) as f32)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Slope {
    Up,
    Down,
}

pub(crate) struct Dechirper {
    up: Vec<Complex<f32>>,
    down: Vec<Complex<f32>>,
    fft: Transform,
    bins: Vec<Complex<f32>>,
    energies: Vec<f32>,
    sorted: Vec<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Peak {
    pub bin: usize,
    pub energy: f32,
    pub mean: f32,
}

impl Peak {
    #[must_use]
    pub(crate) fn ratio(self) -> f32 {
        if self.mean > 0.0 {
            self.energy / self.mean
        } else {
            0.0
        }
    }
}

impl Dechirper {
    #[must_use]
    pub(crate) fn new(chips: usize) -> Self {
        let up = base_upchirp(chips);
        let down = up.iter().map(|c| c.conj()).collect();
        let zero = Complex::new(0.0, 0.0);
        Self {
            up,
            down,
            fft: Transform::forward(chips),
            bins: vec![zero; chips],
            energies: vec![0.0; chips],
            sorted: vec![0.0; chips],
        }
    }

    pub(crate) fn transform(&mut self, samples: &[Complex<f32>], slope: Slope) {
        let reference = match slope {
            Slope::Up => &self.down,
            Slope::Down => &self.up,
        };
        for ((bin, &sample), &chirp) in self.bins.iter_mut().zip(samples).zip(reference) {
            *bin = sample * chirp;
        }
        self.fft.process(&mut self.bins);
        for (energy, bin) in self.energies.iter_mut().zip(&self.bins) {
            *energy = bin.norm_sqr();
        }
    }

    #[must_use]
    pub(crate) fn bins(&self) -> &[Complex<f32>] {
        &self.bins
    }

    #[must_use]
    pub(crate) fn energies(&self) -> &[f32] {
        &self.energies
    }

    #[must_use]
    pub(crate) fn mean(&self) -> f32 {
        self.energies.iter().sum::<f32>() / self.energies.len() as f32
    }

    #[must_use]
    pub(crate) fn peak(&self) -> Peak {
        let mut best = 0;
        for (k, &e) in self.energies.iter().enumerate() {
            if e > self.energies[best] {
                best = k;
            }
        }
        Peak {
            bin: best,
            energy: self.energies[best],
            mean: self.mean(),
        }
    }

    #[must_use]
    pub(crate) fn pair_peak(&self) -> Peak {
        let n = self.energies.len();
        let mut best = (0, f32::MIN);
        for k in 0..n {
            let pair = self.energies[k] + self.energies[(k + 1) % n];
            if pair > best.1 {
                best = (k, pair);
            }
        }
        Peak {
            bin: best.0,
            energy: best.1 / 2.0,
            mean: self.mean(),
        }
    }

    #[must_use]
    pub(crate) fn fraction(&self, bin: usize) -> f64 {
        let n = self.bins.len();
        let left = self.bins[(bin + n - 1) % n];
        let centre = self.bins[bin];
        let right = self.bins[(bin + 1) % n];
        let denominator = centre * 2.0 - left - right;
        if denominator.norm_sqr() <= f32::EPSILON * centre.norm_sqr().max(f32::MIN_POSITIVE) {
            return 0.0;
        }
        f64::from(((left - right) / denominator).re).clamp(-0.5, 0.5)
    }

    #[must_use]
    pub(crate) fn median_noise(&mut self) -> f32 {
        self.sorted.copy_from_slice(&self.energies);
        let middle = self.sorted.len() / 2;
        let (_, median, _) = self.sorted.select_nth_unstable_by(middle, f32::total_cmp);
        (*median / std::f32::consts::LN_2).max(f32::MIN_POSITIVE)
    }

    #[must_use]
    pub(crate) fn noise(&self, bin: usize) -> f32 {
        let n = self.energies.len();
        let total: f32 = self.energies.iter().sum();
        let near =
            self.energies[bin] + self.energies[(bin + 1) % n] + self.energies[(bin + n - 1) % n];
        ((total - near) / (n - 3) as f32).max(f32::MIN_POSITIVE)
    }
}

#[must_use]
pub(crate) fn wrap(value: f64, period: f64) -> f64 {
    value - (value / period).round() * period
}

#[must_use]
pub(crate) fn circular_distance(a: usize, b: usize, n: usize) -> usize {
    let d = (a + n - b) % n;
    d.min(n - d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn symbol(chips: usize, value: usize, cfo_bins: f64) -> Vec<Complex<f32>> {
        base_upchirp(chips)
            .into_iter()
            .enumerate()
            .map(|(k, c)| {
                let turns = (value as f64 + cfo_bins) * k as f64 / chips as f64;
                c * Complex::from_polar(1.0, (TAU * turns) as f32)
            })
            .collect()
    }

    #[test]
    fn a_dechirped_symbol_lands_in_its_bin() {
        let mut dechirper = Dechirper::new(128);
        for value in [0, 1, 37, 127] {
            dechirper.transform(&symbol(128, value, 0.0), Slope::Up);
            let peak = dechirper.peak();
            assert_eq!(peak.bin, value);
            assert!(peak.ratio() > 100.0);
        }
    }

    #[test]
    fn a_fractional_offset_is_interpolated() {
        let mut dechirper = Dechirper::new(256);
        for offset in [-0.4, -0.2, 0.0, 0.25, 0.45] {
            dechirper.transform(&symbol(256, 40, offset), Slope::Up);
            let peak = dechirper.peak();
            let estimate = peak.bin as f64 + dechirper.fraction(peak.bin) - 40.0;
            assert!((estimate - offset).abs() < 0.02, "{offset} read {estimate}");
        }
    }

    #[test]
    fn the_median_noise_ignores_a_strong_tone() {
        let mut dechirper = Dechirper::new(256);
        let mut state = 0x1234_5678u32;
        let noise: Vec<Complex<f32>> = (0..256)
            .map(|k| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                let phase = state as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                Complex::from_polar(0.1, phase) + symbol(256, 77, 0.0)[k] * 10.0
            })
            .collect();
        dechirper.transform(&noise, Slope::Up);
        let floor = dechirper.median_noise();
        let expected = 256.0 * 0.01;
        assert!(
            (floor / expected - 1.0).abs() < 0.5,
            "{floor} vs {expected}"
        );
    }

    #[test]
    fn a_downchirp_dechirps_against_the_upchirp() {
        let mut dechirper = Dechirper::new(128);
        let down: Vec<_> = base_upchirp(128).into_iter().map(|c| c.conj()).collect();
        dechirper.transform(&down, Slope::Down);
        assert_eq!(dechirper.peak().bin, 0);
        dechirper.transform(&down, Slope::Up);
        assert!(dechirper.peak().ratio() < 10.0);
    }
}
