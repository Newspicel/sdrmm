//! Split-phase (Manchester) bits on a residual-carrier phase-modulated downlink, as the L-band
//! weather satellites send HRPT: a loop holds the carrier, the quadrature arm carries the
//! chips, and each bit is the difference of its two half-bit sums. The bit clock follows the
//! peak of that difference with an early-late gate, which also holds it through long runs of
//! one bit value, where a decision-directed detector has nothing to go on.

mod carrier;
mod modulator;
#[cfg(test)]
mod tests;

use num_complex::Complex;
use sdrmm_dsp::SymbolSync;

pub use carrier::ResidualCarrier;
pub use modulator::modulate;

const TIMING_BW: f64 = 0.0015;
const LEVEL_SMOOTHING: f32 = 1.0 / 1024.0;
const MAX_HALF_BIT: usize = 64;

/// The two half-bit sums that end at the newest sample.
struct HalfBits {
    history: [f32; 2 * MAX_HALF_BIT],
    half: usize,
    at: usize,
}

impl HalfBits {
    fn new(half: usize) -> Self {
        Self {
            history: [0.0; 2 * MAX_HALF_BIT],
            half: half.clamp(1, MAX_HALF_BIT),
            at: 0,
        }
    }

    fn reset(&mut self) {
        self.history.fill(0.0);
        self.at = 0;
    }

    /// The first half minus the second half of the bit that ends with `sample`.
    fn push(&mut self, sample: f32) -> f32 {
        let span = 2 * self.half;
        self.history[self.at] = sample;
        self.at = (self.at + 1) % span;
        let (mut first, mut second) = (0.0, 0.0);
        for k in 0..self.half {
            first += self.history[(self.at + k) % span];
            second += self.history[(self.at + self.half + k) % span];
        }
        first - second
    }
}

/// Demodulates residual-carrier PM with split-phase bits into soft bits near ±1.
pub struct SplitPhaseDemod {
    carrier: ResidualCarrier,
    halves: HalfBits,
    sync: SymbolSync,
    /// Each bit sum one sample late, beside how much stronger it was a sample before than a
    /// sample after.
    matched: Vec<Complex<f32>>,
    recent: [f32; 2],
    level: f32,
}

impl SplitPhaseDemod {
    /// `sample_rate` must give at least four samples per bit.
    #[must_use]
    pub fn new(sample_rate: f64, bit_rate: f64, carrier_bw_hz: f64) -> Self {
        let sps = sample_rate / bit_rate;
        Self {
            carrier: ResidualCarrier::new(sample_rate, carrier_bw_hz),
            halves: HalfBits::new((sps / 2.0).round() as usize),
            sync: SymbolSync::new(sps, TIMING_BW),
            matched: Vec::new(),
            recent: [0.0; 2],
            level: 0.0,
        }
    }

    pub fn reset(&mut self) {
        self.carrier.reset();
        self.halves.reset();
        self.sync.reset();
        self.matched.clear();
        self.recent = [0.0; 2];
        self.level = 0.0;
    }

    #[must_use]
    pub fn carrier(&self) -> &ResidualCarrier {
        &self.carrier
    }

    /// Appends one soft bit per recovered bit to `soft`.
    pub fn process(&mut self, iq: &[Complex<f32>], soft: &mut Vec<f32>) {
        self.matched.clear();
        for &sample in iq {
            let sum = self.halves.push(self.carrier.derotate(sample).im);
            let [early, centre] = self.recent;
            self.matched
                .push(Complex::new(centre, early.abs() - sum.abs()));
            self.recent = [centre, sum];
        }
        let level = &mut self.level;
        self.sync.process_steered(&self.matched, |strobe| {
            *level += (strobe.re.abs() - *level) * LEVEL_SMOOTHING;
            let scale = level.max(1e-12).recip();
            soft.push(strobe.re * scale);
            Some(f64::from(strobe.im * scale))
        });
    }
}
