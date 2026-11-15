/// Running standard deviation of a set of values.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Spread {
    count: f64,
    sum: f64,
    squares: f64,
}

impl Spread {
    pub(super) fn push(self, value: f64) -> Self {
        Self {
            count: self.count + 1.0,
            sum: self.sum + value,
            squares: self.squares + value * value,
        }
    }

    pub(super) fn rms(self) -> f64 {
        if self.count == 0.0 {
            return 0.0;
        }
        let mean = self.sum / self.count;
        (self.squares / self.count - mean * mean).max(0.0).sqrt()
    }
}
