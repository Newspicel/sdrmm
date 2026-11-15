use num_complex::Complex;
use sdrmm_dsp::fast_arg;

const SQUARE_SYMBOLS: f32 = 8.0;

const FINE_ENTER: f32 = 0.3;

const FINE_LEAVE: f32 = 0.15;

const FINE_RANGE_RAD: f64 = 0.3;

#[derive(Clone, Debug, Default)]
pub(super) struct SquareLock {
    average: Complex<f32>,
    fine: bool,
}

impl SquareLock {
    pub(super) fn push(&mut self, z: Complex<f32>, allowed: bool) -> Option<f64> {
        let before = self.average;
        self.average += (z * z - self.average) / SQUARE_SYMBOLS;
        let coherence = self.average.norm();
        self.fine = allowed
            && if self.fine {
                coherence > FINE_LEAVE
            } else {
                coherence > FINE_ENTER
            };
        self.fine.then(|| {
            (0.5 * f64::from(fast_arg(self.average * before.conj())))
                .clamp(-FINE_RANGE_RAD, FINE_RANGE_RAD)
        })
    }

    pub(super) fn fine(&self) -> bool {
        self.fine
    }
}
