use std::f32::consts::{PI, TAU};

use num_complex::Complex;
use sdrmm_dsp::fast_atan2;

/// A decision-directed carrier reference for burst PSK whose symbols need not be evenly spaced:
/// phase in radians and frequency in radians per sample, corrected by a second-order loop after
/// each decided symbol.
///
/// Detecting against this reference instead of the previous symbol halves the noise on the phase
/// decision, which is what coherent detection of a differentially encoded stream earns.
#[derive(Clone, Copy, Debug)]
pub struct PhaseTracker {
    phase: f32,
    freq: f32,
    alpha: f32,
    beta: f32,
}

impl PhaseTracker {
    /// `alpha` is the share of each phase error taken into the phase, `beta` the share taken into
    /// the frequency over one symbol.
    #[must_use]
    pub fn new(alpha: f32, beta: f32) -> Self {
        Self {
            phase: 0.0,
            freq: 0.0,
            alpha,
            beta,
        }
    }

    pub fn start(&mut self, phase: f32, freq: f32) {
        self.phase = wrap(phase);
        self.freq = freq;
    }

    #[must_use]
    pub fn phase(&self) -> f32 {
        self.phase
    }

    #[must_use]
    pub fn freq(&self) -> f32 {
        self.freq
    }

    /// The unit phasor that takes the carrier off a sample `offset` samples on.
    #[must_use]
    pub fn derotator(&self, offset: f32) -> Complex<f32> {
        Complex::from_polar(1.0, -(self.phase + self.freq * offset))
    }

    pub fn advance(&mut self, samples: f32) {
        self.phase = wrap(self.phase + self.freq * samples);
    }

    /// Steers by the angle between a derotated symbol and the point decided for it, over a
    /// symbol of `span` samples.
    pub fn steer(&mut self, derotated: Complex<f32>, decided: Complex<f32>, span: f32) {
        let residual = derotated * decided.conj();
        if residual.norm_sqr() == 0.0 {
            return;
        }
        let error = fast_atan2(residual.im, residual.re);
        self.phase = wrap(self.phase + self.alpha * error);
        self.freq += self.beta * error / span;
    }
}

fn wrap(phase: f32) -> f32 {
    if phase > PI || phase <= -PI {
        phase - TAU * ((phase + PI) / TAU).floor()
    } else {
        phase
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frequency_offset_is_pulled_in_and_held() {
        let mut tracker = PhaseTracker::new(0.2, 0.02);
        tracker.start(0.3, 0.0);
        let freq = 0.01f32;
        let span = 20.0;
        let mut worst_late = 0.0f32;
        for symbol in 0..400 {
            let truth = 0.3 + freq * span * (symbol + 1) as f32;
            tracker.advance(span);
            let received = Complex::from_polar(1.0, truth);
            let derotated = received * tracker.derotator(0.0);
            tracker.steer(derotated, Complex::new(1.0, 0.0), span);
            if symbol > 300 {
                worst_late = worst_late.max(derotated.arg().abs());
            }
        }
        assert!((tracker.freq() - freq).abs() < 1e-4, "{}", tracker.freq());
        assert!(worst_late < 0.01, "{worst_late}");
    }

    #[test]
    fn the_phase_stays_wrapped() {
        let mut tracker = PhaseTracker::new(0.1, 0.0);
        tracker.start(3.0, 0.5);
        for _ in 0..1_000 {
            tracker.advance(7.0);
            assert!(tracker.phase() > -PI && tracker.phase() <= PI);
        }
    }
}
