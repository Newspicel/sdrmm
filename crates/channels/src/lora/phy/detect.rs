use num_complex::Complex;
use sdrmm_dsp::special::gamma::gamma_q;

use super::chirp::{Dechirper, fraction_of};

/// Search windows whose energies are summed before a preamble is called. A preamble too weak to
/// clear a threshold in each window still piles up in one pair of bins over several.
pub(crate) const DETECT_WINDOWS: usize = 4;
const DETECT_FALSE_ALARM: f64 = 1e-3;
const CONTINUE_FALSE_ALARM: f64 = 1e-2;
const STEADY_FALSE_ALARM: f64 = 0.1;
/// Windows that must each show the pair on their own, so one strong payload symbol is no
/// preamble.
pub(crate) const STEADY_WINDOWS: usize = DETECT_WINDOWS - 1;
const BISECTIONS: usize = 60;
const RESUM_WINDOWS: u64 = 256;
const LEVEL_CEILING: f64 = 1_000.0;

/// The level that the sum of `cells` unit-mean exponential energies, the noise-only energy of
/// that many FFT bins, exceeds with `probability` over `trials` independent tries.
#[must_use]
pub(crate) fn noise_level(cells: u32, trials: usize, probability: f64) -> f32 {
    let target = probability / trials.max(1) as f64;
    let (mut low, mut high) = (0.0, LEVEL_CEILING);
    for _ in 0..BISECTIONS {
        let middle = 0.5 * (low + high);
        if gamma_q(f64::from(cells), middle) > target {
            low = middle;
        } else {
            high = middle;
        }
    }
    high as f32
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Levels {
    pub detect: f32,
    pub carry_on: f32,
    pub steady: f32,
}

impl Levels {
    #[must_use]
    pub(crate) fn new(chips: usize) -> Self {
        let pairs = 2 * DETECT_WINDOWS as u32;
        Self {
            detect: noise_level(pairs, chips, DETECT_FALSE_ALARM),
            carry_on: noise_level(2, 1, CONTINUE_FALSE_ALARM),
            steady: noise_level(2, 1, STEADY_FALSE_ALARM),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Detection {
    pub bin: usize,
    pub level: f32,
    pub steady: usize,
}

/// The energy spectra of the latest contiguous search windows.
pub(crate) struct Accumulator {
    chips: usize,
    spectra: Vec<f32>,
    sum: Vec<f32>,
    means: [f32; DETECT_WINDOWS],
    filled: usize,
    slot: usize,
    added: u64,
    expected: Option<u64>,
}

impl Accumulator {
    #[must_use]
    pub(crate) fn new(chips: usize) -> Self {
        Self {
            chips,
            spectra: vec![0.0; DETECT_WINDOWS * chips],
            sum: vec![0.0; chips],
            means: [0.0; DETECT_WINDOWS],
            filled: 0,
            slot: 0,
            added: 0,
            expected: None,
        }
    }

    /// Sums the kept spectra afresh, so the rounding of the running sum cannot pile up.
    fn resum(&mut self) {
        self.sum.fill(0.0);
        for spectrum in self.spectra.chunks_exact(self.chips) {
            for (total, &energy) in self.sum.iter_mut().zip(spectrum) {
                *total += energy;
            }
        }
    }

    pub(crate) fn reset(&mut self) {
        self.expected = None;
    }

    /// Adds the window starting at chip `start` and, once enough contiguous windows are in,
    /// returns the strongest pair of adjacent bins across them, in units of the mean bin energy,
    /// and how many windows reach `steady` at that pair on their own.
    pub(crate) fn add(
        &mut self,
        start: u64,
        energies: &[f32],
        mean: f32,
        steady: f32,
    ) -> Option<Detection> {
        if self.expected != Some(start) {
            self.filled = 0;
            self.sum.fill(0.0);
            self.spectra.fill(0.0);
        }
        self.expected = Some(start + self.chips as u64);
        let row = self.slot * self.chips;
        let oldest = &mut self.spectra[row..row + self.chips];
        for ((total, &energy), slot) in self.sum.iter_mut().zip(energies).zip(oldest) {
            *total += energy - *slot;
            *slot = energy;
        }
        self.means[self.slot] = mean;
        self.slot = (self.slot + 1) % DETECT_WINDOWS;
        self.filled = (self.filled + 1).min(DETECT_WINDOWS);
        self.added += 1;
        if self.added.is_multiple_of(RESUM_WINDOWS) {
            self.resum();
        }
        if self.filled < DETECT_WINDOWS {
            return None;
        }
        let unit = self.means.iter().sum::<f32>() / DETECT_WINDOWS as f32;
        let (bin, pair) = best_pair(&self.sum);
        let next = (bin + 1) % self.chips;
        let steady = self
            .spectra
            .chunks_exact(self.chips)
            .zip(self.means)
            .filter(|(spectrum, mean)| spectrum[bin] + spectrum[next] >= steady * mean)
            .count();
        (unit > 0.0).then(|| Detection {
            bin,
            level: pair / unit,
            steady,
        })
    }
}

/// The bin `k` that maximises `energies[k] + energies[k + 1]`, wrapping at the end, and that sum.
#[must_use]
pub(crate) fn best_pair(energies: &[f32]) -> (usize, f32) {
    let n = energies.len();
    let mut best = (0, f32::MIN);
    for (k, pair) in energies.windows(2).enumerate() {
        let sum = pair[0] + pair[1];
        if sum > best.1 {
            best = (k, sum);
        }
    }
    let last = energies[n - 1] + energies[0];
    if last > best.1 {
        best = (n - 1, last);
    }
    best
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Sighting {
    pub bins: f64,
    pub level: f32,
    pub window_start: u64,
}

/// The down-dechirped spectrum of the previous window. The two full downchirps of the SFD span
/// one window whole and parts of its neighbours, so a pair of windows sees most of their energy.
pub(crate) struct DownHistory {
    bins: Vec<Complex<f32>>,
    energies: Vec<f32>,
    mean: f32,
    start: Option<u64>,
}

impl DownHistory {
    #[must_use]
    pub(crate) fn new(chips: usize) -> Self {
        Self {
            bins: vec![Complex::new(0.0, 0.0); chips],
            energies: vec![0.0; chips],
            mean: 0.0,
            start: None,
        }
    }

    /// The strongest downchirp pair across this window and the one before it, anchored on
    /// whichever of the two holds more of it. The current window then becomes the previous.
    pub(crate) fn sighting(&mut self, start: u64, dechirper: &Dechirper) -> Sighting {
        let n = self.bins.len();
        let current = dechirper.energies();
        let mean = dechirper.mean();
        let contiguous = self.start == Some(start.wrapping_sub(n as u64));
        if !contiguous {
            self.energies.fill(0.0);
            self.mean = mean;
        }
        let pair = |energies: &[f32], k: usize| energies[k] + energies[(k + 1) % n];
        let mut best = (0, f32::MIN);
        for k in 0..n {
            let sum = pair(current, k) + pair(&self.energies, k);
            if sum > best.1 {
                best = (k, sum);
            }
        }
        let k = best.0;
        let earlier = pair(&self.energies, k) > pair(current, k);
        let (bins, energies, window_start) = if earlier {
            (&self.bins[..], &self.energies[..], start - n as u64)
        } else {
            (dechirper.bins(), current, start)
        };
        let peak = if energies[(k + 1) % n] > energies[k] {
            (k + 1) % n
        } else {
            k
        };
        let unit = 0.5 * (mean + self.mean);
        let sighting = Sighting {
            bins: peak as f64 + fraction_of(bins, peak),
            level: if unit > 0.0 { best.1 / unit } else { 0.0 },
            window_start,
        };
        self.bins.copy_from_slice(dechirper.bins());
        self.energies.copy_from_slice(current);
        self.mean = mean;
        self.start = Some(start);
        sighting
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_noise_level_matches_the_exponential_tail() {
        let level = noise_level(1, 1, 1e-3);
        assert!((f64::from(level) - 1e3f64.ln()).abs() < 1e-3, "{level}");
        let pair = noise_level(2, 1, 1e-2);
        let tail = (1.0 + f64::from(pair)) * (-f64::from(pair)).exp();
        assert!((tail - 1e-2).abs() < 1e-5, "{pair}");
        assert!(noise_level(8, 4096, 1e-3) > noise_level(8, 128, 1e-3));
    }

    #[test]
    fn one_strong_window_is_not_steady() {
        let chips = 64;
        let mut accumulator = Accumulator::new(chips);
        let quiet = vec![1.0f32; chips];
        let mut loud = quiet.clone();
        loud[9] = 40.0;
        for window in 0..DETECT_WINDOWS as u64 - 1 {
            accumulator.add(window * 64, &quiet, 1.0, 5.0);
        }
        let found = accumulator.add(3 * 64, &loud, 1.0, 5.0);
        assert!(found.is_some_and(|f| f.steady == 1 && (f.bin == 8 || f.bin == 9)));
    }

    #[test]
    fn the_best_pair_wraps_around_the_spectrum() {
        assert_eq!(best_pair(&[5.0, 0.0, 0.0, 4.0]), (3, 9.0));
        assert_eq!(best_pair(&[0.0, 3.0, 4.0, 0.0]), (1, 7.0));
    }

    #[test]
    fn a_steady_weak_pair_is_found_only_once_enough_windows_are_in() {
        let chips = 64;
        let mut accumulator = Accumulator::new(chips);
        let mut energies = vec![1.0f32; chips];
        energies[20] = 3.0;
        energies[21] = 3.0;
        for window in 0..DETECT_WINDOWS as u64 - 1 {
            assert_eq!(accumulator.add(window * 64, &energies, 1.0, 5.0), None);
        }
        let found = accumulator.add(3 * 64, &energies, 1.0, 5.0);
        assert_eq!(
            found,
            Some(Detection {
                bin: 20,
                level: 24.0,
                steady: DETECT_WINDOWS,
            })
        );
        assert_eq!(accumulator.add(5 * 64, &energies, 1.0, 5.0), None);
    }
}
