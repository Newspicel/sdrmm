use super::HeadingSample;
use crate::math::special::{norm_deg, wrap_deg};

const HEADING_CAPACITY: usize = 2048;
/// How far past its nearest heading a level may be placed by extrapolation or interpolation.
const HEADING_REACH_S: f64 = 0.5;

/// Recent headings, unwrapped so a turn through north keeps counting instead of jumping back.
pub(super) struct HeadingRing {
    t: [f64; HEADING_CAPACITY],
    heading: [f64; HEADING_CAPACITY],
    sigma: [f32; HEADING_CAPACITY],
    start: usize,
    len: usize,
    last_raw: f64,
    pub(super) stale: u64,
}

impl HeadingRing {
    pub(super) fn new() -> Self {
        Self {
            t: [0.0; HEADING_CAPACITY],
            heading: [0.0; HEADING_CAPACITY],
            sigma: [0.0; HEADING_CAPACITY],
            start: 0,
            len: 0,
            last_raw: 0.0,
            stale: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.start = 0;
        self.len = 0;
    }

    pub(super) fn push(&mut self, sample: HeadingSample) {
        let valid = sample.t_s.is_finite() && sample.heading_deg.is_finite();
        let newest = self.len.checked_sub(1).map(|last| self.t[self.slot(last)]);
        if !valid || newest.is_some_and(|newest| sample.t_s <= newest) {
            self.stale += 1;
            return;
        }
        let unwrapped = match self.len.checked_sub(1) {
            None => norm_deg(sample.heading_deg),
            Some(last) => {
                self.heading[self.slot(last)] + wrap_deg(sample.heading_deg - self.last_raw)
            }
        };
        self.last_raw = sample.heading_deg;
        if self.len == HEADING_CAPACITY {
            self.start = (self.start + 1) % HEADING_CAPACITY;
            self.len -= 1;
        }
        let slot = self.slot(self.len);
        self.t[slot] = sample.t_s;
        self.heading[slot] = unwrapped;
        self.sigma[slot] = sample.sigma_deg;
        self.len += 1;
    }

    /// The unwrapped heading and its sigma at `t_s`, if a heading lies close enough.
    pub(super) fn unwrapped_at(&self, t_s: f64) -> Option<(f64, f32)> {
        let last = self.len.checked_sub(1)?;
        let (t_first, h_first, s_first) = self.at(0);
        let (t_last, _, _) = self.at(last);
        if t_s >= t_last {
            return self.extrapolate(t_s);
        }
        if t_s <= t_first {
            return (t_first - t_s <= HEADING_REACH_S).then_some((h_first, s_first));
        }
        let (mut low, mut high) = (0, last);
        while high - low > 1 {
            let mid = usize::midpoint(low, high);
            if self.at(mid).0 <= t_s {
                low = mid;
            } else {
                high = mid;
            }
        }
        let (t0, h0, s0) = self.at(low);
        let (t1, h1, s1) = self.at(high);
        if (t_s - t0).min(t1 - t_s) > HEADING_REACH_S {
            return None;
        }
        let weight = (t_s - t0) / (t1 - t0);
        let sigma = f64::from(s0) + weight * (f64::from(s1) - f64::from(s0));
        Some((h0 + weight * (h1 - h0), sigma as f32))
    }

    fn extrapolate(&self, t_s: f64) -> Option<(f64, f32)> {
        let last = self.len.checked_sub(1)?;
        let (t_last, h_last, s_last) = self.at(last);
        if t_s - t_last > HEADING_REACH_S {
            return None;
        }
        let rate = match last.checked_sub(1).map(|before| self.at(before)) {
            Some((t_before, h_before, _)) if t_last > t_before => {
                (h_last - h_before) / (t_last - t_before)
            }
            _ => 0.0,
        };
        Some((h_last + rate * (t_s - t_last), s_last))
    }

    fn slot(&self, index: usize) -> usize {
        (self.start + index) % HEADING_CAPACITY
    }

    fn at(&self, index: usize) -> (f64, f64, f32) {
        let slot = self.slot(index);
        (self.t[slot], self.heading[slot], self.sigma[slot])
    }
}
