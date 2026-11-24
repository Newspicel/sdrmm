/// A running log of a sum of exponentials, kept against its largest term so it neither
/// overflows nor loses the small terms.
#[derive(Clone, Copy)]
pub(super) struct LogSum {
    max: f64,
    sum: f64,
}

impl LogSum {
    pub(super) const EMPTY: Self = Self {
        max: f64::NEG_INFINITY,
        sum: 0.0,
    };

    pub(super) fn add(&mut self, x: f64) {
        if x > self.max {
            self.sum = self.sum * (self.max - x).exp() + 1.0;
            self.max = x;
        } else {
            self.sum += (x - self.max).exp();
        }
    }

    pub(super) fn value(self) -> f64 {
        self.max + self.sum.ln()
    }
}
