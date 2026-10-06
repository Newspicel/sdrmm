use std::collections::BTreeMap;

use num_complex::Complex;
use sdrmm_modem_test_support::ber::rng::Rng;

pub const LEVELS: usize = 6;
pub const COPIES: usize = 8;
const ACTIVE_RANGE_DB: f64 = 20.0;
const BLOCKS_PER_SECOND: f64 = 1_000.0;

#[derive(Clone, Copy)]
pub struct Ladder {
    pub high_db: f64,
    pub low_db: f64,
}

#[derive(Clone, Copy)]
pub struct Run {
    pub snr_db: f64,
    pub seed: u64,
}

impl Ladder {
    pub fn levels(self) -> Vec<f64> {
        let step = (self.low_db - self.high_db) / (LEVELS - 1) as f64;
        (0..LEVELS)
            .map(|level| self.high_db + step * level as f64)
            .collect()
    }

    pub fn runs(self) -> Vec<Run> {
        self.levels()
            .into_iter()
            .flat_map(|snr_db| (0..COPIES).map(move |copy| (snr_db, copy)))
            .enumerate()
            .map(|(index, (snr_db, _))| Run {
                snr_db,
                seed: index as u64 + 1,
            })
            .collect()
    }

    pub fn note(self) -> String {
        format!(
            "{} runs, SNR {} to {} dB.",
            LEVELS * COPIES,
            self.high_db,
            self.low_db
        )
    }
}

pub fn active_power(iq: &[Complex<f32>], rate: f64) -> f64 {
    let block = ((rate / BLOCKS_PER_SECOND).round() as usize).max(1);
    let powers: Vec<f64> = iq
        .chunks(block)
        .map(|chunk| {
            chunk.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / chunk.len() as f64
        })
        .collect();
    let top = powers.iter().copied().fold(0.0, f64::max);
    let floor = top / 10f64.powf(ACTIVE_RANGE_DB / 10.0);
    let active: Vec<f64> = powers.into_iter().filter(|&p| p >= floor).collect();
    active.iter().sum::<f64>() / active.len().max(1) as f64
}

pub fn add(iq: &[Complex<f32>], power: f64, run: Run) -> Vec<Complex<f32>> {
    let noise_power = power / 10f64.powf(run.snr_db / 10.0);
    let real = iq.iter().all(|s| s.im == 0.0);
    let (sigma_re, sigma_im) = if real {
        (noise_power.sqrt(), 0.0)
    } else {
        let sigma = (noise_power / 2.0).sqrt();
        (sigma, sigma)
    };
    let mut rng = Rng::new(run.seed);
    iq.iter()
        .map(|&s| {
            let (re, im) = rng.normal_pair();
            s + Complex::new((re * sigma_re) as f32, (im * sigma_im) as f32)
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub kept: usize,
    pub expected: usize,
}

impl Tally {
    pub fn of(clean: &[String], noisy: &[String], unique: bool) -> Self {
        let expected = counts(clean, unique);
        let found = counts(noisy, unique);
        Self {
            kept: expected
                .iter()
                .map(|(key, &n)| n.min(found.get(key).copied().unwrap_or(0)))
                .sum(),
            expected: expected.values().sum(),
        }
    }

    pub fn add(self, other: Self) -> Self {
        Self {
            kept: self.kept + other.kept,
            expected: self.expected + other.expected,
        }
    }

    pub fn percent(self) -> f64 {
        if self.expected == 0 {
            return 0.0;
        }
        100.0 * self.kept as f64 / self.expected as f64
    }
}

fn counts(keys: &[String], unique: bool) -> BTreeMap<&str, usize> {
    let mut counts = BTreeMap::new();
    for key in keys {
        let n = counts.entry(key.as_str()).or_insert(0);
        *n = if unique { 1 } else { *n + 1 };
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(list: &[&str]) -> Vec<String> {
        list.iter().map(|&k| k.to_owned()).collect()
    }

    #[test]
    fn the_ladder_spans_its_range_in_equal_steps() {
        let ladder = Ladder {
            high_db: 20.0,
            low_db: 0.0,
        };
        assert_eq!(ladder.levels(), [20.0, 16.0, 12.0, 8.0, 4.0, 0.0]);
        let runs = ladder.runs();
        assert_eq!(runs.len(), LEVELS * COPIES);
        assert_eq!(runs[COPIES].snr_db, 16.0);
        assert_ne!(runs[0].seed, runs[1].seed);
    }

    #[test]
    fn only_messages_seen_without_noise_count() {
        let clean = keys(&["a", "b"]);
        let tally = Tally::of(&clean, &keys(&["a", "a", "junk"]), true);
        assert_eq!(
            tally,
            Tally {
                kept: 1,
                expected: 2
            }
        );
        assert_eq!(tally.percent(), 50.0);
    }

    #[test]
    fn repeated_frames_count_up_to_the_clean_count() {
        let clean = keys(&["call", "call", "call"]);
        let tally = Tally::of(&clean, &keys(&["call", "call"]), false);
        assert_eq!(tally.kept, 2);
        assert_eq!(tally.expected, 3);
    }

    #[test]
    fn a_tool_that_never_decodes_scores_zero() {
        assert_eq!(Tally::default().percent(), 0.0);
    }

    #[test]
    fn noise_lands_at_the_requested_snr() {
        let rate = 48_000.0;
        let silent = vec![Complex::default(); 48_000];
        let mut tone = vec![Complex::new(0.5f32, 0.0); 48_000];
        tone.extend(silent);
        let power = active_power(&tone, rate);
        assert!((power - 0.25).abs() < 1e-9, "{power}");
        let run = Run {
            snr_db: 10.0,
            seed: 7,
        };
        let noisy = add(&vec![Complex::default(); 200_000], power, run);
        let measured = noisy.iter().map(|s| f64::from(s.norm_sqr())).sum::<f64>() / 200_000.0;
        assert!((10.0 * (power / measured).log10() - 10.0).abs() < 0.1);
    }

    #[test]
    fn real_audio_gets_real_noise_of_the_same_power() {
        let run = Run {
            snr_db: 0.0,
            seed: 3,
        };
        let real = add(&vec![Complex::new(0.1f32, 0.0); 100_000], 1.0, run);
        assert!(real.iter().all(|s| s.im == 0.0));
        let complex = add(&vec![Complex::new(0.1f32, 0.1); 100_000], 1.0, run);
        assert!(complex.iter().any(|s| s.im != 0.1));
        let power = |iq: &[Complex<f32>], offset: Complex<f32>| {
            iq.iter()
                .map(|&s| f64::from((s - offset).norm_sqr()))
                .sum::<f64>()
                / iq.len() as f64
        };
        assert!((power(&real, Complex::new(0.1, 0.0)) - 1.0).abs() < 0.02);
        assert!((power(&complex, Complex::new(0.1, 0.1)) - 1.0).abs() < 0.02);
    }
}
