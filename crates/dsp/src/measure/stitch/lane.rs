use std::f64::consts::{PI, TAU};

use num_complex::Complex;

use super::{
    STITCH_FFT, STITCH_KEEP,
    floor::{Floor, POWER_FLOOR_DB},
};
use crate::math::complex::narrow;

/// Width of the raised-cosine crossfade at each lane edge, as a fraction of the lane rate.
pub(super) const RAMP: f64 = 0.05;

/// One lane bin's route to an output bin and the weights it is summed with.
#[derive(Clone, Copy)]
pub(super) struct Tap {
    pub(super) lane_bin: usize,
    pub(super) out_bin: usize,
    pub(super) mask: f32,
    pub(super) gain: f32,
    pub(super) quality: f32,
    pub(super) share: f32,
    pub(super) weight: f32,
}

pub(super) struct Lane {
    pub(super) offset_hz: f64,
    pub(super) shift_bins: i64,
    pub(super) out_shift: i64,
    pub(super) residual: Complex<f64>,
    pub(super) phasor: Complex<f64>,
    pub(super) dc: Complex<f32>,
    pub(super) frame: Vec<Complex<f32>>,
    pub(super) spectrum: Vec<Complex<f32>>,
    pub(super) taps: Vec<Tap>,
    pub(super) correction: Complex<f32>,
    pub(super) floor: Floor,
    pub(super) noise_db: f32,
    pub(super) gain_db: f32,
    pub(super) spur_bins: u32,
}

impl Tap {
    pub(super) fn new(lane_bin: usize, out_bin: usize, mask: f32) -> Self {
        Self {
            lane_bin,
            out_bin,
            mask,
            gain: 1.0,
            quality: 1.0,
            share: mask,
            weight: 0.0,
        }
    }
}

impl Lane {
    pub(super) fn new() -> Self {
        Self {
            offset_hz: f64::NAN,
            shift_bins: 0,
            out_shift: 0,
            residual: Complex::new(1.0, 0.0),
            phasor: Complex::new(1.0, 0.0),
            dc: Complex::default(),
            frame: vec![Complex::default(); STITCH_FFT],
            spectrum: vec![Complex::default(); STITCH_FFT],
            taps: Vec::with_capacity(STITCH_FFT),
            correction: Complex::new(1.0, 0.0),
            floor: Floor::new(),
            noise_db: POWER_FLOOR_DB,
            gain_db: 0.0,
            spur_bins: 0,
        }
    }

    /// Removes DC and the sub-bin part of the lane offset while copying `input` into the frame.
    pub(super) fn condition(&mut self, input: &[Complex<f32>], at: usize, dc_alpha: f32) {
        for (slot, sample) in self.frame[at..].iter_mut().zip(input) {
            self.dc += (*sample - self.dc) * dc_alpha;
            let clean = *sample - self.dc;
            *slot = clean * narrow(self.phasor);
            self.phasor *= self.residual;
        }
        self.phasor /= self.phasor.norm();
    }

    /// The phase a whole-bin shift has turned through by `frame_start`.
    pub(super) fn rotation(&self, frame_start: u64) -> Complex<f32> {
        let len = STITCH_FFT as i64;
        let start = (frame_start % STITCH_FFT as u64) as i64;
        let turns = (self.out_shift.rem_euclid(len) * start).rem_euclid(len);
        let phase = TAU * turns as f64 / STITCH_FFT as f64;
        Complex::from_polar(1.0, phase as f32)
    }

    pub(super) fn flag_spur(&mut self, bin: usize) {
        if !self.floor.spur[bin] {
            self.floor.spur[bin] = true;
            self.spur_bins += 1;
        }
    }

    pub(super) fn forget_estimates(&mut self) {
        self.gain_db = 0.0;
        self.noise_db = POWER_FLOOR_DB;
        self.spur_bins = 0;
        self.floor.spur.fill(false);
    }
}

/// The lane's crossfade weight at a signed bin: flat across the kept band, a raised cosine
/// down to zero over [`RAMP`].
pub(super) fn lane_mask(bin: i64) -> f32 {
    let f = (bin as f64 / STITCH_FFT as f64).abs();
    let flat = STITCH_KEEP / 2.0 - RAMP / 2.0;
    if f <= flat {
        1.0
    } else if f >= flat + RAMP {
        0.0
    } else {
        f64::midpoint(1.0, (PI * (f - flat) / RAMP).cos()) as f32
    }
}

pub(super) fn signed_bin(index: usize, len: usize) -> i64 {
    if index < len / 2 {
        index as i64
    } else {
        index as i64 - len as i64
    }
}
