use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::special::parabolic_peak;

use super::{
    resample::farrow_at,
    square_law::{square_law_line, square_law_offset},
};

const TRACK_BLOCK_SYMBOLS: usize = 64;

const MIN_RATE_SIGMAS: f64 = 4.0;

const STEP_SEARCH_OVERSAMPLE: usize = 4;

const MIN_RATE_EXCURSION_SAMPLES: f64 = 0.05;

/// A timing offset and the clock rate error it drifts at, referred to `at_sample`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TimingTrack {
    pub offset_samples: f64,
    pub at_sample: f64,
    pub rate: f64,
}

impl TimingTrack {
    #[must_use]
    pub fn offset_at(&self, sample: f64, sps: usize) -> f64 {
        (self.offset_samples + self.rate * (sample - self.at_sample)).rem_euclid(sps as f64)
    }

    pub fn resample(&self, filtered: &[Complex<f32>], sps: usize, out: &mut Vec<Complex<f32>>) {
        self.resample_spaced(filtered, sps, 1, out);
    }

    pub fn resample_spaced(
        &self,
        filtered: &[Complex<f32>],
        sps: usize,
        per_symbol: usize,
        out: &mut Vec<Complex<f32>>,
    ) {
        let advance = 1.0 - self.rate;
        let step = sps as f64 / advance;
        let origin = (self.offset_samples - self.rate * self.at_sample) / advance;
        if !(step.is_finite() && step > 0.0) || !origin.is_finite() {
            return;
        }
        let first = ((1.0 - origin) / step).ceil();
        let spacing = step / per_symbol.max(1) as f64;
        let mut k = first * per_symbol.max(1) as f64;
        loop {
            let position = origin + k * spacing;
            if (position as usize) + 2 >= filtered.len() {
                break;
            }
            if let Some(value) = farrow_at(filtered, position) {
                out.push(value);
            }
            k += 1.0;
        }
    }
}

pub fn square_law_track(
    filtered: &[Complex<f32>],
    sps: usize,
    lines: &mut Vec<Complex<f64>>,
) -> TimingTrack {
    let block = TRACK_BLOCK_SYMBOLS * sps;
    let blocks = filtered.len() / block;
    let whole = TimingTrack {
        offset_samples: square_law_offset(filtered, sps),
        at_sample: 0.0,
        rate: 0.0,
    };
    if blocks < 2 {
        return whole;
    }
    lines.clear();
    lines.extend(
        filtered
            .chunks_exact(block)
            .map(|b| square_law_line(b, sps)),
    );
    let centre = (blocks as f64 - 1.0) / 2.0;
    let coarse = strongest_step(lines, centre);
    let mean = destepped_sum(lines, coarse, centre);
    if mean.norm() <= 0.0 {
        return whole;
    }
    let fit = residual_slope(lines, coarse, mean, centre);
    let total = coarse + fit.slope;
    let excursion = (total * centre * sps as f64 / TAU).abs();
    let step = if total.abs() >= MIN_RATE_SIGMAS * fit.standard_error
        && excursion >= MIN_RATE_EXCURSION_SAMPLES
    {
        total
    } else {
        0.0
    };
    let acc = destepped_sum(lines, step, centre);
    let to_samples = -(sps as f64) / TAU;
    TimingTrack {
        offset_samples: (acc.arg() * to_samples).rem_euclid(sps as f64),
        at_sample: (centre + 0.5) * block as f64,
        rate: step * to_samples / block as f64,
    }
}

fn strongest_step(lines: &[Complex<f64>], centre: f64) -> f64 {
    let bins = STEP_SEARCH_OVERSAMPLE * lines.len();
    let power = |step: f64| destepped_sum(lines, step, centre).norm();
    let spacing = TAU / bins as f64;
    let step_of = |i: usize| (i as f64 - (bins / 2) as f64) * spacing;
    let Some((best, _)) = (0..bins)
        .map(|i| (i, power(step_of(i))))
        .max_by(|a, b| a.1.total_cmp(&b.1))
    else {
        return 0.0;
    };
    let shift = parabolic_peak(
        power(step_of(best) - spacing),
        power(step_of(best)),
        power(step_of(best) + spacing),
    );
    step_of(best) + shift * spacing
}

/// A weighted line through the per-block phase residuals, and its standard error.
struct SlopeFit {
    slope: f64,
    standard_error: f64,
}

fn residual_slope(
    lines: &[Complex<f64>],
    coarse: f64,
    mean: Complex<f64>,
    centre: f64,
) -> SlopeFit {
    let residual =
        |b: usize, l: Complex<f64>| (l * block_rotor(coarse, b, centre) * mean.conj()).arg();
    let (mut num, mut den, mut weight) = (0.0f64, 0.0f64, 0.0f64);
    for (b, &l) in lines.iter().enumerate() {
        let t = b as f64 - centre;
        let w = l.norm();
        num += w * t * residual(b, l);
        den += w * t * t;
        weight += w;
    }
    if den <= 0.0 || weight <= 0.0 || lines.len() < 3 {
        return SlopeFit {
            slope: 0.0,
            standard_error: f64::INFINITY,
        };
    }
    let slope = num / den;
    let scatter: f64 = lines
        .iter()
        .enumerate()
        .map(|(b, &l)| l.norm() * (residual(b, l) - slope * (b as f64 - centre)).powi(2))
        .sum::<f64>()
        / weight
        * lines.len() as f64
        / (lines.len() - 2) as f64;
    SlopeFit {
        slope,
        standard_error: (scatter * weight / lines.len() as f64 / den).sqrt(),
    }
}

/// Sum of the per-block lines with a phase step of `step` per block taken out.
fn destepped_sum(lines: &[Complex<f64>], step: f64, centre: f64) -> Complex<f64> {
    lines
        .iter()
        .enumerate()
        .map(|(b, &l)| l * block_rotor(step, b, centre))
        .sum()
}

fn block_rotor(step: f64, block: usize, centre: f64) -> Complex<f64> {
    Complex::from_polar(1.0, -step * (block as f64 - centre))
}
