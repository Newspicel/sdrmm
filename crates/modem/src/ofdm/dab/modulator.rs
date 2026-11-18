use num_complex::Complex;
use sdrmm_dsp::math::fft::Transform;

use super::{
    Mode, TransmissionMode, interleaving_for_mode, map_symbol_for_mode, reference_symbol_for_mode,
};

/// Builds DAB transmission frames: a null symbol, the phase reference, then one differential
/// QPSK symbol per slice of bits.
pub struct FrameModulator {
    mode: Mode,
    transmission_mode: TransmissionMode,
    inverse: Transform,
    bins: Vec<usize>,
    reference: Vec<Complex<f32>>,
}

impl FrameModulator {
    #[must_use]
    pub fn new(transmission_mode: TransmissionMode) -> Self {
        let mode = Mode::new(transmission_mode);
        let bins = interleaving_for_mode(transmission_mode);
        let spectrum = reference_symbol_for_mode(transmission_mode);
        let reference = bins.iter().map(|&bin| spectrum[bin]).collect();
        Self {
            mode,
            transmission_mode,
            inverse: Transform::inverse(mode.useful),
            bins,
            reference,
        }
    }

    #[must_use]
    pub const fn mode(&self) -> Mode {
        self.mode
    }

    /// Appends one frame carrying `symbols`, each `mode().symbol_bits()` long.
    pub fn frame(&mut self, symbols: &[Vec<bool>], out: &mut Vec<Complex<f32>>) {
        out.extend(std::iter::repeat_n(Complex::new(0.0, 0.0), self.mode.null));
        let mut previous = self.reference.clone();
        self.emit(&previous, out);
        for bits in symbols {
            let mapped = map_symbol_for_mode(self.transmission_mode, bits);
            let points: Vec<Complex<f32>> = mapped
                .iter()
                .zip(&previous)
                .map(|(&point, &reference)| point * reference)
                .collect();
            self.emit(&points, out);
            previous = points;
        }
    }

    fn emit(&mut self, points: &[Complex<f32>], out: &mut Vec<Complex<f32>>) {
        let mut time = vec![Complex::new(0.0, 0.0); self.mode.useful];
        for (index, &bin) in self.bins.iter().enumerate() {
            time[bin] = points[index];
        }
        self.inverse.process_unitary(&mut time);
        out.extend_from_slice(&time[self.mode.useful - self.mode.guard..]);
        out.extend_from_slice(&time);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_has_the_documented_length_and_a_null_symbol() {
        let mut modulator = FrameModulator::new(TransmissionMode::I);
        let mode = modulator.mode();
        let symbols = vec![vec![true; mode.symbol_bits()]; mode.symbols - 1];
        let mut iq = Vec::new();
        modulator.frame(&symbols, &mut iq);
        assert_eq!(iq.len(), mode.frame());
        assert!(iq[..mode.null].iter().all(|value| value.norm() < 1e-9));
    }
}
