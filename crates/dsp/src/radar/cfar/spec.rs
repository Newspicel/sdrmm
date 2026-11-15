use super::{MAX_GUARD, MAX_TRAIN_DOPPLER, MAX_TRAIN_RANGE};
use crate::radar::threshold::CfarStatistic;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CfarSpec {
    pub stat: CfarStatistic,
    pub plane: bool,
    pub guard_range: usize,
    pub train_range: usize,
    pub guard_doppler: usize,
    pub train_doppler: usize,
    pub alpha: f32,
    pub alpha_edge: f32,
    pub min_snr: f32,
    pub min_gate: usize,
    pub clutter_half_rows: usize,
}

impl CfarSpec {
    #[must_use]
    pub fn valid(&self) -> bool {
        let positive = |value: f32| value.is_finite() && value > 0.0;
        let rank_ok = match self.stat {
            CfarStatistic::Os { rank } => rank > 0.0 && rank <= 1.0,
            CfarStatistic::Ca | CfarStatistic::Go => true,
        };
        rank_ok
            && positive(self.alpha)
            && positive(self.alpha_edge)
            && self.min_snr.is_finite()
            && self.min_snr >= 0.0
            && (1..=MAX_TRAIN_RANGE).contains(&self.train_range)
            && self.guard_range <= MAX_GUARD
            && self.guard_doppler <= MAX_GUARD
            && self.train_doppler <= MAX_TRAIN_DOPPLER
            && (!self.plane || self.train_doppler >= 1)
    }

    #[must_use]
    pub const fn cells(&self) -> usize {
        if self.plane {
            let outer = (2 * (self.guard_range + self.train_range) + 1)
                * (2 * (self.guard_doppler + self.train_doppler) + 1);
            outer - (2 * self.guard_range + 1) * (2 * self.guard_doppler + 1)
        } else {
            2 * self.train_range
        }
    }

    #[must_use]
    pub const fn statistic_cells(&self) -> usize {
        match self.stat {
            CfarStatistic::Go if self.plane => 2 * self.train_range * (2 * self.pad() + 1),
            _ => self.cells(),
        }
    }

    #[must_use]
    pub const fn edge_cells(&self) -> usize {
        if self.plane {
            self.cells()
        } else {
            2 * self.train_range
        }
    }

    pub(super) const fn pad(&self) -> usize {
        if self.plane {
            self.guard_doppler + self.train_doppler
        } else {
            0
        }
    }

    pub(super) const fn is_clutter(&self, row: usize, rows: usize) -> bool {
        row.abs_diff(rows / 2) <= self.clutter_half_rows
    }
}
