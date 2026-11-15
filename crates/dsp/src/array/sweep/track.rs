pub(super) const LEVEL_CAPACITY: usize = 1024;
/// The span the current turn rate is measured over.
const RATE_WINDOW_S: f64 = 0.3;

/// The levels of the sweep under way, each with the unwrapped heading it was heard at.
pub(super) struct Track {
    pub(super) t: [f64; LEVEL_CAPACITY],
    pub(super) level: [f32; LEVEL_CAPACITY],
    pub(super) heading: [f64; LEVEL_CAPACITY],
    pub(super) sigma: [f32; LEVEL_CAPACITY],
    pub(super) len: usize,
}

impl Track {
    pub(super) fn new() -> Self {
        Self {
            t: [0.0; LEVEL_CAPACITY],
            level: [0.0; LEVEL_CAPACITY],
            heading: [0.0; LEVEL_CAPACITY],
            sigma: [0.0; LEVEL_CAPACITY],
            len: 0,
        }
    }

    pub(super) fn push(&mut self, t: f64, level: f32, heading: f64, sigma: f32) {
        let at = self.len;
        self.t[at] = t;
        self.level[at] = level;
        self.heading[at] = heading;
        self.sigma[at] = sigma;
        self.len += 1;
    }

    pub(super) fn drop_front(&mut self, count: usize) {
        let count = count.min(self.len);
        let len = self.len;
        self.t.copy_within(count..len, 0);
        self.level.copy_within(count..len, 0);
        self.heading.copy_within(count..len, 0);
        self.sigma.copy_within(count..len, 0);
        self.len -= count;
    }

    pub(super) fn last(&self) -> usize {
        self.len.saturating_sub(1)
    }

    pub(super) fn window_start(&self) -> usize {
        let newest = self.t[self.last()];
        let mut start = self.last();
        while start > 0 && newest - self.t[start - 1] <= RATE_WINDOW_S {
            start -= 1;
        }
        start
    }

    pub(super) fn rate_dps(&self) -> f64 {
        let last = self.last();
        let start = self.window_start();
        if start == last {
            return 0.0;
        }
        (self.heading[last] - self.heading[start]) / (self.t[last] - self.t[start])
    }

    /// The last sample before the heading first moved more than `within_deg`.
    pub(super) fn onset(&self, within_deg: f64) -> usize {
        let first = self.heading[0];
        (1..self.len)
            .find(|&index| (self.heading[index] - first).abs() > within_deg)
            .map_or(0, |moved| moved - 1)
    }

    /// The sample turned furthest in the direction of `sign`.
    pub(super) fn extreme(&self, sign: f64) -> usize {
        (1..self.len).fold(0, |best, index| {
            if sign * self.heading[index] > sign * self.heading[best] {
                index
            } else {
                best
            }
        })
    }
}
