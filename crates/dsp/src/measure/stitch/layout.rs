use super::{
    MAX_LANES, STITCH_FFT, Stitcher,
    lane::{Tap, lane_mask, signed_bin},
    matching::{Match, Overlap},
};

/// The least two neighbouring lanes may share, as a fraction of a lane.
const MIN_OVERLAP: f64 = 0.02;

impl Stitcher {
    /// Centres the output between the outermost lanes.
    pub(super) fn centre_lanes(&mut self) {
        let shifts = self.lanes.iter().map(|lane| lane.shift_bins);
        let low = shifts.clone().min().unwrap_or(0);
        let high = shifts.max().unwrap_or(0);
        self.center_bins = (low + high).div_euclid(2);
        for lane in &mut self.lanes {
            lane.out_shift = lane.shift_bins - self.center_bins;
        }
    }

    pub(super) fn place_taps(&mut self) {
        let half = self.out_len as i64 / 2;
        let out_len = self.out_len as i64;
        for lane in &mut self.lanes {
            lane.taps.clear();
            for lane_bin in 0..STITCH_FFT {
                let bin = signed_bin(lane_bin, STITCH_FFT);
                let mask = lane_mask(bin);
                let target = bin + lane.out_shift;
                if mask <= 0.0 || target < -half || target >= half {
                    continue;
                }
                let out_bin = target.rem_euclid(out_len) as usize;
                lane.taps.push(Tap::new(lane_bin, out_bin, mask));
            }
        }
    }

    /// Pairs each lane with its neighbour in frequency, keeping the match of a pair neither of
    /// whose lanes moved.
    pub(super) fn pair_lanes(&mut self, changed: &[bool; MAX_LANES]) {
        let lanes = &self.lanes;
        self.order
            .sort_unstable_by(|a, b| lanes[*a].offset_hz.total_cmp(&lanes[*b].offset_hz));
        self.previous.clear();
        self.previous.extend_from_slice(&self.matches);
        self.matches.clear();
        self.overlaps.clear();
        self.gap = None;
        for window in 0..self.order.len().saturating_sub(1) {
            let (low, high) = (self.order[window], self.order[window + 1]);
            let start = self.overlaps.len();
            let shared = self.find_overlap(low, high);
            if (shared as f64) < MIN_OVERLAP * STITCH_FFT as f64 && self.gap.is_none() {
                self.gap = Some((low, high));
            }
            if shared == 0 {
                continue;
            }
            let kept = self
                .previous
                .iter()
                .find(|old| old.low == low && old.high == high && !changed[low] && !changed[high])
                .copied()
                .unwrap_or_else(|| Match::fresh(low, high));
            self.matches.push(kept.spanning(start, start + shared));
        }
    }

    fn find_overlap(&mut self, low: usize, high: usize) -> usize {
        self.sums.fill(-1.0);
        for tap in &self.lanes[low].taps {
            self.sums[tap.out_bin] = tap.lane_bin as f32;
        }
        let start = self.overlaps.len();
        for tap in &self.lanes[high].taps {
            let found = self.sums[tap.out_bin];
            if found >= 0.0 {
                self.overlaps.push(Overlap {
                    low_bin: found as usize,
                    high_bin: tap.lane_bin,
                    low_seen: 0,
                    high_seen: 0,
                });
            }
        }
        self.overlaps.len() - start
    }
}
