use super::{MIN_STAGE_TAPS, PASSBAND_FRAC, STOPBAND_DB, is_unity};
use crate::{
    Decimator,
    filter::fir::{design_halfband, design_lowpass_kaiser, halfband_taps, kaiser_taps},
    filter::resamp::{Prototype, taps_per_phase},
};

/// How close to the image of the output band a stage may let its stopband start, as a share
/// of the band rate.
const PROTECT_FRAC: f64 = 0.5;
/// The decimation must factor into these primes, so every stage stays short.
const SMOOTH_PRIMES: [usize; 6] = [2, 3, 5, 7, 11, 13];
/// The most decimations below the exact quotient the plan weighs against each other.
const MAX_CANDIDATES: usize = 256;
/// Relative distance from a whole number below which a quotient counts as whole.
const WHOLE_QUOTIENT: f64 = 1e-9;
/// The weight of a fractional resampler tap against an integer stage tap in a plan's cost.
const FRACTION_TAP_COST: f64 = 2.0;

/// One stage's filter: a low-pass, or for a stage by two possibly a cheaper half-band.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum StageFilter {
    Lowpass { taps: usize, cutoff: f64 },
    Halfband { taps: usize },
}

impl StageFilter {
    pub(super) fn taps(self) -> usize {
        match self {
            Self::Lowpass { taps, .. } | Self::Halfband { taps } => taps,
        }
    }

    /// The nonzero taps, which is what one output costs.
    fn products(self) -> usize {
        match self {
            Self::Lowpass { taps, .. } => taps,
            Self::Halfband { taps } => taps / 2 + 2,
        }
    }

    fn design(self) -> Vec<f32> {
        match self {
            Self::Lowpass { taps, cutoff } => design_lowpass_kaiser(taps, cutoff, STOPBAND_DB),
            Self::Halfband { taps } => design_halfband(taps, STOPBAND_DB),
        }
    }
}

/// The stage factors, largest first, of the cheapest smooth decimation for `quotient`.
pub(super) fn stage_factors(quotient: f64) -> Vec<usize> {
    prime_factors_desc(integer_decimation(quotient))
}

pub(super) fn integer_decimation(quotient: f64) -> usize {
    let rounded = quotient.round();
    let mut candidate = if (quotient - rounded).abs() < WHOLE_QUOTIENT {
        rounded as usize
    } else {
        quotient.floor() as usize
    };
    let minimum = (candidate / 2)
        .max(candidate.saturating_sub(MAX_CANDIDATES))
        .max(1);
    let mut best = 1usize << candidate.max(1).ilog2();
    let mut best_cost = decimation_cost(quotient, best);
    while candidate >= minimum {
        if is_smooth(candidate) {
            let cost = decimation_cost(quotient, candidate);
            if cost < best_cost {
                best = candidate;
                best_cost = cost;
            }
        }
        candidate -= 1;
    }
    best
}

pub(super) fn stage(input_rate: f64, factor: usize, band_rate: f64) -> Decimator {
    Decimator::new(
        &stage_filter(input_rate, factor, band_rate).design(),
        factor,
    )
}

/// The cheaper of the stage's low-pass and, for a stage by two, a half-band that meets the same
/// passband and protection edges at the same attenuation. Planning prices every stage as its
/// low-pass, so a cheaper stage by two never changes the plan.
pub(super) fn stage_filter(input_rate: f64, factor: usize, band_rate: f64) -> StageFilter {
    let lowpass = lowpass_stage(input_rate, factor, band_rate);
    let (pass, stop) = stage_edges(input_rate, factor, band_rate);
    let transition = 0.5 - 2.0 * pass.max(0.5 - stop);
    if factor != 2 || transition <= 0.0 {
        return lowpass;
    }
    let halfband = StageFilter::Halfband {
        taps: halfband_taps(transition, STOPBAND_DB).max(MIN_STAGE_TAPS),
    };
    if halfband.products() < lowpass.products() {
        halfband
    } else {
        lowpass
    }
}

fn is_smooth(mut n: usize) -> bool {
    for factor in SMOOTH_PRIMES {
        while n.is_multiple_of(factor) {
            n /= factor;
        }
    }
    n == 1
}

fn decimation_cost(quotient: f64, decimation: usize) -> f64 {
    let mut rate = quotient;
    let mut cost = 0.0;
    for factor in prime_factors_desc(decimation) {
        let taps = lowpass_stage(rate, factor, 1.0).taps();
        rate /= factor as f64;
        cost += taps as f64 * rate / quotient;
    }
    if !is_unity(rate) {
        cost += FRACTION_TAP_COST
            * taps_per_phase(rate.recip(), Prototype::Kaiser(STOPBAND_DB)) as f64
            / quotient;
    }
    cost
}

fn prime_factors_desc(mut n: usize) -> Vec<usize> {
    let mut factors = Vec::new();
    let mut d = 2;
    while d * d <= n {
        while n.is_multiple_of(d) {
            factors.push(d);
            n /= d;
        }
        d += 1;
    }
    if n > 1 {
        factors.push(n);
    }
    factors.reverse();
    factors
}

fn lowpass_stage(input_rate: f64, factor: usize, band_rate: f64) -> StageFilter {
    let (pass, stop) = stage_edges(input_rate, factor, band_rate);
    StageFilter::Lowpass {
        taps: kaiser_taps(stop - pass, STOPBAND_DB).max(MIN_STAGE_TAPS),
        cutoff: f64::midpoint(pass, stop),
    }
}

/// The edge the passband must reach and the one beyond which nothing may fold into the
/// protected band, as fractions of the stage's input rate.
fn stage_edges(input_rate: f64, factor: usize, band_rate: f64) -> (f64, f64) {
    let pass = PASSBAND_FRAC * band_rate / input_rate;
    let stop = (input_rate / factor as f64 - PROTECT_FRAC * band_rate) / input_rate;
    (pass, stop)
}
