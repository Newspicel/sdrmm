use std::f64::consts::TAU;

use num_complex::Complex;

use super::{GfdmParams, GfdmPreamble};
use crate::framesync::{Repetition, RepetitionDetector, conj_product, derotate};

pub const TAP_FLOOR: f64 = 0.1;
/// Times the median lag a path must reach. Noise lags are exponential, so 20 medians
/// (14 means) passes one about once in a million, and a noise lag in the prefix no longer
/// starts the window early at 0 dB.
pub const NOISE_FLOOR: f64 = 20.0;
pub const DEFAULT_INTEGER_SPAN: i32 = 2;
/// Normalised correlation a preamble must reach. Over noise it is exponential with mean
/// `1 / n`; a frame at 0 dB sits near 0.5.
pub const DEFAULT_MIN_METRIC: f64 = 0.2;
pub const DEFAULT_BACKOFF: usize = 2;
/// Repetition peaks checked against the known preamble. At 0 dB payload and noise lift a
/// false peak of the half-block repetition metric over the true one about once in 100 frames.
pub const CANDIDATES: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GfdmAcquisition {
    pub preamble_start: usize,
    pub data_start: usize,
    pub cfo: f64,
    pub metric: f64,
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    lo: usize,
    strongest: usize,
    cfo: f64,
    metric: f64,
}

#[derive(Clone, Debug)]
pub struct GfdmSync {
    detector: RepetitionDetector,
    preamble: GfdmPreamble,
    cp: usize,
    integer_span: i32,
    min_metric: f64,
    backoff: usize,
    rotated: Vec<Complex<f32>>,
    energy: Vec<f64>,
    best: Vec<f64>,
    chosen: Vec<f64>,
    sorted: Vec<f64>,
}

impl GfdmSync {
    #[must_use]
    pub fn new(params: &GfdmParams) -> Self {
        let preamble = GfdmPreamble::new(params);
        let half = preamble.half();
        let span = lag_span(half, params.cp);
        Self {
            detector: RepetitionDetector::new(half, half),
            cp: params.cp,
            integer_span: DEFAULT_INTEGER_SPAN,
            min_metric: DEFAULT_MIN_METRIC,
            backoff: DEFAULT_BACKOFF.min(params.cp),
            rotated: vec![Complex::new(0.0, 0.0); span + params.block()],
            energy: vec![0.0; span],
            best: vec![0.0; span],
            chosen: vec![0.0; span],
            sorted: vec![0.0; span],
            preamble,
        }
    }

    #[must_use]
    pub fn with_integer_span(mut self, span: i32) -> Self {
        self.integer_span = span.max(0);
        self
    }

    #[must_use]
    pub fn with_min_metric(mut self, metric: f64) -> Self {
        self.min_metric = metric;
        self
    }

    #[must_use]
    pub fn preamble(&self) -> &GfdmPreamble {
        &self.preamble
    }

    #[must_use]
    pub fn cfo_range(&self) -> f64 {
        (f64::from(self.integer_span) + 0.5) / self.preamble.half() as f64
    }

    pub fn acquire(&mut self, x: &[Complex<f32>], search: usize) -> Option<GfdmAcquisition> {
        let chosen = self.best_candidate(x, search)?;
        if chosen.metric < self.min_metric {
            return None;
        }
        let start = chosen.lo + self.timing(chosen.strongest)?;
        let n = self.preamble.block().len();
        Some(GfdmAcquisition {
            preamble_start: start,
            data_start: start + n,
            cfo: self.fine_cfo(x, start, chosen.cfo),
            metric: chosen.metric,
        })
    }

    fn best_candidate(&mut self, x: &[Complex<f32>], search: usize) -> Option<Candidate> {
        let mut taken = [0usize; CANDIDATES];
        let mut chosen: Option<Candidate> = None;
        for k in 0..CANDIDATES {
            let Some(found) = self.detector.detect_apart(x, search, &taken[..k]) else {
                break;
            };
            taken[k] = found.offset;
            let Some(candidate) = self.verify(x, found) else {
                continue;
            };
            if chosen.is_none_or(|c| candidate.metric > c.metric) {
                chosen = Some(candidate);
                self.chosen.copy_from_slice(&self.best);
            }
        }
        chosen
    }

    fn verify(&mut self, x: &[Complex<f32>], found: Repetition) -> Option<Candidate> {
        let lo = found.offset.saturating_sub(self.preamble.half());
        let (cfo, peak) = self.pick_integer(x, lo, found.cfo)?;
        let strongest = self.best.iter().position(|&e| e >= peak)?;
        Some(Candidate {
            lo,
            strongest,
            cfo,
            metric: self.normalised(x, lo + strongest, peak),
        })
    }

    fn normalised(&self, x: &[Complex<f32>], at: usize, peak: f64) -> f64 {
        let n = self.preamble.block().len();
        let received: f64 = x
            .iter()
            .skip(at)
            .take(n)
            .map(|v| f64::from(v.norm_sqr()))
            .sum();
        let known: f64 = self
            .preamble
            .block()
            .iter()
            .map(|v| f64::from(v.norm_sqr()))
            .sum();
        if received > 0.0 {
            peak / (received * known)
        } else {
            0.0
        }
    }

    fn pick_integer(&mut self, x: &[Complex<f32>], lo: usize, coarse: f64) -> Option<(f64, f64)> {
        let n = self.preamble.block().len();
        if x.len() < lo + n {
            return None;
        }
        let half = self.preamble.half() as f64;
        let mut best = (coarse, f64::NEG_INFINITY);
        for m in -self.integer_span..=self.integer_span {
            let candidate = coarse + f64::from(m) / half;
            let peak = self.correlate(x, lo, candidate);
            if peak > best.1 {
                best = (candidate, peak);
                self.best.copy_from_slice(&self.energy);
            }
        }
        Some(best)
    }

    fn correlate(&mut self, x: &[Complex<f32>], lo: usize, cfo: f64) -> f64 {
        let n = self.preamble.block().len();
        derotate(x, lo, cfo, &mut self.rotated);
        let known = self.preamble.block();
        let mut peak = 0.0f64;
        for (t, slot) in self.energy.iter_mut().enumerate() {
            let mut acc = Complex::new(0.0f64, 0.0);
            for (&tap, &y) in known.iter().zip(&self.rotated[t..t + n]) {
                acc += conj_product(tap, y);
            }
            *slot = acc.norm_sqr();
            peak = peak.max(*slot);
        }
        peak
    }

    /// Paths are sought only within a prefix of the strongest lag: the half-period repeats,
    /// at a quarter of its power, would otherwise pass the tap floor as echoes.
    fn timing(&mut self, strongest: usize) -> Option<usize> {
        let floor = (TAP_FLOOR * self.chosen[strongest]).max(NOISE_FLOOR * self.median());
        let reach = self.cp.min(self.preamble.half() - 1);
        let lo = strongest.saturating_sub(reach);
        let hi = (strongest + reach).min(self.chosen.len() - 1);
        let paths = &self.chosen[lo..=hi];
        let first = lo + paths.iter().position(|&e| e > floor)?;
        let last = lo + paths.iter().rposition(|&e| e > floor)?;
        let earliest = first.saturating_sub(self.backoff);
        Some(earliest.max(last.saturating_sub(self.cp)).min(first))
    }

    fn median(&mut self) -> f64 {
        self.sorted.copy_from_slice(&self.chosen);
        let middle = self.sorted.len() / 2;
        *self.sorted.select_nth_unstable_by(middle, f64::total_cmp).1
    }

    fn fine_cfo(&self, x: &[Complex<f32>], start: usize, coarse: f64) -> f64 {
        let half = self.preamble.half();
        let mut acc = Complex::new(0.0f64, 0.0);
        for n in start..start + half {
            let (Some(&a), Some(&b)) = (x.get(n), x.get(n + half)) else {
                break;
            };
            acc += conj_product(a, b);
        }
        let period = half as f64;
        let measured = acc.arg() / (TAU * period);
        measured + ((coarse - measured) * period).round() / period
    }
}

/// Fine timing correlates the whole known block over a half period either side of the
/// repetition metric's plateau. At low SNR that plateau wanders by up to a half period, and
/// only the full-block correlation tells the true start (gain `n`) from its half-period
/// repeats (gain `n / 2`).
fn lag_span(half: usize, cp: usize) -> usize {
    2 * half + cp + 1
}
