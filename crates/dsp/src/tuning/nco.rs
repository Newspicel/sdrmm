mod lanes;
mod table;

use std::fmt;

use num_complex::Complex;
pub use table::unit_phasor;
use table::{PHASE_SCALE, PHASORS, Table, phasor};

/// What an oscillator with an invalid frequency produces.
const SILENT: Complex<f32> = Complex::new(f32::NAN, f32::NAN);

#[derive(Clone)]
pub struct Nco {
    phase: u64,
    step: u64,
    valid: bool,
    lane: usize,
    anchor: Complex<f32>,
    steps: Box<lanes::Steps>,
    table: &'static Table,
}

impl fmt::Debug for Nco {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nco")
            .field("phase", &self.phase)
            .field("step", &self.step)
            .field("valid", &self.valid)
            .field("lane", &self.lane)
            .finish_non_exhaustive()
    }
}

impl Nco {
    #[must_use]
    pub fn new(freq_hz: f32, sample_rate: f32) -> Self {
        let mut nco = Self {
            phase: 0,
            step: 0,
            valid: true,
            lane: 0,
            anchor: Complex::new(1.0, 0.0),
            steps: Box::new(lanes::Steps::new(0)),
            table: &PHASORS,
        };
        nco.set_freq(freq_hz, sample_rate);
        nco
    }

    pub fn reset(&mut self) {
        self.phase = 0;
        self.lane = 0;
        self.realign();
    }

    pub fn set_freq(&mut self, freq_hz: f32, sample_rate: f32) {
        self.set_turns(f64::from(freq_hz) / f64::from(sample_rate));
    }

    /// Sets the frequency in cycles per sample, at the full precision of the phase accumulator.
    pub fn set_turns(&mut self, turns: f64) {
        let mut turns = turns.fract();
        self.valid = turns.is_finite();
        if turns >= 0.5 {
            turns -= 1.0;
        } else if turns < -0.5 {
            turns += 1.0;
        }
        let step = (turns * PHASE_SCALE).round() as i64 as u64;
        if step != self.step {
            self.step = step;
            *self.steps = lanes::Steps::new(step);
        }
        self.realign();
    }

    /// Restarts the oscillator at `turns` of a cycle.
    pub fn set_phase(&mut self, turns: f64) {
        self.phase = (turns.rem_euclid(1.0) * PHASE_SCALE) as u64;
        self.lane = 0;
        self.realign();
    }

    /// Moves on by `samples` without producing them, exactly as if they had been mixed.
    pub fn skip(&mut self, samples: usize) {
        self.phase = self
            .phase
            .wrapping_add(self.step.wrapping_mul(samples as u64));
        self.lane = (self.lane + samples) % lanes::GROUP;
        self.realign();
    }

    /// The phase of the next sample, in turns.
    #[must_use]
    pub fn turns(&self) -> f64 {
        self.phase as f64 / PHASE_SCALE
    }

    #[must_use]
    pub const fn is_identity(&self) -> bool {
        self.valid && self.step == 0 && self.phase == 0
    }

    #[must_use]
    pub fn next_sample(&mut self) -> Complex<f32> {
        if !self.valid {
            return SILENT;
        }
        self.wave()
    }

    #[inline(never)]
    pub fn mix_into(&mut self, input: &[Complex<f32>], out: &mut [Complex<f32>]) {
        debug_assert_eq!(input.len(), out.len());
        let len = input.len().min(out.len());
        let (input, out) = (&input[..len], &mut out[..len]);
        if !self.valid {
            out.fill(SILENT);
            return;
        }
        lanes::mix_into(self, input, out);
    }

    /// Mixes a real signal: each sample scales the oscillator.
    pub fn mix_real_into(&mut self, input: &[f32], out: &mut [Complex<f32>]) {
        debug_assert_eq!(input.len(), out.len());
        let len = input.len().min(out.len());
        let (input, out) = (&input[..len], &mut out[..len]);
        if !self.valid {
            out.fill(SILENT);
            return;
        }
        lanes::mix_real_into(self, input, out);
    }

    pub fn mix(&mut self, samples: &mut [Complex<f32>]) {
        if !self.valid {
            samples.fill(SILENT);
            return;
        }
        lanes::mix(self, samples);
    }

    fn realign(&mut self) {
        let back = self.step.wrapping_mul(self.lane as u64);
        self.anchor = phasor(self.table, self.phase.wrapping_sub(back));
    }

    #[inline(always)]
    fn wave(&mut self) -> Complex<f32> {
        let sample = lanes::product(self.steps.at(self.lane), self.anchor);
        self.advance(1);
        sample
    }

    #[inline(always)]
    fn advance(&mut self, samples: usize) {
        self.phase = self
            .phase
            .wrapping_add(self.step.wrapping_mul(samples as u64));
        self.lane += samples;
        if self.lane == lanes::GROUP {
            self.lane = 0;
            self.anchor = phasor(self.table, self.phase);
        }
    }
}

#[cfg(test)]
mod tests;
