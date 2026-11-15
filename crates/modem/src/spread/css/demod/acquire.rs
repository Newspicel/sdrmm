use std::f64::consts::TAU;

use num_complex::Complex;

use super::{super::MAX_CLOCK_PPM, CssDemod, argmax_bin, timing_range, wrap};

const INLIER_BINS: f64 = 1.5;

const VOTE_BINS: f64 = 1.0;

/// How far each fine pass may tilt the wrap phase, in turns across the whole preamble. Half a
/// turn is the most a phase measured modulo one turn can tell apart from its alias.
const FINE_SPANS: [f64; 3] = [0.0, 0.5, 0.5];

const SIGNIFICANT_SIGMAS: f64 = 3.0;

const SLOPE_REACH: i32 = 2;

/// A line fit that reports its own spread needs a point beyond the two that set it.
const MIN_FIT_POINTS: f64 = 3.0;

/// Leads whose preamble phases match well enough to be checked on their own symbol grid.
const LEAD_CANDIDATES: usize = 4;

/// Lead corrections, in samples, tried on each candidate's own grid.
const FRACTIONAL_LEADS: [f64; 5] = [-0.5, -0.25, 0.0, 0.25, 0.5];

/// Window grids, in symbols, so every lead lies within a quarter symbol of one of them.
const GRID_OFFSETS: [f64; 2] = [0.0, 0.5];

const CANDIDATE_SLOTS: usize = GRID_OFFSETS.len() * LEAD_CANDIDATES;

/// Whole samples the fine lines may still be off once the coarse lead is known.
const SLIP_REACH: i64 = 1;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Point {
    index: f64,
    value: f64,
    weight: f64,
    residual: f64,
}

/// A lead and carrier the preamble agrees with, before the fine fit.
#[derive(Clone, Copy, Debug)]
struct Coarse {
    timing: Line,
    frequency: f64,
}

#[derive(Clone, Copy, Debug)]
struct Line {
    intercept: f64,
    slope: f64,
}

impl Line {
    fn at(self, index: f64) -> f64 {
        self.intercept + self.slope * index
    }
}

impl CssDemod {
    pub fn estimate_origin(&mut self, iq: &[Complex<f32>], preamble: &[u32]) -> usize {
        self.restart();
        let n = self.params.chips() as f64;
        let coarse = self.coarse_lead(iq, preamble);
        let mut lines = (
            coarse.timing,
            Line {
                intercept: wrap(coarse.frequency, n).round(),
                slope: 0.0,
            },
        );
        let length = preamble.len().saturating_sub(1).max(1) as f64;
        for span in FINE_SPANS {
            lines = self.fine_lines(iq, preamble, lines.0, lines.1, span / length);
        }
        let slip = self.whole_sample_slip(iq, preamble, lines.0, lines.1);
        lines.0.intercept += slip;
        lines.1.intercept += slip;
        self.settle(preamble.len(), lines.0, lines.1)
    }

    fn coarse_line(
        &mut self,
        iq: &[Complex<f32>],
        preamble: &[u32],
        offset: f64,
        drift: f64,
    ) -> Line {
        let n = self.params.chips() as f64;
        self.points.clear();
        for (k, &known) in preamble.iter().enumerate() {
            let index = k as f64;
            self.load(iq, offset + index * (n - drift), 0.0, n - drift);
            self.dechirp();
            let bin = argmax_bin(&self.energies);
            let tone = f64::from(bin) + self.fraction(bin as usize);
            self.points.push(Point {
                index,
                value: (tone - f64::from(known) * self.stretch).rem_euclid(n),
                weight: f64::from(self.energies[bin as usize]),
                residual: 0.0,
            });
        }
        let seed = self.hough(n);
        let reach = self.clock_reach();
        let line = inlier_line(&self.points, seed, n, reach);
        Line {
            intercept: line.intercept,
            slope: (line.slope + drift).clamp(-reach, reach),
        }
    }

    fn hough(&self, n: f64) -> Line {
        let count = self.points.len().max(1) as f64;
        let step = 0.5 / count;
        let reach = (n * MAX_CLOCK_PPM * 1e-6 / step).ceil() as i64;
        let mut best = (
            0usize,
            f64::INFINITY,
            Line {
                intercept: 0.0,
                slope: 0.0,
            },
        );
        for i in -reach..=reach {
            let slope = i as f64 * step;
            for p in &self.points {
                let key = p.value - slope * p.index;
                let votes = self
                    .points
                    .iter()
                    .filter(|q| wrap(q.value - slope * q.index - key, n).abs() <= VOTE_BINS)
                    .count();
                if votes > best.0 || (votes == best.0 && slope.abs() < best.1) {
                    best = (
                        votes,
                        slope.abs(),
                        Line {
                            intercept: key,
                            slope,
                        },
                    );
                }
            }
        }
        best.2
    }

    /// The lead whose own symbol grid holds the preamble most coherently.
    fn coarse_lead(&mut self, iq: &[Complex<f32>], preamble: &[u32]) -> Coarse {
        let n = self.params.chips() as f64;
        let mut best = (
            f64::NEG_INFINITY,
            Coarse {
                timing: Line {
                    intercept: 0.0,
                    slope: n,
                },
                frequency: 0.0,
            },
        );
        for candidate in self.coarse_candidates(iq, preamble).into_iter().flatten() {
            let (strength, refined) = self.aligned_coherence(iq, preamble, candidate);
            if strength > best.0 {
                best = (strength, refined);
            }
        }
        best.1
    }

    /// Leads whose preamble phases agree best, searched from window grids a half symbol
    /// apart so no lead is cut near the middle of every window.
    fn coarse_candidates(
        &mut self,
        iq: &[Complex<f32>],
        preamble: &[u32],
    ) -> [Option<Coarse>; CANDIDATE_SLOTS] {
        let n = self.params.chips() as f64;
        let mut candidates = [None; CANDIDATE_SLOTS];
        let mut used = 0;
        for offset in GRID_OFFSETS.map(|o| o * n) {
            let first = self.coarse_line(iq, preamble, offset, 0.0);
            let bins = self.coarse_line(iq, preamble, offset, first.slope);
            let (grid, peaks) = self.coarse_grid(iq, preamble, bins, offset);
            for (_, index) in peaks.into_iter().filter(|&(s, _)| s.is_finite()) {
                let lead = wrap(grid.intercept + centred(index, n), n);
                candidates[used] = Some(Coarse {
                    timing: Line {
                        intercept: lead,
                        slope: grid.slope,
                    },
                    frequency: bins.intercept + lead - offset,
                });
                used += 1;
            }
        }
        candidates
    }

    /// The window grid from `offset` whose drift makes the preamble phases agree best, and the
    /// leads its phases favour.
    fn coarse_grid(
        &mut self,
        iq: &[Complex<f32>],
        preamble: &[u32],
        bins: Line,
        offset: f64,
    ) -> (Line, [(f64, usize); LEAD_CANDIDATES]) {
        let n = self.params.chips() as f64;
        let step = 0.5 / preamble.len().max(1) as f64;
        let spin = Line {
            intercept: bins.intercept,
            slope: 0.0,
        };
        let mut best = (
            f64::NEG_INFINITY,
            Line {
                intercept: offset,
                slope: n - bins.slope,
            },
            [(f64::NEG_INFINITY, 0usize); LEAD_CANDIDATES],
        );
        for nudge in -SLOPE_REACH..=SLOPE_REACH {
            let slope = bins.slope + f64::from(nudge) * step;
            if slope.abs() > self.clock_reach() {
                continue;
            }
            let grid = Line {
                intercept: offset,
                slope: n - slope,
            };
            let (strength, _) = self.phase_index(iq, preamble, grid, spin);
            if strength > best.0 {
                best = (strength, grid, strongest_peaks(&self.hypotheses));
            }
        }
        (best.1, best.2)
    }

    /// Windows cut on a lead's own symbol grid keep every peak whole, so the true lead outscores
    /// a sidelobe that only matched phases on a grid cut across symbol edges. The peaks are
    /// loaded once and the lead is tried a fraction of a sample either side.
    fn aligned_coherence(
        &mut self,
        iq: &[Complex<f32>],
        preamble: &[u32],
        candidate: Coarse,
    ) -> (f64, Coarse) {
        let n = self.params.chips() as f64;
        let frequency = Line {
            intercept: candidate.frequency,
            slope: 0.0,
        };
        let mut sums = [Complex::new(0.0, 0.0); FRACTIONAL_LEADS.len()];
        let mut previous: Option<(Complex<f64>, u32)> = None;
        for (k, &known) in preamble.iter().enumerate() {
            let peak = self.known_peak(iq, k, known, candidate.timing, frequency);
            if let Some((before, value)) = previous {
                let change = f64::from(known) - f64::from(value);
                for (sum, fraction) in sums.iter_mut().zip(FRACTIONAL_LEADS) {
                    *sum += peak
                        * before.conj()
                        * Complex::from_polar(1.0, TAU * change * fraction / n);
                }
            }
            previous = Some((peak, known));
        }
        let (strength, fraction) = sums
            .iter()
            .zip(FRACTIONAL_LEADS)
            .map(|(sum, fraction)| (sum.norm(), fraction))
            .fold((f64::NEG_INFINITY, 0.0), |best, next| {
                if next.0 > best.0 { next } else { best }
            });
        let refined = Coarse {
            timing: Line {
                intercept: candidate.timing.intercept + fraction,
                slope: candidate.timing.slope,
            },
            frequency: candidate.frequency + fraction,
        };
        (strength, refined)
    }

    /// The fine fit knows the lead only modulo one sample; the coarse lead is already within
    /// [`SLIP_REACH`] of it, so a farther sidelobe of the preamble's phase steps cannot pull it.
    fn whole_sample_slip(
        &mut self,
        iq: &[Complex<f32>],
        preamble: &[u32],
        timing: Line,
        frequency: Line,
    ) -> f64 {
        let n = self.params.chips() as i64;
        self.phase_index(iq, preamble, timing, frequency);
        let mut best = (f64::NEG_INFINITY, 0i64);
        for slip in -SLIP_REACH..=SLIP_REACH {
            let strength = self.hypotheses[slip.rem_euclid(n) as usize].norm_sqr();
            if strength > best.0 {
                best = (strength, slip);
            }
        }
        best.1 as f64
    }

    fn clock_reach(&self) -> f64 {
        timing_range(self.params.chips())
    }

    fn phase_index(
        &mut self,
        iq: &[Complex<f32>],
        preamble: &[u32],
        timing: Line,
        frequency: Line,
    ) -> (f64, usize) {
        self.hypotheses.fill(Complex::new(0.0, 0.0));
        let mut previous: Option<(Complex<f64>, u32)> = None;
        for (k, &known) in preamble.iter().enumerate() {
            let peak = self.known_peak(iq, k, known, timing, frequency);
            if let Some((before, value)) = previous {
                self.accumulate_hypotheses(peak * before.conj(), known, value);
            }
            previous = Some((peak, known));
        }
        let index = argmax_norm(&self.hypotheses);
        (self.hypotheses[index].norm(), index)
    }

    fn known_peak(
        &mut self,
        iq: &[Complex<f32>],
        symbol: usize,
        known: u32,
        timing: Line,
        frequency: Line,
    ) -> Complex<f64> {
        let index = symbol as f64;
        self.load(iq, timing.at(index), frequency.at(index), timing.slope);
        self.dechirp();
        let n = self.params.chips();
        let bin = self.bins[(f64::from(known) * self.stretch).round() as usize % n];
        Complex::new(f64::from(bin.re), f64::from(bin.im))
    }

    fn accumulate_hypotheses(&mut self, product: Complex<f64>, value: u32, before: u32) {
        let n = self.hypotheses.len();
        let change = f64::from(value) - f64::from(before);
        let step = Complex::from_polar(1.0, TAU * change / n as f64);
        let mut turn = product;
        for slot in &mut self.hypotheses {
            *slot += turn;
            turn *= step;
        }
    }

    fn fine_lines(
        &mut self,
        iq: &[Complex<f32>],
        preamble: &[u32],
        timing: Line,
        frequency: Line,
        reach: f64,
    ) -> (Line, Line) {
        let n = self.params.chips() as f64;
        self.points.clear();
        for (k, &known) in preamble.iter().enumerate() {
            self.load(
                iq,
                timing.at(k as f64),
                frequency.at(k as f64),
                timing.slope,
            );
            self.dechirp();
            let split = self.wrap_point(known);
            if split == 0 || split >= self.params.chips() {
                continue;
            }
            let resolved = self.split_tone(f64::from(known) * self.stretch, split);
            self.points.push(Point {
                index: k as f64,
                value: resolved.late,
                weight: resolved.weight,
                residual: wrap(resolved.tone - f64::from(known) * self.stretch, n),
            });
        }
        let late = circular_line(&self.points, reach);
        let unwrapped = |p: &Point| late.at(p.index) + wrap(p.value - late.at(p.index), 1.0);
        let n = self.params.chips() as f64;
        let points = &self.points;
        let timing_fit = fit(|| {
            points
                .iter()
                .map(|p| (p.index, timing.at(p.index) + unwrapped(p), p.weight))
        });
        let frequency_fit = fit(|| {
            points.iter().map(|p| {
                (
                    p.index,
                    frequency.at(p.index) + unwrapped(p) + p.residual,
                    p.weight,
                )
            })
        });
        (
            timing_fit.map_or(timing, |f| f.settled(n, self.clock_reach())),
            frequency_fit.map_or(frequency, |f| f.settled(0.0, f64::INFINITY)),
        )
    }

    fn settle(&mut self, preamble: usize, timing: Line, frequency: Line) -> usize {
        let n = self.params.chips() as f64;
        let count = preamble as f64;
        self.timing.reset(timing.slope - n);
        self.tracker.reset(frequency.slope);
        self.offset_bins = frequency.at(count);
        self.anchor = Some(timing.at(count));
        timing.intercept.round().max(0.0) as usize
    }
}

fn inlier_line(points: &[Point], seed: Line, n: f64, reach: f64) -> Line {
    let unwrapped = || {
        points.iter().filter_map(|p| {
            let residual = wrap(p.value - seed.at(p.index), n);
            (residual.abs() <= INLIER_BINS).then_some((
                p.index,
                seed.at(p.index) + residual,
                p.weight,
            ))
        })
    };
    let line = fit(unwrapped).map_or(seed, |f| f.bounded(0.0, reach));
    Line {
        intercept: wrap(line.intercept, n),
        slope: line.slope,
    }
}

fn circular_line(points: &[Point], reach: f64) -> Line {
    let count = points.len().max(1) as f64;
    let step = 1.0 / (4.0 * count);
    let reach = (reach / step).ceil() as i32;
    let mut best = (
        f64::NEG_INFINITY,
        Line {
            intercept: 0.0,
            slope: 0.0,
        },
    );
    for i in -reach..=reach {
        let slope = f64::from(i) * step;
        let level: Complex<f64> = points
            .iter()
            .map(|p| Complex::from_polar(p.weight, TAU * (p.value - slope * p.index)))
            .sum();
        if level.norm() > best.0 {
            best = (
                level.norm(),
                Line {
                    intercept: level.arg() / TAU,
                    slope,
                },
            );
        }
    }
    best.1
}

#[derive(Clone, Copy, Debug)]
struct Fit {
    line: Line,
    slope_sigma: f64,
    mean_x: f64,
    mean_y: f64,
}

impl Fit {
    fn settled(self, expected_slope: f64, reach: f64) -> Line {
        if (self.line.slope - expected_slope).abs() > SIGNIFICANT_SIGMAS * self.slope_sigma {
            return self.bounded(expected_slope, reach);
        }
        self.through_mean(expected_slope)
    }

    fn bounded(self, expected_slope: f64, reach: f64) -> Line {
        let slope = self
            .line
            .slope
            .clamp(expected_slope - reach, expected_slope + reach);
        self.through_mean(slope)
    }

    fn through_mean(self, slope: f64) -> Line {
        Line {
            intercept: self.mean_y - slope * self.mean_x,
            slope,
        }
    }
}

fn fit<I: Iterator<Item = (f64, f64, f64)>>(points: impl Fn() -> I) -> Option<Fit> {
    let (mut count, mut sw, mut sx, mut sy) = (0.0, 0.0, 0.0, 0.0);
    for (x, y, w) in points() {
        count += 1.0;
        sw += w;
        sx += w * x;
        sy += w * y;
    }
    if sw <= 0.0 || count < MIN_FIT_POINTS {
        return None;
    }
    let (mean_x, mean_y) = (sx / sw, sy / sw);
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for (x, y, w) in points() {
        sxx += w * (x - mean_x) * (x - mean_x);
        sxy += w * (x - mean_x) * (y - mean_y);
    }
    if sxx <= 0.0 {
        return None;
    }
    let slope = sxy / sxx;
    let line = Line {
        intercept: mean_y - slope * mean_x,
        slope,
    };
    let residual: f64 = points()
        .map(|(x, y, w)| w * (y - line.at(x)).powi(2))
        .sum::<f64>()
        / sw;
    let slope_sigma = (residual * count / (count - 2.0) / (sxx * count / sw)).sqrt();
    Some(Fit {
        line,
        slope_sigma,
        mean_x,
        mean_y,
    })
}

fn centred(index: usize, n: f64) -> f64 {
    wrap(index as f64, n)
}

/// The strongest local maxima of `values`, strongest first; unused slots hold no strength.
fn strongest_peaks(values: &[Complex<f64>]) -> [(f64, usize); LEAD_CANDIDATES] {
    let len = values.len();
    let mut best = [(f64::NEG_INFINITY, 0usize); LEAD_CANDIDATES];
    for (k, v) in values.iter().enumerate() {
        let strength = v.norm_sqr();
        let left = values[(k + len - 1) % len].norm_sqr();
        let right = values[(k + 1) % len].norm_sqr();
        if strength < left || strength < right {
            continue;
        }
        if let Some(slot) = best.iter().position(|&(s, _)| strength > s) {
            best[slot..].rotate_right(1);
            best[slot] = (strength, k);
        }
    }
    best
}

fn argmax_norm(values: &[Complex<f64>]) -> usize {
    let mut best = 0usize;
    for (k, v) in values.iter().enumerate() {
        if v.norm_sqr() > values[best].norm_sqr() {
            best = k;
        }
    }
    best
}
