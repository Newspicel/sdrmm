use std::f64::consts::{PI, TAU};

const GRID_DENSITY: usize = 16;
const MAX_ITERS: usize = 64;
const CONVERGE_TOL: f64 = 1e-4;
const NODE_TOLERANCE: f64 = 1e-13;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum RemezError {
    #[error("equiripple filter requires odd tap count >= 3, got {0}")]
    InvalidTaps(usize),
    #[error("bands, desired and weights must be non-empty and match in length")]
    BandMismatch,
    #[error("band edges must be strictly increasing within [0, 0.5]")]
    InvalidBands,
    #[error("weights must be positive and finite")]
    InvalidWeights,
    #[error("remez exchange did not converge")]
    DidNotConverge,
}

struct Grid {
    cos_w: Vec<f64>,
    desired: Vec<f64>,
    weights: Vec<f64>,
}

/// Designs an optimal Type I equiripple linear-phase FIR filter via the Parks-McClellan
/// Remez exchange algorithm. Band edges are in cycles per sample `[0, 0.5]`.
///
/// Fails with [`RemezError::DidNotConverge`] when the taps outrun the bands: the best ripple
/// then lies below what `f64` resolves, and fewer taps meet the same response.
pub fn design_equiripple(
    taps: usize,
    bands: &[(f64, f64)],
    desired: &[f64],
    weights: &[f64],
) -> Result<Vec<f32>, RemezError> {
    if taps < 3 || taps.is_multiple_of(2) {
        return Err(RemezError::InvalidTaps(taps));
    }
    if bands.is_empty() || bands.len() != desired.len() || bands.len() != weights.len() {
        return Err(RemezError::BandMismatch);
    }
    let mut prev = -1.0f64;
    for &(lo, hi) in bands {
        if !(lo.is_finite() && hi.is_finite() && lo >= 0.0 && lo <= hi && hi <= 0.5 && lo > prev) {
            return Err(RemezError::InvalidBands);
        }
        prev = hi;
    }
    if weights.iter().any(|&w| !(w.is_finite() && w > 0.0)) {
        return Err(RemezError::InvalidWeights);
    }
    let r = taps / 2 + 1;
    let grid = build_grid(r, bands, |i, _| desired[i], |i, _| weights[i]);
    let coeffs = remez_exchange(taps, r, &grid)?;
    Ok(coeffs.into_iter().map(|v| v as f32).collect())
}

/// Equiripple low-pass FIR with passband `[0, pass]`, stopband `[stop, 0.5]` and stopband
/// ripple weight `stop_weight` relative to unit passband weight.
pub fn design_lowpass_equiripple(
    taps: usize,
    pass: f64,
    stop: f64,
    stop_weight: f64,
) -> Result<Vec<f32>, RemezError> {
    design_equiripple(
        taps,
        &[(0.0, pass), (stop, 0.5)],
        &[1.0, 0.0],
        &[1.0, stop_weight],
    )
}

/// Optimal equiripple half-band FIR (`4k + 3` taps) via the Vaidyanathan-Nguyen trick.
/// Every even offset from the centre tap is exactly zero, and passband/stopband ripples are equal.
pub fn design_halfband_equiripple(taps: usize, pass: f64) -> Result<Vec<f32>, RemezError> {
    if taps < 3 || taps % 4 != 3 {
        return Err(RemezError::InvalidTaps(taps));
    }
    if !(pass.is_finite() && pass > 0.0 && pass < 0.25) {
        return Err(RemezError::InvalidBands);
    }
    let sub_taps = taps / 2;
    let r = sub_taps / 2 + 1;
    let upper = 2.0 * pass;
    let grid = build_grid(
        r,
        &[(0.0, upper)],
        |_, f| 0.5 / (PI * f).cos(),
        |_, f| (PI * f).cos(),
    );
    let p = remez_exchange(sub_taps, r, &grid)?;
    let mut h = vec![0.0f32; taps];
    let center = taps / 2;
    h[center] = 0.5;
    let g_len = sub_taps + 1;
    for k in 0..g_len {
        let g = if k == 0 {
            0.5 * p[0]
        } else if k == sub_taps {
            0.5 * p[sub_taps - 1]
        } else {
            p[k].midpoint(p[k - 1])
        };
        h[2 * k] = g as f32;
    }
    Ok(h)
}

fn build_grid(
    r: usize,
    bands: &[(f64, f64)],
    desired: impl Fn(usize, f64) -> f64,
    weight: impl Fn(usize, f64) -> f64,
) -> Grid {
    let step = 0.5 / (GRID_DENSITY * r) as f64;
    let mut cos_w = Vec::new();
    let mut des = Vec::new();
    let mut wts = Vec::new();
    for (band_idx, &(lo, hi)) in bands.iter().enumerate() {
        let span = hi - lo;
        let points = ((span / step).ceil() as usize).max(1);
        for k in 0..=points {
            let f = if k == points {
                hi
            } else {
                lo + span * k as f64 / points as f64
            };
            cos_w.push((TAU * f).cos());
            des.push(desired(band_idx, f));
            wts.push(weight(band_idx, f));
        }
    }
    Grid {
        cos_w,
        desired: des,
        weights: wts,
    }
}

/// The polynomial through the first `r` extremal points that leaves the weighted error
/// `delta`, alternating in sign, on all `r + 1`.
struct Fit {
    x: Vec<f64>,
    y: Vec<f64>,
    weights: Vec<f64>,
    delta: f64,
}

impl Fit {
    fn through(grid: &Grid, extrema: &[usize]) -> Result<Self, RemezError> {
        let mut x: Vec<f64> = extrema.iter().map(|&index| grid.cos_w[index]).collect();
        let all = barycentric_weights(&x);
        let mut numerator = 0.0f64;
        let mut denominator = 0.0f64;
        for (k, (&index, &weight)) in extrema.iter().zip(&all).enumerate() {
            numerator += weight * grid.desired[index];
            denominator += alternating(k) * weight / grid.weights[index];
        }
        let delta = numerator / denominator;
        if !delta.is_finite() {
            return Err(RemezError::DidNotConverge);
        }
        x.pop();
        let y = extrema
            .iter()
            .take(x.len())
            .enumerate()
            .map(|(k, &index)| grid.desired[index] - alternating(k) * delta / grid.weights[index])
            .collect();
        Ok(Self {
            weights: barycentric_weights(&x),
            x,
            y,
            delta,
        })
    }

    fn at(&self, cos_w: f64) -> f64 {
        let mut numerator = 0.0f64;
        let mut denominator = 0.0f64;
        for ((&x, &y), &weight) in self.x.iter().zip(&self.y).zip(&self.weights) {
            let distance = cos_w - x;
            if distance.abs() < NODE_TOLERANCE {
                return y;
            }
            let term = weight / distance;
            numerator += term * y;
            denominator += term;
        }
        numerator / denominator
    }
}

fn alternating(k: usize) -> f64 {
    if k.is_multiple_of(2) { 1.0 } else { -1.0 }
}

fn remez_exchange(taps: usize, r: usize, grid: &Grid) -> Result<Vec<f64>, RemezError> {
    let points = grid.cos_w.len();
    if points <= r {
        return Err(RemezError::InvalidBands);
    }
    let mut extrema: Vec<usize> = (0..=r).map(|k| k * (points - 1) / r).collect();
    let mut error = vec![0.0f64; points];
    for _ in 0..MAX_ITERS {
        let fit = Fit::through(grid, &extrema)?;
        for (index, slot) in error.iter_mut().enumerate() {
            *slot = grid.weights[index] * (grid.desired[index] - fit.at(grid.cos_w[index]));
        }
        for (k, &index) in extrema.iter().enumerate() {
            error[index] = alternating(k) * fit.delta;
        }
        let peak = error
            .iter()
            .fold(0.0f64, |peak, value| peak.max(value.abs()));
        if peak <= fit.delta.abs() * (1.0 + CONVERGE_TOL) {
            return Ok(reconstruct_impulse(taps, &fit));
        }
        extrema = find_extrema(&error, r + 1).ok_or(RemezError::DidNotConverge)?;
    }
    Err(RemezError::DidNotConverge)
}

/// Barycentric weights of the nodes `x`, scaled so the largest is one. Summed as logarithms:
/// the plain products underflow past a hundred nodes.
fn barycentric_weights(x: &[f64]) -> Vec<f64> {
    let logs: Vec<(f64, f64)> = x
        .iter()
        .enumerate()
        .map(|(i, &node)| {
            x.iter().enumerate().filter(|&(j, _)| j != i).fold(
                (0.0f64, 1.0f64),
                |(log, sign), (_, &other)| {
                    let distance = node - other;
                    (log + distance.abs().ln(), sign * distance.signum())
                },
            )
        })
        .collect();
    let smallest = logs
        .iter()
        .fold(f64::INFINITY, |least, &(log, _)| least.min(log));
    logs.into_iter()
        .map(|(log, sign)| sign * (smallest - log).exp())
        .collect()
}

/// The `needed` alternating extrema of `err` with the largest errors, or `None` when it has
/// fewer.
fn find_extrema(err: &[f64], needed: usize) -> Option<Vec<usize>> {
    let n = err.len();
    let mut cands = Vec::with_capacity(needed + 8);
    if n >= 2 && ((err[0] > 0.0 && err[0] >= err[1]) || (err[0] < 0.0 && err[0] <= err[1])) {
        cands.push(0);
    }
    for i in 1..n - 1 {
        let e = err[i];
        let is_max = e >= err[i - 1] && e > err[i + 1] && e > 0.0;
        let is_min = e <= err[i - 1] && e < err[i + 1] && e < 0.0;
        if is_max || is_min {
            cands.push(i);
        }
    }
    if n >= 2 {
        let last = n - 1;
        if (err[last] > 0.0 && err[last] >= err[last - 1])
            || (err[last] < 0.0 && err[last] <= err[last - 1])
        {
            cands.push(last);
        }
    }
    let mut alt: Vec<usize> = Vec::with_capacity(cands.len());
    for idx in cands {
        if let Some(prev) = alt.last_mut()
            && err[*prev].signum() == err[idx].signum()
        {
            if err[idx].abs() > err[*prev].abs() {
                *prev = idx;
            }
            continue;
        }
        alt.push(idx);
    }
    while alt.len() > needed {
        if alt.len() == needed + 1 {
            let last_err = alt.last().map_or(0.0, |&i| err[i].abs());
            if err[alt[0]].abs() <= last_err {
                alt.remove(0);
            } else {
                alt.pop();
            }
        } else {
            let mut min_i = 0;
            let mut min_v = err[alt[0]].abs();
            for (i, &idx) in alt.iter().enumerate().skip(1) {
                if err[idx].abs() < min_v {
                    min_v = err[idx].abs();
                    min_i = i;
                }
            }
            if min_i == 0 {
                alt.remove(0);
            } else if min_i + 1 == alt.len() {
                alt.pop();
            } else if err[alt[min_i - 1]].abs() < err[alt[min_i + 1]].abs() {
                alt.remove(min_i);
                alt.remove(min_i - 1);
            } else {
                alt.remove(min_i + 1);
                alt.remove(min_i);
            }
        }
    }
    (alt.len() == needed).then_some(alt)
}

fn reconstruct_impulse(taps: usize, fit: &Fit) -> Vec<f64> {
    let m = taps / 2;
    let samples: Vec<f64> = (0..=m)
        .map(|k| fit.at((TAU * k as f64 / taps as f64).cos()))
        .collect();
    let mut h = vec![0.0f64; taps];
    let inv_n = 1.0 / taps as f64;
    for k in 0..=m {
        let mut sum = samples[0];
        for (j, &amp) in samples.iter().enumerate().skip(1) {
            sum += 2.0 * amp * (TAU * (j * k) as f64 * inv_n).cos();
        }
        let tap = sum * inv_n;
        h[m - k] = tap;
        h[m + k] = tap;
    }
    h
}

#[cfg(test)]
mod tests {
    use super::{super::response_db, *};
    use crate::filter::fir::design_halfband;

    #[test]
    fn equiripple_lowpass_beats_kaiser_and_holds_flat_passband() {
        let h = design_lowpass_equiripple(63, 0.15, 0.25, 10.0).unwrap();
        assert_eq!(h.len(), 63);
        assert!(h.iter().eq(h.iter().rev()));
        let mut max_pass = 0.0f64;
        for k in 0..=100 {
            let f = 0.15 * f64::from(k) / 100.0;
            max_pass = max_pass.max(response_db(&h, f).abs());
        }
        assert!(max_pass < 0.05, "passband ripple {max_pass} dB");
        let mut worst_stop = f64::NEG_INFINITY;
        for k in 0..=150 {
            let f = 0.25 + 0.25 * f64::from(k) / 150.0;
            worst_stop = worst_stop.max(response_db(&h, f));
        }
        assert!(worst_stop < -70.0, "stopband {worst_stop} dB");
    }

    #[test]
    fn equiripple_halfband_has_exact_zeros_and_deeper_stopband_than_kaiser() {
        let taps = 51;
        let pass = 0.15;
        let eq = design_halfband_equiripple(taps, pass).unwrap();
        let center = taps / 2;
        assert_eq!(eq[center], 0.5);
        for (i, &tap) in eq.iter().enumerate() {
            let even = i.abs_diff(center).is_multiple_of(2);
            assert_eq!(tap == 0.0, even && i != center, "index {i}: {tap}");
        }
        let kaiser = design_halfband(taps, 100.0);
        let stop = 0.5 - pass;
        let peak = |h: &[f32]| {
            (0..=200)
                .map(|k| response_db(h, stop + (0.5 - stop) * f64::from(k) / 200.0))
                .fold(f64::NEG_INFINITY, f64::max)
        };
        let eq_stop = peak(&eq);
        let kaiser_stop = peak(&kaiser);
        assert!(eq_stop < -95.0, "equiripple stopband {eq_stop} dB");
        assert!(
            eq_stop < kaiser_stop - 5.0,
            "equiripple {eq_stop} dB vs kaiser {kaiser_stop} dB"
        );
    }

    #[test]
    fn designs_are_equiripple_or_refused() {
        let deviation = |h: &[f32], low: f64, high: f64, want: f64| {
            (0..=400)
                .map(|k| {
                    let freq = low + (high - low) * f64::from(k) / 400.0;
                    (10f64.powf(response_db(h, freq) / 20.0) - want).abs()
                })
                .fold(0.0f64, f64::max)
        };
        for taps in [15, 63, 127] {
            for pass in [0.05, 0.2] {
                for width in [0.02, 0.05] {
                    for weight in [1.0, 10.0] {
                        let stop = pass + width;
                        let h = design_lowpass_equiripple(taps, pass, stop, weight).unwrap();
                        let in_pass = deviation(&h, 0.0, pass, 1.0);
                        let in_stop = deviation(&h, stop, 0.5, 0.0);
                        let balance = in_pass / (weight * in_stop);
                        assert!(
                            in_stop < 1e-6 || (0.7..1.5).contains(&balance),
                            "taps {taps} pass {pass} width {width} weight {weight}: {balance}"
                        );
                    }
                }
            }
        }
        assert_eq!(
            design_lowpass_equiripple(301, 0.1, 0.2, 1.0),
            Err(RemezError::DidNotConverge)
        );
        assert_eq!(
            design_halfband_equiripple(127, 0.05),
            Err(RemezError::DidNotConverge)
        );
    }

    #[test]
    fn invalid_specs_are_rejected() {
        assert_eq!(
            design_lowpass_equiripple(64, 0.1, 0.2, 1.0),
            Err(RemezError::InvalidTaps(64))
        );
        assert_eq!(
            design_halfband_equiripple(49, 0.15),
            Err(RemezError::InvalidTaps(49))
        );
        assert_eq!(
            design_equiripple(31, &[(0.0, 0.2)], &[1.0, 0.0], &[1.0]),
            Err(RemezError::BandMismatch)
        );
    }
}
