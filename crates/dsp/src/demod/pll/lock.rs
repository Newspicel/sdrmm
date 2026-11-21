const MIN_COEFF: f64 = 1e-4;
const MAX_COEFF: f64 = 0.5;

/// Smoothed lock quality in [0, 1]: one when the phase error sits at zero, zero when it
/// spreads evenly over `range`.
#[derive(Clone, Debug)]
pub(super) struct LockDetector {
    value: f64,
    coeff: f64,
    scale: f64,
}

impl LockDetector {
    pub(super) fn new(loop_bw: f64, range: f64) -> Self {
        Self {
            value: 0.0,
            coeff: loop_bw.clamp(MIN_COEFF, MAX_COEFF),
            scale: 2.0 / range,
        }
    }

    #[inline]
    pub(super) fn update(&mut self, error: Option<f64>) {
        let quality = error.map_or(0.0, |e| 1.0 - e.abs() * self.scale);
        self.value += self.coeff * (quality - self.value);
    }

    pub(super) fn value(&self) -> f32 {
        self.value.clamp(0.0, 1.0) as f32
    }
}
