//! Parallel code-phase search over every PRN on one stored snapshot, a few rows per millisecond
//! so a sky search never stalls the stream.

use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::fft::FftPair;

#[cfg(test)]
use super::code::sampled_code;
use super::code::{PRNS, fill_sampled_code};

pub(super) const SAMPLES_PER_MS: usize = 2_048;
pub(super) const RATE: f64 = 2_048_000.0;
pub(super) const SNAPSHOT_MS: usize = 8;
const ROWS_PER_MS: usize = 16;
const DOPPLER_STEP_HZ: i32 = 500;
const PEAK_GUARD_SAMPLES: usize = 4;
const CROSS_CORRELATION_MARGIN: f32 = 2.0;
const STALE_MS: u64 = 1_000;
const _: () = assert!(RATE as usize / SAMPLES_PER_MS == 2 * DOPPLER_STEP_HZ as usize);

/// A satellite found in the snapshot, its code start carried in absolute samples.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Found {
    pub prn: u8,
    pub doppler_hz: f64,
    pub code_start: f64,
    pub cn0_db_hz: f32,
}

pub(super) struct Search {
    span_hz: u32,
    threshold: f32,
    fft: FftPair,
    snapshot: Vec<Complex<f32>>,
    snapshot_start: u64,
    filled_ms: usize,
    spectra: Vec<Complex<f32>>,
    code: Vec<f32>,
    code_fft: Vec<Complex<f32>>,
    buf: Vec<Complex<f32>>,
    powers: Vec<f32>,
    prn: u8,
    searched: u8,
    job: usize,
}

impl Search {
    pub fn new(span_hz: u32, threshold: f32) -> Self {
        let rows = rows(span_hz);
        Self {
            span_hz,
            threshold,
            fft: FftPair::new(SAMPLES_PER_MS),
            snapshot: vec![Complex::default(); SNAPSHOT_MS * SAMPLES_PER_MS],
            snapshot_start: 0,
            filled_ms: 0,
            spectra: vec![Complex::default(); 2 * SNAPSHOT_MS * SAMPLES_PER_MS],
            code: vec![0.0; SAMPLES_PER_MS],
            code_fft: vec![Complex::default(); SAMPLES_PER_MS],
            buf: vec![Complex::default(); SAMPLES_PER_MS],
            powers: vec![0.0; rows * SAMPLES_PER_MS],
            prn: PRNS,
            searched: 0,
            job: 0,
        }
    }

    pub fn reset(&mut self) {
        self.filled_ms = 0;
        self.searched = 0;
        self.job = 0;
    }

    fn rows(&self) -> usize {
        self.powers.len() / SAMPLES_PER_MS
    }

    /// Takes one millisecond starting at absolute sample `start` and spends a bounded slice of
    /// work on the search, skipping PRNs `tracked` already holds.
    pub fn feed(
        &mut self,
        block: &[Complex<f32>],
        start: u64,
        tracked: impl Fn(u8) -> bool,
    ) -> Option<Found> {
        if self.filled_ms > 0
            && start.saturating_sub(self.snapshot_start) > STALE_MS * SAMPLES_PER_MS as u64
        {
            self.reset();
        }
        if self.filled_ms < SNAPSHOT_MS {
            self.store(block, start);
            if self.filled_ms == SNAPSHOT_MS {
                self.transform();
                self.next_prn(&tracked);
            }
            return None;
        }
        if self.job == 0 && tracked(self.prn) {
            self.next_prn(&tracked);
            return None;
        }
        let jobs = self.rows() * SNAPSHOT_MS;
        let end = (self.job + ROWS_PER_MS).min(jobs);
        for job in self.job..end {
            self.accumulate(job / SNAPSHOT_MS, job % SNAPSHOT_MS);
        }
        self.job = end;
        if end < jobs {
            return None;
        }
        let found = self.detect();
        self.next_prn(&tracked);
        found
    }

    fn store(&mut self, block: &[Complex<f32>], start: u64) {
        if self.filled_ms == 0 {
            self.snapshot_start = start;
        }
        let slot = &mut self.snapshot[self.filled_ms * SAMPLES_PER_MS..][..SAMPLES_PER_MS];
        slot.copy_from_slice(&block[..SAMPLES_PER_MS]);
        self.filled_ms += 1;
    }

    fn next_prn(&mut self, tracked: &impl Fn(u8) -> bool) {
        self.job = 0;
        self.powers.fill(0.0);
        loop {
            if self.searched >= PRNS {
                self.reset();
                return;
            }
            self.searched += 1;
            self.prn = self.prn % PRNS + 1;
            if !tracked(self.prn) {
                break;
            }
        }
        fill_sampled_code(self.prn, &mut self.code);
        for (bin, &chip) in self.code_fft.iter_mut().zip(&self.code) {
            *bin = Complex::new(chip, 0.0);
        }
        self.fft.forward(&mut self.code_fft);
    }

    fn lowest_hz(&self) -> i32 {
        -(self.span_hz as i32)
    }

    fn transform(&mut self) {
        let lowest = self.lowest_hz();
        for (index, spectrum) in self
            .spectra
            .as_chunks_mut::<SAMPLES_PER_MS>()
            .0
            .iter_mut()
            .enumerate()
        {
            let (ms, slot) = (index / 2, index % 2);
            let doppler = lowest + slot as i32 * DOPPLER_STEP_HZ;
            let samples = &self.snapshot[ms * SAMPLES_PER_MS..][..SAMPLES_PER_MS];
            wipe(samples, f64::from(doppler), spectrum);
            self.fft.forward(spectrum);
        }
    }

    fn correlate(&mut self, row: usize, ms: usize) {
        let spectrum = &self.spectra[(ms * 2 + row % 2) * SAMPLES_PER_MS..][..SAMPLES_PER_MS];
        let (head, tail) = spectrum.split_at((row / 2) % SAMPLES_PER_MS);
        let shifted = tail.iter().chain(head);
        for ((bin, &value), code) in self.buf.iter_mut().zip(shifted).zip(&self.code_fft) {
            *bin = value * code.conj();
        }
        self.fft.inverse(&mut self.buf);
    }

    fn accumulate(&mut self, row: usize, ms: usize) {
        self.correlate(row, ms);
        let powers = &mut self.powers[row * SAMPLES_PER_MS..][..SAMPLES_PER_MS];
        for (sum, value) in powers.iter_mut().zip(&self.buf) {
            *sum += value.norm_sqr();
        }
    }

    fn strongest_away_from(&self, phase: usize) -> f32 {
        self.powers
            .iter()
            .enumerate()
            .filter(|&(index, _)| {
                let gap = (index % SAMPLES_PER_MS).abs_diff(phase);
                gap.min(SAMPLES_PER_MS - gap) > PEAK_GUARD_SAMPLES
            })
            .map(|(_, &power)| power)
            .fold(0.0f32, f32::max)
    }

    fn detect(&self) -> Option<Found> {
        let floor = self.powers.iter().map(|&p| f64::from(p)).sum::<f64>()
            / self.powers.len().max(1) as f64;
        let (best, &peak) = self
            .powers
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))?;
        let (row, delay) = (best / SAMPLES_PER_MS, best % SAMPLES_PER_MS);
        let ratio = peak / (floor as f32).max(f32::MIN_POSITIVE);
        if ratio < self.threshold
            || peak < CROSS_CORRELATION_MARGIN * self.strongest_away_from(delay)
        {
            return None;
        }
        let coarse = f64::from(self.lowest_hz() + row as i32 * DOPPLER_STEP_HZ);
        let delay = delay as f64 + self.interpolate(row, delay);
        Some(Found {
            prn: self.prn,
            doppler_hz: coarse + self.fine_doppler(coarse, delay),
            code_start: self.snapshot_start as f64 + delay,
            cn0_db_hz: (10.0 * ((ratio - 1.0).max(1e-6) * 1_000.0).log10()).clamp(0.0, 65.0),
        })
    }

    /// Sub-sample offset of the correlation peak, a parabola through the amplitudes around it.
    fn interpolate(&self, row: usize, delay: usize) -> f64 {
        let at = |offset: usize| {
            let index = (delay + offset) % SAMPLES_PER_MS;
            f64::from(self.powers[row * SAMPLES_PER_MS + index]).sqrt()
        };
        let (before, peak, after) = (at(SAMPLES_PER_MS - 1), at(0), at(1));
        let curvature = before - 2.0 * peak + after;
        if curvature >= 0.0 {
            return 0.0;
        }
        (0.5 * (before - after) / curvature).clamp(-0.5, 0.5)
    }

    /// Residual Doppler from the phase advance between successive millisecond prompts. A data
    /// bit flip only negates one product, so it shrinks the sum but leaves its angle.
    fn fine_doppler(&self, coarse_hz: f64, delay: f64) -> f64 {
        let step = TAU * coarse_hz / RATE;
        let start = delay.round() as usize;
        let mut previous: Option<Complex<f64>> = None;
        let mut turn = Complex::<f64>::default();
        for ms in 0..SNAPSHOT_MS {
            let mut prompt = Complex::<f64>::default();
            for k in 0..SAMPLES_PER_MS {
                let n = ms * SAMPLES_PER_MS + k;
                let code =
                    self.code[(k + SAMPLES_PER_MS - start % SAMPLES_PER_MS) % SAMPLES_PER_MS];
                let sample = self.snapshot[n];
                let carrier = Complex::from_polar(1.0, -step * n as f64);
                prompt += Complex::new(f64::from(sample.re), f64::from(sample.im))
                    * carrier
                    * f64::from(code);
            }
            if let Some(previous) = previous {
                turn += prompt * previous.conj();
            }
            previous = Some(prompt);
        }
        turn.arg() / TAU * 1_000.0
    }
}

fn rows(span_hz: u32) -> usize {
    (2 * span_hz as usize) / DOPPLER_STEP_HZ as usize + 1
}

fn wipe(input: &[Complex<f32>], doppler_hz: f64, out: &mut [Complex<f32>]) {
    let step = TAU * doppler_hz / RATE;
    for (n, (slot, &sample)) in out.iter_mut().zip(input).enumerate() {
        let (sin, cos) = (step * n as f64).sin_cos();
        *slot = sample * Complex::new(cos as f32, -sin as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(prn: u8, doppler_hz: f64, delay: usize, ms: usize) -> Vec<Complex<f32>> {
        let code = sampled_code(prn, SAMPLES_PER_MS);
        (0..ms * SAMPLES_PER_MS)
            .map(|n| {
                let phase = TAU * doppler_hz * n as f64 / RATE;
                let chip = code[(n + SAMPLES_PER_MS - delay) % SAMPLES_PER_MS];
                Complex::new(phase.cos() as f32, phase.sin() as f32) * chip
            })
            .collect()
    }

    fn run(search: &mut Search, iq: &[Complex<f32>], tracked: impl Fn(u8) -> bool) -> Vec<Found> {
        iq.as_chunks::<SAMPLES_PER_MS>()
            .0
            .iter()
            .enumerate()
            .filter_map(|(ms, block)| search.feed(block, (ms * SAMPLES_PER_MS) as u64, &tracked))
            .collect()
    }

    #[test]
    fn finds_the_satellite_its_doppler_and_code_start() {
        let iq = signal(7, 1_230.0, 317, 400);
        let mut search = Search::new(5_000, 4.0);
        let found = run(&mut search, &iq, |_| false);
        assert!(found.iter().all(|hit| hit.prn == 7), "{found:?}");
        let hit = found[0];
        assert_eq!(hit.prn, 7);
        assert!((hit.doppler_hz - 1_230.0).abs() < 5.0, "{}", hit.doppler_hz);
        assert!((hit.code_start - 317.0).abs() < 0.5, "{}", hit.code_start);
        assert!(hit.cn0_db_hz > 45.0);
    }

    #[test]
    fn a_tracked_prn_is_never_searched_and_noise_finds_nothing() {
        let iq = signal(7, -2_000.0, 900, 400);
        let mut search = Search::new(5_000, 4.0);
        assert!(run(&mut search, &iq, |prn| prn == 7).is_empty());
        let noise = sdrmm_decoder_test_support::complex_noise(3, 0.5, 400 * SAMPLES_PER_MS);
        assert!(run(&mut Search::new(10_000, 4.0), &noise, |_| false).is_empty());
    }

    #[test]
    fn shifted_spectra_match_a_direct_doppler_wipe() {
        let mut search = Search::new(3_000, 4.0);
        search.snapshot =
            sdrmm_decoder_test_support::complex_noise(9, 0.5, SNAPSHOT_MS * SAMPLES_PER_MS);
        search.transform();
        search.next_prn(&|_| false);
        let mut direct = vec![Complex::default(); SAMPLES_PER_MS];
        for row in 0..search.rows() {
            let doppler = search.lowest_hz() + row as i32 * DOPPLER_STEP_HZ;
            search.correlate(row, 1);
            wipe(
                &search.snapshot[SAMPLES_PER_MS..2 * SAMPLES_PER_MS],
                f64::from(doppler),
                &mut direct,
            );
            search.fft.forward(&mut direct);
            for (bin, code) in direct.iter_mut().zip(&search.code_fft) {
                *bin *= code.conj();
            }
            search.fft.inverse(&mut direct);
            let peak = direct.iter().map(|v| v.norm()).fold(0.0f32, f32::max);
            let worst = direct
                .iter()
                .zip(&search.buf)
                .map(|(a, b)| (a - b).norm())
                .fold(0.0f32, f32::max);
            assert!(
                worst < 1e-3 * peak,
                "{doppler} Hz differs by {worst} of {peak}"
            );
        }
    }
}
