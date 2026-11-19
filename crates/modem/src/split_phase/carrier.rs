use std::f32::consts::{PI, TAU};
use std::f64::consts::FRAC_1_SQRT_2;

use num_complex::Complex;
use sdrmm_dsp::{LoopFilter, fast_arg};

/// The loop updates once per block of this many hertz worth of samples, which also bounds
/// its pull-in to half of it either side.
const BLOCK_RATE_HZ: f64 = 160_000.0;
const FREQ_LIMIT: f64 = 0.45;
const FLL_GAIN: f64 = 0.1;
const LOCK_SMOOTHING: f32 = 1.0 / 64.0;
const LOCK_ENTER: f32 = 0.8;
const LOCK_LEAVE: f32 = 0.5;

/// Holds the residual carrier of a phase-modulated downlink at zero phase. The split-phase
/// data has no power near the carrier, so each block's mean is the carrier alone: its angle
/// is the phase error, and the turn between blocks pulls the loop in from far off.
pub struct ResidualCarrier {
    filter: LoopFilter,
    block: usize,
    filled: usize,
    sum: Complex<f32>,
    previous: Complex<f32>,
    phase: f32,
    step: f32,
    lock: f32,
    locked: bool,
    sample_rate: f64,
}

impl ResidualCarrier {
    #[must_use]
    pub fn new(sample_rate: f64, loop_bw_hz: f64) -> Self {
        let block = (sample_rate / BLOCK_RATE_HZ).round().max(1.0) as usize;
        let block_rate = sample_rate / block as f64;
        Self {
            filter: LoopFilter::new(loop_bw_hz / block_rate, FRAC_1_SQRT_2, FREQ_LIMIT),
            block,
            filled: 0,
            sum: Complex::default(),
            previous: Complex::default(),
            phase: 0.0,
            step: 0.0,
            lock: 0.0,
            locked: false,
            sample_rate,
        }
    }

    pub fn reset(&mut self) {
        self.filter.reset(0.0);
        self.filled = 0;
        self.sum = Complex::default();
        self.previous = Complex::default();
        self.phase = 0.0;
        self.step = 0.0;
        self.lock = 0.0;
        self.locked = false;
    }

    #[must_use]
    pub fn locked(&self) -> bool {
        self.locked
    }

    /// The carrier's offset from the channel centre.
    #[must_use]
    pub fn offset_hz(&self) -> f64 {
        self.filter.freq_norm() * self.sample_rate / self.block as f64
    }

    /// `sample` turned so the carrier lies on the positive real axis.
    pub fn derotate(&mut self, sample: Complex<f32>) -> Complex<f32> {
        let (sin, cos) = self.phase.sin_cos();
        let turned = sample * Complex::new(cos, -sin);
        self.phase += self.step;
        if self.phase >= PI {
            self.phase -= TAU;
        } else if self.phase < -PI {
            self.phase += TAU;
        }
        self.sum += turned;
        self.filled += 1;
        if self.filled == self.block {
            self.update();
        }
        turned
    }

    fn update(&mut self) {
        let mean = self.sum;
        self.sum = Complex::default();
        self.filled = 0;
        if !mean.is_finite() || mean.norm_sqr() <= 0.0 {
            return;
        }
        let error = fast_arg(mean);
        self.track_lock(error);
        if !self.locked && self.previous.norm_sqr() > 0.0 {
            let turn = f64::from(fast_arg(mean * self.previous.conj()));
            let pulled = self.filter.freq_norm() + FLL_GAIN * turn / std::f64::consts::TAU;
            self.filter.reset(pulled);
        }
        self.previous = mean;
        let per_block = self.filter.advance(f64::from(error));
        self.step = (per_block / self.block as f64) as f32;
    }

    fn track_lock(&mut self, error: f32) {
        self.lock += (error.cos() - self.lock) * LOCK_SMOOTHING;
        self.locked = if self.locked {
            self.lock > LOCK_LEAVE
        } else {
            self.lock > LOCK_ENTER
        };
    }
}
