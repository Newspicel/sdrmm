use std::f64::consts::{PI, TAU};

use num_complex::Complex;

/// A notch on complex baseband: it removes one frequency and leaves its mirror alone, which a
/// real biquad cannot do.
///
/// One zero sits on the unit circle at the notch and one pole just inside it, so the width is set
/// by how close the pole sits and the response away from the notch stays flat.
#[derive(Clone, Debug)]
pub struct IqNotch {
    rate: f64,
    width_hz: f64,
    zero: Complex<f64>,
    pole: Complex<f64>,
    gain: f64,
    state: Complex<f64>,
}

impl IqNotch {
    #[must_use]
    pub fn new(rate: f64, offset_hz: f64, width_hz: f64) -> Self {
        assert!(rate > 0.0, "rate must be positive");
        let mut notch = Self {
            rate,
            width_hz,
            zero: Complex::new(1.0, 0.0),
            pole: Complex::new(0.0, 0.0),
            gain: 1.0,
            state: Complex::new(0.0, 0.0),
        };
        notch.retune(offset_hz);
        notch
    }

    pub fn retune(&mut self, offset_hz: f64) {
        let radius = (1.0 - PI * self.width_hz / self.rate).clamp(0.0, 1.0 - 1e-9);
        self.zero = Complex::from_polar(1.0, TAU * offset_hz / self.rate);
        self.pole = self.zero * radius;
        self.gain = (1.0 + radius) / 2.0;
    }

    pub fn reset(&mut self) {
        self.state = Complex::new(0.0, 0.0);
    }

    pub fn process(&mut self, samples: &mut [Complex<f32>]) {
        if !(self.state.re.is_finite() && self.state.im.is_finite()) {
            self.reset();
        }
        for sample in samples {
            let x = Complex::new(f64::from(sample.re), f64::from(sample.im));
            let w = x + self.pole * self.state;
            let y = (w - self.zero * self.state) * self.gain;
            self.state = w;
            *sample = Complex::new(y.re as f32, y.im as f32);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{complex_tone, rms_c};

    const RATE: f64 = 240_000.0;
    const SETTLE: usize = 24_000;

    fn through(notch: &mut IqNotch, freq_hz: f64) -> f32 {
        let mut tone = complex_tone(freq_hz / RATE, 96_000);
        notch.process(&mut tone);
        rms_c(&tone[SETTLE..])
    }

    #[test]
    fn removes_its_frequency() {
        let mut notch = IqNotch::new(RATE, 30_000.0, 1_000.0);
        assert!(through(&mut notch, 30_000.0) < 0.01);
    }

    #[test]
    fn leaves_the_mirror_and_the_neighbours_alone() {
        for freq_hz in [-30_000.0, 25_000.0, 35_000.0, 0.0] {
            let mut notch = IqNotch::new(RATE, 30_000.0, 1_000.0);
            let kept = through(&mut notch, freq_hz);
            assert!(kept > 0.95, "{freq_hz} Hz kept only {kept}");
        }
    }

    #[test]
    fn half_its_width_away_it_passes_about_half_the_power() {
        let mut notch = IqNotch::new(RATE, 30_000.0, 2_000.0);
        let edge = through(&mut notch, 31_000.0);
        assert!((0.62..0.78).contains(&edge), "edge gain {edge}");
    }

    #[test]
    fn a_retune_moves_the_notch() {
        let mut notch = IqNotch::new(RATE, 30_000.0, 1_000.0);
        notch.retune(-50_000.0);
        assert!(through(&mut notch, -50_000.0) < 0.01);
        notch.reset();
        assert!(through(&mut notch, 30_000.0) > 0.95);
    }

    #[test]
    fn recovers_after_a_non_finite_sample() {
        let mut notch = IqNotch::new(RATE, 30_000.0, 1_000.0);
        let mut poisoned = vec![Complex::new(f32::NAN, 0.0); 4];
        notch.process(&mut poisoned);
        assert!(through(&mut notch, 30_000.0) < 0.01);
    }
}
