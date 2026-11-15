use num_complex::Complex;

use super::cycles;

type C64 = Complex<f64>;

/// Complex exponential advanced by one fixed phase step per sample.
pub(super) struct Rotor {
    value: C64,
    step: C64,
}

impl Rotor {
    pub(super) fn new(freq_hz: f64, start_s: f64, step_s: f64) -> Self {
        Self {
            value: cycles(freq_hz, start_s),
            step: cycles(freq_hz, step_s),
        }
    }

    pub(super) fn next(&mut self) -> C64 {
        let value = self.value;
        self.value *= self.step;
        value
    }
}
