use num_complex::Complex;

const LOCK_SYMBOLS: f32 = 32.0;

const LOCK_ENTER: f32 = 0.4;

const LOCK_LEAVE: f32 = 0.2;

#[derive(Clone, Debug, Default)]
pub(super) struct LockDetector {
    quality: f32,
    locked: bool,
}

impl LockDetector {
    pub(super) fn push(&mut self, z: Complex<f32>) -> bool {
        let power = z.norm_sqr();
        let alignment = if power > f32::MIN_POSITIVE {
            (z.re * z.re - z.im * z.im) / power
        } else {
            0.0
        };
        self.quality += (alignment - self.quality) / LOCK_SYMBOLS;
        self.locked = if self.locked {
            self.quality > LOCK_LEAVE
        } else {
            self.quality > LOCK_ENTER
        };
        self.locked
    }

    pub(super) fn locked(&self) -> bool {
        self.locked
    }

    pub(super) fn quality(&self) -> f32 {
        self.quality
    }

    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}
