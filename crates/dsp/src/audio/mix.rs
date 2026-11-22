//! Sums several named audio lanes in time, releasing samples every live lane has delivered.

use std::collections::HashMap;

/// A stable lane id for a name (FNV-1a 64).
pub fn lane_of(name: &str) -> u64 {
    name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

struct Lane {
    cursor: u64,
}

/// A lane stays until `forget()` is called.
///
/// Mixes lanes that arrive at different times. A lane more than `max_lag` samples behind the
/// leading lane stops holding the mix back and rejoins at the release point when it pushes again.
pub struct MixBus {
    max_lag: u64,
    released: u64,
    mix: Vec<f32>,
    lanes: HashMap<u64, Lane>,
}

impl MixBus {
    pub fn new(max_lag: usize) -> Self {
        Self {
            max_lag: max_lag as u64,
            released: 0,
            mix: Vec::new(),
            lanes: HashMap::new(),
        }
    }

    pub fn push(&mut self, lane: u64, samples: &[f32]) {
        let released = self.released;
        let entry = self
            .lanes
            .entry(lane)
            .or_insert_with(|| Lane { cursor: released });
        entry.cursor = entry.cursor.max(released);
        let start = (entry.cursor - released) as usize;
        let end = start + samples.len();
        if self.mix.len() < end {
            self.mix.resize(end, 0.0);
        }
        for (slot, sample) in self.mix[start..end].iter_mut().zip(samples) {
            *slot += sample;
        }
        entry.cursor += samples.len() as u64;
    }

    /// Appends the released samples, clamped to [-1, 1].
    pub fn drain(&mut self, out: &mut Vec<f32>) {
        let lead = self
            .lanes
            .values()
            .map(|l| l.cursor)
            .max()
            .unwrap_or(self.released);
        let floor = lead.saturating_sub(self.max_lag);
        let release = self
            .lanes
            .values()
            .map(|l| l.cursor)
            .filter(|&cursor| cursor >= floor)
            .min()
            .unwrap_or(self.released);
        let count = ((release - self.released) as usize).min(self.mix.len());
        out.extend(self.mix.drain(..count).map(|s| s.clamp(-1.0, 1.0)));
        self.released += count as u64;
    }

    /// Releases everything written, up to the leading lane, and starts over with no lanes.
    pub fn flush(&mut self, out: &mut Vec<f32>) {
        out.extend(self.mix.drain(..).map(|s| s.clamp(-1.0, 1.0)));
        self.lanes.clear();
        self.released = 0;
    }

    pub fn forget(&mut self, lane: u64) {
        self.lanes.remove(&lane);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drained(bus: &mut MixBus) -> Vec<f32> {
        let mut out = Vec::new();
        bus.drain(&mut out);
        out
    }

    #[test]
    fn one_lane_passes_through_at_once() {
        let mut bus = MixBus::new(100);
        bus.push(1, &[0.1, 0.2, 0.3]);
        assert_eq!(drained(&mut bus), vec![0.1, 0.2, 0.3]);
        assert!(drained(&mut bus).is_empty());
    }

    #[test]
    fn flush_releases_the_held_tail_and_resets() {
        let mut bus = MixBus::new(100);
        bus.push(1, &[0.1; 4]);
        bus.push(2, &[0.2; 2]);
        bus.drain(&mut Vec::new());
        let mut out = Vec::new();
        bus.flush(&mut out);
        assert_eq!(out, vec![0.1; 2]);
        bus.flush(&mut out);
        assert_eq!(out.len(), 2);
        bus.push(2, &[0.2; 3]);
        assert_eq!(drained(&mut bus), vec![0.2; 3]);
    }

    #[test]
    fn two_lanes_sum_in_time() {
        let mut bus = MixBus::new(100);
        bus.push(1, &[0.1; 4]);
        bus.push(2, &[0.2; 2]);
        assert_eq!(drained(&mut bus), vec![0.1 + 0.2; 2]);
        bus.push(2, &[0.2; 2]);
        assert_eq!(drained(&mut bus), vec![0.1 + 0.2; 2]);
    }

    #[test]
    fn a_silent_lane_stops_holding_the_mix_after_max_lag() {
        let mut bus = MixBus::new(10);
        bus.push(1, &[0.1; 5]);
        bus.push(2, &[0.1; 5]);
        assert_eq!(drained(&mut bus).len(), 5);
        bus.push(1, &[0.1; 20]);
        assert_eq!(drained(&mut bus).len(), 20);
        bus.push(2, &[0.2; 3]);
        bus.push(1, &[0.1; 3]);
        assert_eq!(drained(&mut bus), vec![0.1 + 0.2; 3]);
    }

    #[test]
    fn the_mix_is_clamped() {
        let mut bus = MixBus::new(100);
        bus.push(1, &[0.9, -0.9]);
        bus.push(2, &[0.9, -0.9]);
        assert_eq!(drained(&mut bus), vec![1.0, -1.0]);
    }

    #[test]
    fn forgetting_a_lane_releases_what_it_held() {
        let mut bus = MixBus::new(100);
        bus.push(1, &[0.1; 4]);
        bus.push(2, &[0.1; 1]);
        bus.forget(2);
        assert_eq!(drained(&mut bus).len(), 4);
    }

    #[test]
    fn lane_of_is_stable() {
        assert_eq!(lane_of("a"), 0xaf63_dc4c_8601_ec8c);
        assert_ne!(lane_of("alice"), lane_of("bob"));
    }
}
