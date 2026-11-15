use super::MAX_SNAPSHOTS;
use crate::radar::cfar::Hit;

/// A cluster while it grows: its peak, power-weighted centroid sums and strongest cells.
#[derive(Clone, Copy, Debug)]
pub(super) struct Growth {
    pub(super) peak: Hit,
    weight: f64,
    row_sum: f64,
    gate_sum: f64,
    pub(super) cells: u32,
    pub(super) min_row: u32,
    pub(super) max_row: u32,
    pub(super) strongest: [(f32, u32, u32); MAX_SNAPSHOTS],
    pub(super) held: usize,
}

impl Growth {
    pub(super) fn new(seed: Hit) -> Self {
        Self {
            peak: seed,
            weight: 0.0,
            row_sum: 0.0,
            gate_sum: 0.0,
            cells: 0,
            min_row: seed.row,
            max_row: seed.row,
            strongest: [(0.0, 0, 0); MAX_SNAPSHOTS],
            held: 0,
        }
    }

    pub(super) fn absorb(&mut self, hit: Hit) {
        if hit.power > self.peak.power {
            self.peak = hit;
        }
        let weight = f64::from(hit.power.max(0.0));
        self.weight += weight;
        self.row_sum += weight * f64::from(hit.row);
        self.gate_sum += weight * f64::from(hit.gate);
        self.cells += 1;
        self.min_row = self.min_row.min(hit.row);
        self.max_row = self.max_row.max(hit.row);
        let entry = (hit.power, hit.row, hit.gate);
        if self.held < MAX_SNAPSHOTS {
            self.strongest[self.held] = entry;
            self.held += 1;
        } else if let Some(weakest) = self.strongest.iter_mut().min_by(|a, b| a.0.total_cmp(&b.0))
            && weakest.0 < hit.power
        {
            *weakest = entry;
        }
    }

    pub(super) fn centroid(&self) -> (f32, f32) {
        if self.weight > 0.0 {
            (
                (self.row_sum / self.weight) as f32,
                (self.gate_sum / self.weight) as f32,
            )
        } else {
            (self.peak.row as f32, self.peak.gate as f32)
        }
    }
}
