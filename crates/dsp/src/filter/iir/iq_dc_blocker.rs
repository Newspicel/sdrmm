use num_complex::Complex;

use super::{STRIDE, corner_coeff};

/// Removes a zero-IF front end's own DC term from a complex stream.
///
/// A signal genuinely at 0 Hz is arithmetically indistinguishable from that offset and is removed
/// with it, so the corner must stay far below one analysis bin.
#[derive(Clone, Debug)]
pub struct IqDcBlocker {
    mean: Complex<f32>,
    coeff: f32,
    powers: [f32; STRIDE + 1],
}

impl IqDcBlocker {
    #[must_use]
    pub fn new(rate: f64, corner_hz: f64) -> Self {
        assert!(
            rate > 0.0 && corner_hz > 0.0,
            "rate and corner must be positive"
        );
        let coeff = corner_coeff(rate, corner_hz);
        let keep = 1.0 - coeff;
        Self {
            mean: Complex::new(0.0, 0.0),
            coeff,
            powers: std::array::from_fn(|power| keep.powi(power as i32)),
        }
    }

    pub fn reset(&mut self) {
        self.mean = Complex::new(0.0, 0.0);
    }

    pub fn hold(&self, samples: &mut [Complex<f32>]) {
        let mean = if self.mean.is_finite() {
            self.mean
        } else {
            Complex::new(0.0, 0.0)
        };
        for s in samples {
            *s = if s.is_finite() {
                *s - mean
            } else {
                Complex::new(0.0, 0.0)
            };
        }
    }

    pub fn process(&mut self, samples: &mut [Complex<f32>]) {
        if !self.mean.is_finite() {
            self.reset();
        }
        let (strides, tail) = samples.as_chunks_mut::<STRIDE>();
        for stride in strides {
            if stride.iter().all(|s| s.is_finite()) {
                self.stride(stride);
            } else {
                for s in stride {
                    self.step(s);
                }
            }
        }
        for s in tail {
            self.step(s);
        }
    }

    fn stride(&mut self, stride: &mut [Complex<f32>; STRIDE]) {
        let [x1, x2, x3, x4] = *stride;
        let [_, a1, a2, a3, a4] = self.powers;
        let c = self.coeff;
        let m0 = self.mean;
        let m1 = m0 * a1 + x1 * c;
        let m2 = m0 * a2 + (x1 * a1 + x2) * c;
        let m3 = m0 * a3 + (x1 * a2 + x2 * a1 + x3) * c;
        let m4 = m0 * a4 + (x1 * a3 + x2 * a2 + x3 * a1 + x4) * c;
        *stride = [x1 - m1, x2 - m2, x3 - m3, x4 - m4];
        self.mean = m4;
    }

    fn step(&mut self, s: &mut Complex<f32>) {
        if s.is_finite() {
            self.mean += (*s - self.mean) * self.coeff;
            *s -= self.mean;
        } else {
            *s = Complex::new(0.0, 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        math::complex::{narrow, widen},
        testutil::{complex_tone, rms_c},
    };

    #[test]
    fn strided_dc_blocking_matches_the_one_pole_recursion() {
        let mut samples: Vec<Complex<f32>> = complex_tone(0.013, 3_001)
            .into_iter()
            .map(|s| s + Complex::new(0.4, -0.3))
            .collect();
        samples[1_234] = Complex::new(f32::NAN, 0.0);
        let mut reference = samples.clone();
        let mut blocker = IqDcBlocker::new(48_000.0, 20.0);
        let coeff = blocker.coeff;
        blocker.process(&mut samples);
        let mut mean = Complex::new(0.0f64, 0.0);
        for s in &mut reference {
            if s.is_finite() {
                let x = widen(*s);
                mean += (x - mean) * f64::from(coeff);
                *s = narrow(x - mean);
            } else {
                *s = Complex::new(0.0, 0.0);
            }
        }
        for (got, want) in samples.iter().zip(&reference) {
            assert!((got - want).norm() < 1e-5, "{got} vs {want}");
        }
        assert_eq!(samples[1_234], Complex::new(0.0, 0.0));
    }
    const IQ_RATE: f64 = 2_400_000.0;
    const IQ_CORNER: f64 = 20.0;

    #[test]
    fn iq_dc_blocker_removes_a_complex_offset() {
        let mut blocker = IqDcBlocker::new(IQ_RATE, IQ_CORNER);
        let mut x = vec![Complex::new(0.021, -0.014); 1 << 19];
        blocker.process(&mut x);
        let residue = rms_c(&x[x.len() / 2..]);
        assert!(residue < 1e-4, "dc residue {residue}");
    }

    #[test]
    fn iq_dc_blocker_leaves_a_carrier_one_bin_off_dc_alone() {
        let n = 1 << 19;
        let bin_hz = IQ_RATE / 4_096.0;
        let mut x = complex_tone(bin_hz / IQ_RATE, n);
        let clean = rms_c(&x[n / 2..]);
        IqDcBlocker::new(IQ_RATE, IQ_CORNER).process(&mut x);
        let gain = rms_c(&x[n / 2..]) / clean;
        assert!((0.99..1.01).contains(&gain), "one bin off dc: gain {gain}");
    }

    #[test]
    fn iq_dc_blocker_separates_an_offset_from_a_carrier_riding_on_it() {
        let n = 1 << 19;
        let offset = Complex::new(0.05, 0.03);
        let mut x = complex_tone(50_000.0 / IQ_RATE, n);
        for s in &mut x {
            *s += offset;
        }
        IqDcBlocker::new(IQ_RATE, IQ_CORNER).process(&mut x);
        let tail = &x[n / 2..];
        let mean: Complex<f32> = tail.iter().sum::<Complex<f32>>() / tail.len() as f32;
        assert!(mean.norm() < 1e-3, "offset survived: {}", mean.norm());
        assert!(
            (rms_c(tail) - 1.0).abs() < 0.01,
            "carrier lost: {}",
            rms_c(tail)
        );
    }

    #[test]
    fn iq_dc_blocker_recovers_after_a_non_finite_sample() {
        let mut blocker = IqDcBlocker::new(IQ_RATE, IQ_CORNER);
        let mut poisoned = vec![Complex::new(0.5, 0.5); 4_096];
        poisoned[0] = Complex::new(f32::NAN, 0.0);
        blocker.process(&mut poisoned);
        assert!(
            poisoned.iter().all(|s| s.is_finite()),
            "a non-finite sample poisoned the stream"
        );
    }

    #[test]
    fn a_held_iq_dc_blocker_keeps_its_estimate_through_a_burst() {
        let offset = Complex::new(0.02, -0.01);
        let mut blocker = IqDcBlocker::new(IQ_RATE, IQ_CORNER);
        let mut settled = vec![offset; 1 << 19];
        blocker.process(&mut settled);
        let mut burst = vec![Complex::new(0.9, 0.9); 1 << 16];
        blocker.hold(&mut burst);
        assert!((burst[0] - Complex::new(0.88, 0.91)).norm() < 1e-3);
        let mut after = vec![offset; 8];
        blocker.process(&mut after);
        assert!(after[0].norm() < 1e-3, "the burst moved the estimate");
    }

    #[test]
    fn iq_dc_blocker_resets_its_estimate() {
        let mut blocker = IqDcBlocker::new(IQ_RATE, IQ_CORNER);
        let mut settled = vec![Complex::new(1.0, 0.0); 1 << 19];
        blocker.process(&mut settled);
        blocker.reset();
        let mut fresh = vec![Complex::new(1.0, 0.0); 8];
        blocker.process(&mut fresh);
        assert!(
            fresh[0].norm() > 0.99,
            "a reset blocker kept its old estimate"
        );
    }
}
