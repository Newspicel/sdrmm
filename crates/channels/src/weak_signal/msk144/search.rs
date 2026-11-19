use std::f64::consts::TAU;

use num_complex::Complex;
use sdrmm_dsp::math::fft::Transform;

use super::frame::{FRAME, SECOND_SYNC, SYNC_SAMPLES, sync_template};
use super::{RATE, SYNCED};

pub(super) const PEAKS: usize = 2;
const PEAK_GUARD: usize = 7;
const STEP: usize = 216;
const EDGE: usize = 12;
const MAX_PINGS: usize = 5;
const PING_LEVEL: f32 = 3.0;
const PING_CONTRAST: f32 = 12.0;
pub(super) const PING_TOLERANCE_HZ: f32 = 8.0;
pub(super) const PING_STEP_HZ: f32 = 2.0;
const PING_FRAMES: usize = 3;
/// Which of three frames around a ping to average, best guesses first.
pub(super) const PING_MASKS: [[bool; PING_FRAMES]; 6] = [
    [false, true, false],
    [true, false, false],
    [false, false, true],
    [true, true, false],
    [false, true, true],
    [true, true, true],
];

/// Frames averaged at the strongest sync, with the sync offsets worth trying.
pub(super) struct Synced {
    pub(super) frequency_hz: f32,
    pub(super) frame: [Complex<f32>; FRAME],
    pub(super) peaks: [usize; PEAKS],
}

/// Where a ping seems to be and how far off the expected carrier it sits.
#[derive(Clone, Copy)]
pub(super) struct Ping {
    pub(super) start: usize,
    pub(super) offset_hz: f32,
}

pub(super) struct Search {
    template: [Complex<f32>; SYNC_SAMPLES],
    averaged: [Complex<f32>; FRAME],
    best: [Complex<f32>; FRAME],
    scores: [f32; FRAME],
    best_scores: [f32; FRAME],
    spectrum: Transform,
    squared: Vec<Complex<f32>>,
    edge: [f32; EDGE],
}

impl Search {
    pub(super) fn new() -> Self {
        Self {
            template: sync_template(),
            averaged: [Complex::default(); FRAME],
            best: [Complex::default(); FRAME],
            scores: [0.0; FRAME],
            best_scores: [0.0; FRAME],
            spectrum: Transform::forward(FRAME),
            squared: vec![Complex::default(); FRAME],
            edge: std::array::from_fn(|index| {
                (1.0 - (index as f32 * std::f32::consts::PI / EDGE as f32).cos()) / 2.0
            }),
        }
    }

    /// Averages the frames `mask` picks out of `data` at each trial carrier within `tolerance_hz`
    /// of `center_hz`, keeping the carrier whose averaged frame correlates best with the sync.
    pub(super) fn sync(
        &mut self,
        data: &[Complex<f32>],
        mask: &[bool],
        center_hz: f32,
        tolerance_hz: f32,
        step_hz: f32,
    ) -> Option<Synced> {
        let averaged = mask.iter().filter(|&&used| used).count();
        let scale = 1.0 / (48.0 * (averaged as f32).sqrt());
        let steps = (tolerance_hz / step_hz).round() as i32;
        let mut best = (0.0f32, 0.0f32);
        for trial in -steps..=steps {
            let offset_hz = trial as f32 * step_hz;
            self.average(data, mask, center_hz + offset_hz);
            let peak = self.correlate() * scale;
            if peak > best.0 {
                best = (peak, offset_hz);
                self.best = self.averaged;
                self.best_scores = self.scores;
            }
        }
        if best.0 < SYNCED {
            return None;
        }
        Some(Synced {
            frequency_hz: center_hz + best.1,
            frame: self.best,
            peaks: self.peaks(),
        })
    }

    fn average(&mut self, data: &[Complex<f32>], mask: &[bool], carrier_hz: f32) {
        self.averaged.fill(Complex::default());
        let step = -TAU * f64::from(carrier_hz) / RATE;
        for (frame, _) in mask.iter().enumerate().filter(|&(_, &used)| used) {
            let start = frame * FRAME;
            let Some(samples) = data.get(start..start + FRAME) else {
                continue;
            };
            for (index, (sum, &sample)) in self.averaged.iter_mut().zip(samples).enumerate() {
                let phase = step * (start + index + 1) as f64;
                let turn = Complex::new(phase.cos() as f32, phase.sin() as f32);
                *sum += sample * turn;
            }
        }
    }

    fn correlate(&mut self) -> f32 {
        let mut peak = 0.0f32;
        for (shift, score) in self.scores.iter_mut().enumerate() {
            let sum: Complex<f32> = self
                .template
                .iter()
                .enumerate()
                .map(|(index, sync)| {
                    let first = self.averaged[(shift + index) % FRAME];
                    let second = self.averaged[(shift + SECOND_SYNC + index) % FRAME];
                    (first + second) * sync.conj()
                })
                .sum();
            *score = sum.norm();
            peak = peak.max(*score);
        }
        peak
    }

    fn peaks(&mut self) -> [usize; PEAKS] {
        std::array::from_fn(|_| {
            let (at, _) =
                self.best_scores
                    .iter()
                    .enumerate()
                    .fold((0, f32::MIN), |best, (index, &score)| {
                        if score > best.1 { (index, score) } else { best }
                    });
            let low = at.saturating_sub(PEAK_GUARD);
            let high = (at + PEAK_GUARD).min(FRAME - 1);
            self.best_scores[low..=high].fill(0.0);
            at
        })
    }

    /// Finds short pings by the two lines squaring an MSK signal leaves at twice each tone.
    pub(super) fn pings(
        &mut self,
        data: &[Complex<f32>],
        center_hz: f32,
        tolerance_hz: f32,
    ) -> Vec<Ping> {
        let steps = (data.len().saturating_sub(FRAME)) / STEP;
        let mut level = Vec::with_capacity(steps);
        let mut contrast = Vec::with_capacity(steps);
        let mut offset = Vec::with_capacity(steps);
        for step in 0..steps {
            let (strength, ratio, offset_hz) = self.tone_pair(
                &data[step * STEP..step * STEP + FRAME],
                center_hz,
                tolerance_hz,
            );
            level.push(strength);
            contrast.push(ratio);
            offset.push(offset_hz);
        }
        let mut sorted = level.clone();
        sorted.sort_by(f32::total_cmp);
        let Some(&floor) = sorted.get((steps / 4).saturating_sub(1)) else {
            return Vec::new();
        };
        if floor <= 0.0 {
            return Vec::new();
        }
        for value in &mut level {
            *value /= floor;
        }
        let mut pings = pick(&mut level, &offset, PING_LEVEL, tolerance_hz, MAX_PINGS);
        if pings.len() < 3 {
            let more = MAX_PINGS - pings.len();
            pings.extend(pick(
                &mut contrast,
                &offset,
                PING_CONTRAST,
                tolerance_hz,
                more,
            ));
        }
        pings
    }

    fn tone_pair(
        &mut self,
        frame: &[Complex<f32>],
        center_hz: f32,
        tolerance_hz: f32,
    ) -> (f32, f32, f32) {
        for (slot, &sample) in self.squared.iter_mut().zip(frame) {
            *slot = sample * sample;
        }
        for (index, &weight) in self.edge.iter().enumerate() {
            self.squared[index] *= weight;
            self.squared[FRAME - 1 - index] *= weight;
        }
        self.spectrum.process(&mut self.squared);
        let resolution = RATE as f32 / FRAME as f32;
        let high = self.line(2.0 * (center_hz + 500.0), tolerance_hz, resolution);
        let low = self.line(2.0 * (center_hz - 500.0), tolerance_hz, resolution);
        let offset_hz = if high.0 >= low.0 { high.2 } else { low.2 };
        (high.0.max(low.0), high.1.max(low.1), offset_hz)
    }

    /// The strongest bin within twice the tolerance of `line_hz`: its power, its contrast to
    /// the rest of the window, and how far the carrier it implies sits from where it should.
    fn line(&self, line_hz: f32, tolerance_hz: f32, resolution: f32) -> (f32, f32, f32) {
        let bin = |hz: f32| ((hz / resolution).round().max(1.0) as usize).min(FRAME - 2);
        let (low, high) = (
            bin(line_hz - 2.0 * tolerance_hz),
            bin(line_hz + 2.0 * tolerance_hz),
        );
        let power = |index: usize| self.squared[index].norm_sqr();
        let peak = (low..=high).fold(low, |best, index| {
            if power(index) > power(best) {
                index
            } else {
                best
            }
        });
        let (before, at, after) = (
            self.squared[peak - 1],
            self.squared[peak],
            self.squared[peak + 1],
        );
        let denominator = 2.0 * at - before - after;
        let delta = if denominator.norm_sqr() > 0.0 {
            -((before - after) / denominator).re
        } else {
            0.0
        };
        let strength = power(peak);
        let rest = ((low..=high).map(power).sum::<f32>() - strength) / (high - low + 1) as f32;
        let expected = (line_hz / resolution).round();
        (
            strength,
            strength / (rest + 0.01),
            (peak as f32 + delta - expected) * resolution / 2.0,
        )
    }
}

/// The three frames centred on a ping.
pub(super) fn ping_window(data: &[Complex<f32>], ping: Ping) -> Option<&[Complex<f32>]> {
    let span = PING_FRAMES * FRAME;
    let start = ping
        .start
        .saturating_sub(FRAME)
        .min(data.len().saturating_sub(span));
    data.get(start..start + span)
}

fn pick(
    metric: &mut [f32],
    offset: &[f32],
    threshold: f32,
    tolerance_hz: f32,
    limit: usize,
) -> Vec<Ping> {
    let mut pings = Vec::new();
    for _ in 0..limit {
        let Some((at, &value)) = metric.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)) else {
            break;
        };
        if value < threshold {
            break;
        }
        if offset[at].abs() <= tolerance_hz {
            pings.push(Ping {
                start: at * STEP + 1,
                offset_hz: offset[at],
            });
        }
        metric[at] = 0.0;
    }
    pings
}
