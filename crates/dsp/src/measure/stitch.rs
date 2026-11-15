mod floor;
mod lane;
mod layout;
mod matching;
mod sorted_window;
mod weights;

use std::f64::consts::TAU;

use num_complex::Complex;

use crate::{math::db::Decibels, math::fft::FftPair};
use lane::Lane;
use matching::{Match, Overlap};

pub const STITCH_FFT: usize = 2048;
pub const STITCH_KEEP: f64 = 0.85;

const MIN_LANES: usize = 2;
const MAX_LANES: usize = 16;
const DC_CORNER: f64 = 1e-5;
const HOP: usize = STITCH_FFT / 2;
const MAX_SHIFT_BINS: f64 = 1e12;

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum StitchError {
    #[error("a stitch takes {MIN_LANES} to {MAX_LANES} lanes, not {0}")]
    Lanes(usize),
    #[error("the input rate must be positive and finite, got {0} Hz")]
    Rate(f64),
    #[error("expected {expected} lanes, got {got}")]
    LaneCount { expected: usize, got: usize },
    #[error("lane {lane} holds {got} samples, lane 0 holds {expected}")]
    LaneLength {
        lane: usize,
        expected: usize,
        got: usize,
    },
    #[error("lane offset {0} Hz is out of range")]
    Offset(f64),
    #[error("lanes {0} and {1} do not overlap")]
    NoOverlap(usize, usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StitchOptions {
    pub equalise: bool,
    pub flatten: bool,
    pub snr_blend: bool,
    pub spur_reject: bool,
    pub match_phase: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LaneState {
    pub gain_db: f32,
    pub noise_db: f32,
    pub phase_deg: f32,
    pub coherence: Option<f32>,
    pub spur_bins: u32,
}

pub struct Stitcher {
    input_rate: f64,
    out_len: usize,
    dc_alpha: f32,
    options: StitchOptions,
    lanes: Vec<Lane>,
    order: Vec<usize>,
    matches: Vec<Match>,
    previous: Vec<Match>,
    overlaps: Vec<Overlap>,
    gap: Option<(usize, usize)>,
    center_bins: i64,
    sums: Vec<f32>,
    blend_sums: Vec<f32>,
    ordered: Vec<f32>,
    scratch: Vec<f32>,
    output: Vec<Complex<f32>>,
    lane_fft: FftPair,
    out_fft: FftPair,
    fill: usize,
    frame_start: u64,
    tracked: u64,
    ready: bool,
    reference_db: f32,
}

impl StitchOptions {
    const fn tracks_floor(self) -> bool {
        self.equalise || self.snr_blend || self.spur_reject
    }
}

impl Default for StitchOptions {
    fn default() -> Self {
        Self {
            equalise: true,
            flatten: true,
            snr_blend: true,
            spur_reject: true,
            match_phase: true,
        }
    }
}

#[must_use]
pub fn auto_offsets(lanes: usize, input_rate: f64) -> Vec<f64> {
    let bin_hz = input_rate / STITCH_FFT as f64;
    let step = step_bins(lanes);
    (0..lanes)
        .map(|lane| {
            let twice = 2 * lane as i64 - (lanes as i64 - 1);
            (twice * step) as f64 / 2.0 * bin_hz
        })
        .collect()
}

#[must_use]
pub fn output_rate(lanes: usize, input_rate: f64) -> f64 {
    lanes as f64 * input_rate
}

impl Stitcher {
    pub fn new(lanes: usize, input_rate: f64) -> Result<Self, StitchError> {
        if !(MIN_LANES..=MAX_LANES).contains(&lanes) {
            return Err(StitchError::Lanes(lanes));
        }
        if !input_rate.is_finite() || input_rate <= 0.0 {
            return Err(StitchError::Rate(input_rate));
        }
        let out_len = STITCH_FFT * lanes;
        let mut stitcher = Self {
            input_rate,
            out_len,
            dc_alpha: (1.0 - (-TAU * DC_CORNER).exp()) as f32,
            options: StitchOptions::default(),
            lanes: (0..lanes).map(|_| Lane::new()).collect(),
            order: (0..lanes).collect(),
            matches: Vec::with_capacity(lanes),
            previous: Vec::with_capacity(lanes),
            overlaps: Vec::with_capacity((lanes - 1) * STITCH_FFT),
            gap: None,
            center_bins: 0,
            sums: vec![0.0; out_len],
            blend_sums: vec![0.0; out_len],
            ordered: vec![0.0; STITCH_FFT],
            scratch: Vec::with_capacity(STITCH_FFT),
            output: vec![Complex::default(); out_len],
            lane_fft: FftPair::new(STITCH_FFT),
            out_fft: FftPair::new(out_len),
            fill: 0,
            frame_start: 0,
            tracked: 0,
            ready: false,
            reference_db: 0.0,
        };
        stitcher.retune(&auto_offsets(lanes, input_rate))?;
        Ok(stitcher)
    }

    #[must_use]
    pub fn output_rate(&self) -> f64 {
        output_rate(self.lanes.len(), self.input_rate)
    }

    #[must_use]
    pub fn output_center_offset_hz(&self) -> f64 {
        self.center_bins as f64 * self.bin_hz()
    }

    #[must_use]
    pub fn lane_state(&self, lane: usize) -> LaneState {
        let Some(state) = self.lanes.get(lane) else {
            return LaneState::default();
        };
        let magnitude = state.correction.norm();
        let matched_db = if magnitude > f32::MIN_POSITIVE {
            magnitude.amplitude_to_db()
        } else {
            0.0
        };
        LaneState {
            gain_db: state.gain_db + matched_db,
            noise_db: state.noise_db,
            phase_deg: state.correction.arg().to_degrees(),
            coherence: self
                .matches
                .iter()
                .find(|pair| pair.high == lane)
                .and_then(|pair| pair.coherence),
            spur_bins: state.spur_bins,
        }
    }

    pub fn set_options(&mut self, options: StitchOptions) {
        if options.tracks_floor() && !self.options.tracks_floor() {
            self.restart_floors();
        }
        if !options.tracks_floor() {
            self.ready = false;
        }
        self.options = options;
        self.update_weights();
    }

    pub fn retune(&mut self, offsets_hz: &[f64]) -> Result<(), StitchError> {
        if offsets_hz.len() != self.lanes.len() {
            return Err(StitchError::LaneCount {
                expected: self.lanes.len(),
                got: offsets_hz.len(),
            });
        }
        let bin_hz = self.bin_hz();
        if let Some(offset) = offsets_hz
            .iter()
            .find(|offset| !offset.is_finite() || (*offset / bin_hz).abs() > MAX_SHIFT_BINS)
        {
            return Err(StitchError::Offset(*offset));
        }
        let mut changed = [false; MAX_LANES];
        for (index, (lane, offset)) in self.lanes.iter_mut().zip(offsets_hz).enumerate() {
            if lane.offset_hz.to_bits() == offset.to_bits() {
                continue;
            }
            changed[index] = true;
            lane.offset_hz = *offset;
            lane.shift_bins = (offset / bin_hz).round() as i64;
            let residual = offset - lane.shift_bins as f64 * bin_hz;
            lane.residual = Complex::from_polar(1.0, TAU * residual / self.input_rate);
        }
        if changed.iter().any(|flag| *flag) {
            self.centre_lanes();
            self.place_taps();
            self.pair_lanes(&changed);
            self.restart_floors();
            self.update_weights();
        }
        self.gap
            .map_or(Ok(()), |(low, high)| Err(StitchError::NoOverlap(low, high)))
    }

    pub fn reset(&mut self) {
        self.fill = 0;
        self.frame_start = 0;
        for lane in &mut self.lanes {
            lane.phasor = Complex::new(1.0, 0.0);
            lane.dc = Complex::default();
        }
        self.restart_floors();
        self.update_weights();
    }

    pub fn process(
        &mut self,
        lanes: &[&[Complex<f32>]],
        out: &mut Vec<Complex<f32>>,
    ) -> Result<usize, StitchError> {
        let len = self.check_input(lanes)?;
        let before = out.len();
        let mut at = 0;
        while at < len {
            let take = (STITCH_FFT - self.fill).min(len - at);
            for (lane, input) in self.lanes.iter_mut().zip(lanes) {
                lane.condition(&input[at..at + take], self.fill, self.dc_alpha);
            }
            self.fill += take;
            at += take;
            if self.fill == STITCH_FFT {
                self.run_frame(out);
            }
        }
        Ok(out.len() - before)
    }

    fn check_input(&self, lanes: &[&[Complex<f32>]]) -> Result<usize, StitchError> {
        if lanes.len() != self.lanes.len() {
            return Err(StitchError::LaneCount {
                expected: self.lanes.len(),
                got: lanes.len(),
            });
        }
        let len = lanes.first().map_or(0, |lane| lane.len());
        if let Some((lane, input)) = lanes
            .iter()
            .enumerate()
            .find(|(_, input)| input.len() != len)
        {
            return Err(StitchError::LaneLength {
                lane,
                expected: len,
                got: input.len(),
            });
        }
        Ok(len)
    }

    fn bin_hz(&self) -> f64 {
        self.input_rate / STITCH_FFT as f64
    }

    fn run_frame(&mut self, out: &mut Vec<Complex<f32>>) {
        for lane in &mut self.lanes {
            lane.spectrum.copy_from_slice(&lane.frame);
            self.lane_fft.forward(&mut lane.spectrum);
            lane.frame.copy_within(HOP.., 0);
        }
        self.fill = STITCH_FFT - HOP;
        self.match_lanes();
        if self.options.tracks_floor() {
            self.track_floors();
        }
        self.synthesise();
        let edge = self.out_len / 4;
        out.extend_from_slice(&self.output[edge..self.out_len - edge]);
        self.frame_start += HOP as u64;
    }

    fn synthesise(&mut self) {
        self.output.fill(Complex::default());
        for lane in &self.lanes {
            let factor = lane.rotation(self.frame_start) * lane.correction;
            for tap in &lane.taps {
                self.output[tap.out_bin] += lane.spectrum[tap.lane_bin] * factor * tap.weight;
            }
        }
        self.out_fft.inverse(&mut self.output);
    }
}

/// Bins between neighbouring lane centres, kept even for an even lane count so every centre,
/// half a step off zero, still lands on the bin grid.
fn step_bins(lanes: usize) -> i64 {
    let bins = (STITCH_KEEP * STITCH_FFT as f64).round() as i64;
    if lanes.is_multiple_of(2) && bins % 2 == 1 {
        bins - 1
    } else {
        bins
    }
}

#[cfg(test)]
mod tests;
