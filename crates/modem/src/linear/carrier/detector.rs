use num_complex::Complex;
use sdrmm_dsp::fast_arg;

use crate::constellation::Constellation;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseDetector {
    DecisionDirected,
    MthPower { m: u32 },
}

impl PhaseDetector {
    #[must_use]
    pub fn error(self, y: Complex<f32>, table: &Constellation) -> f64 {
        if y.re == 0.0 && y.im == 0.0 {
            return 0.0;
        }
        match self {
            Self::DecisionDirected => {
                let x = table.nearest(y);
                if x.re == 0.0 && x.im == 0.0 {
                    return 0.0;
                }
                f64::from(fast_arg(y * x.conj()))
            }
            Self::MthPower { m } => {
                let reference = unit_power(table.points()[0], m);
                let stripped = unit_power(y, m);
                if reference.norm_sqr() <= 0.0 || stripped.norm_sqr() <= 0.0 {
                    return 0.0;
                }
                f64::from(fast_arg(stripped * reference.conj())) / f64::from(m)
            }
        }
    }
}

/// `y` on the unit circle raised to the `m`-th power, which strips `m`-fold modulation.
pub(super) fn unit_power(y: Complex<f32>, m: u32) -> Complex<f32> {
    let norm = y.norm_sqr().sqrt();
    if norm <= 0.0 || !norm.is_finite() {
        return Complex::new(0.0, 0.0);
    }
    let unit = y / norm;
    let mut acc = Complex::new(1.0f32, 0.0);
    for _ in 0..m {
        acc *= unit;
    }
    acc
}
