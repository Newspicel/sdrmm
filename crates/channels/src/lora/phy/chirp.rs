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

const ENERGY_LANES: usize = 8;

pub(crate) struct Dechirper {
    up: Vec<Complex<f32>>,
    down: Vec<Complex<f32>>,
    fft: Transform,
    bins: Vec<Complex<f32>>,
    energies: Vec<f32>,
    total: f32,
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
            total: 0.0,
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
        let mut lanes = [0.0f32; ENERGY_LANES];
        let energies = self.energies.as_chunks_mut::<ENERGY_LANES>().0.iter_mut();
        for (energies, bins) in energies.zip(self.bins.as_chunks::<ENERGY_LANES>().0) {
            for ((energy, bin), lane) in energies.iter_mut().zip(bins).zip(&mut lanes) {
                *energy = bin.norm_sqr();
                *lane += *energy;
            }
        }
        self.total = lanes.iter().sum();
    }

    /// How late, in chips, an upchirp whose wrap falls on chip `split` arrived, from its
    /// dechirped samples with the tone near `tone` bins. Lateness turns the part after the wrap
    /// by that fraction of a cycle while a frequency error turns both parts evenly, so the
    /// phase drift inside each part is measured and taken out of the step across the wrap.
    #[must_use]
    pub(crate) fn lateness(&self, samples: &[Complex<f32>], tone: f64, split: usize) -> f64 {
        let n = self.down.len();
        let bounds = [0, split / 2, split, split + (n - split) / 2, n];
        let step = Complex::<f32>::from_polar(1.0, (-TAU * tone / n as f64) as f32);
        let mut parts = [Complex::<f64>::new(0.0, 0.0); 4];
        for (part, ends) in parts.iter_mut().zip(bounds.windows(2)) {
            let turns = -tone * ends[0] as f64 / n as f64;
            let mut rotation = Complex::<f32>::from_polar(1.0, (TAU * turns.fract()) as f32);
            let mut sum = Complex::<f32>::new(0.0, 0.0);
            let range = ends[0]..ends[1];
            for (&sample, &chirp) in samples[range.clone()].iter().zip(&self.down[range]) {
                sum += sample * chirp * rotation;
                rotation *= step;
            }
            *part = Complex::new(f64::from(sum.re), f64::from(sum.im));
        }
        let drift = |first: Complex<f64>, second: Complex<f64>, length: usize| {
            let turn = second * first.conj();
            let spacing = length as f64 / 2.0 / n as f64;
            (
                turn.arg() / TAU / spacing.max(f64::MIN_POSITIVE),
                turn.norm(),
            )
        };
        let (before, weight_before) = drift(parts[0], parts[1], split);
        let (after, weight_after) = drift(parts[2], parts[3], n - split);
        let total = weight_before + weight_after;
        let offset = if total > 0.0 {
            (before * weight_before + after * weight_after) / total
        } else {
            0.0
        };
        let jump = ((parts[2] + parts[3]) * (parts[0] + parts[1]).conj()).arg() / TAU;
        jump - offset / 2.0
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
        self.total / self.energies.len() as f32
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
    pub(crate) fn fraction(&self, bin: usize) -> f64 {
        fraction_of(&self.bins, bin)
    }

    #[must_use]
    pub(crate) fn noise(&self, bin: usize) -> f32 {
        let n = self.energies.len();
        let total = self.total;
        let near =
            self.energies[bin] + self.energies[(bin + 1) % n] + self.energies[(bin + n - 1) % n];
        ((total - near) / (n - 3) as f32).max(f32::MIN_POSITIVE)
    }
}

/// Where between its neighbours the tone at `bin` sits, from -0.5 to 0.5 bins.
#[must_use]
pub(crate) fn fraction_of(bins: &[Complex<f32>], bin: usize) -> f64 {
    let n = bins.len();
    let left = bins[(bin + n - 1) % n];
    let centre = bins[bin];
    let right = bins[(bin + 1) % n];
    let denominator = centre * 2.0 - left - right;
    if denominator.norm_sqr() <= f32::EPSILON * centre.norm_sqr().max(f32::MIN_POSITIVE) {
        return 0.0;
    }
    f64::from(((left - right) / denominator).re).clamp(-0.5, 0.5)
}

#[must_use]
pub(crate) fn wrap(value: f64, period: f64) -> f64 {
    value - (value / period).round() * period
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
    fn the_noise_floor_ignores_a_strong_tone() {
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
        let floor = dechirper.noise(dechirper.peak().bin);
        let expected = 256.0 * 0.01;
        assert!(
            (floor / expected - 1.0).abs() < 0.5,
            "{floor} vs {expected}"
        );
    }

    #[test]
    fn a_late_symbol_turns_after_its_wrap() {
        let chips = 256;
        let value = 70;
        let mut dechirper = Dechirper::new(chips);
        for late in [-0.3, 0.0, 0.2] {
            let delayed: Vec<Complex<f32>> = (0..chips)
                .map(|k| {
                    let t = k as f64 - late;
                    let n = chips as f64;
                    let turns = t * t / (2.0 * n) + (value as f64 / n - 0.5) * t
                        - (t - (n - value as f64)).max(0.0);
                    Complex::from_polar(1.0, (TAU * turns) as f32)
                })
                .collect();
            dechirper.transform(&delayed, Slope::Up);
            let bin = dechirper.peak().bin;
            let tone = bin as f64 + dechirper.fraction(bin);
            let read = dechirper.lateness(&delayed, tone, chips - value);
            assert!((read - late).abs() < 0.02, "{late} read {read}");
        }
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
