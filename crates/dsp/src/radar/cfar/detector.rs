use std::ops::Range;

use super::range::fill_prefix;
use super::{CfarSpec, MAX_GUARD, MAX_PAD, MAX_TRAIN_DOPPLER, MAX_TRAIN_RANGE};
use crate::radar::RadarDspError;
use crate::radar::strongest::Strongest;
use crate::radar::threshold::os_order;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Hit {
    pub row: u32,
    pub gate: u32,
    pub power: f32,
    pub noise: f32,
}

/// Cell-averaging, greatest-of or order-statistic CFAR over a range-Doppler map, along range
/// only or across a range-Doppler plane.
pub struct Cfar {
    pub(super) spec: CfarSpec,
    pub(super) gates: usize,
    pub(super) rows: usize,
    pub(super) prefix: Vec<f64>,
    pub(super) table: Vec<f64>,
    pub(super) scratch: Vec<f32>,
}

impl Cfar {
    pub fn new(spec: CfarSpec, gates: usize, rows: usize) -> Result<Self, RadarDspError> {
        if !spec.valid() || gates == 0 || rows == 0 {
            return Err(RadarDspError::Setting);
        }
        let widest = CfarSpec {
            plane: true,
            guard_range: MAX_GUARD,
            train_range: MAX_TRAIN_RANGE,
            guard_doppler: MAX_GUARD,
            train_doppler: MAX_TRAIN_DOPPLER,
            ..spec
        };
        Ok(Self {
            spec,
            gates,
            rows,
            prefix: vec![0.0; gates + 1],
            table: vec![0.0; (rows + 2 * MAX_PAD + 1) * (gates + 1)],
            scratch: Vec::with_capacity(widest.cells()),
        })
    }

    pub fn set_spec(&mut self, spec: CfarSpec) -> Result<(), RadarDspError> {
        if !spec.valid() {
            return Err(RadarDspError::Setting);
        }
        self.spec = spec;
        Ok(())
    }

    #[must_use]
    pub const fn spec(&self) -> CfarSpec {
        self.spec
    }

    pub fn detect(
        &mut self,
        power: &[f32],
        report_rows: Range<usize>,
        out: &mut Vec<Hit>,
        capacity: usize,
    ) -> Result<usize, RadarDspError> {
        out.clear();
        if power.len() < self.rows * self.gates {
            return Err(RadarDspError::Shape);
        }
        out.reserve(capacity);
        let mut keep = Strongest::new(capacity);
        if self.spec.plane {
            self.build_table(power);
        }
        let rows = report_rows.start.min(self.rows)..report_rows.end.min(self.rows);
        for row in rows {
            if self.spec.is_clutter(row, self.rows) {
                continue;
            }
            let line = &power[row * self.gates..(row + 1) * self.gates];
            if !self.spec.plane {
                fill_prefix(&mut self.prefix, line);
            }
            for gate in self.spec.min_gate..self.gates {
                let (noise, factor) = if self.spec.plane {
                    self.plane_noise(power, row, gate)
                } else {
                    self.range_noise(line, gate)
                };
                let cell = line[gate];
                if noise > 0.0 && cell > factor * noise && cell >= self.spec.min_snr * noise {
                    let hit = Hit {
                        row: row as u32,
                        gate: gate as u32,
                        power: cell,
                        noise,
                    };
                    keep.offer(out, hit, |hit| hit.power);
                }
            }
        }
        out.sort_unstable_by_key(|hit| (hit.row, hit.gate));
        Ok(keep.dropped())
    }
}

pub(super) fn order_statistic(cells: &mut [f32], rank: f32) -> f32 {
    if cells.is_empty() {
        return 0.0;
    }
    let order = os_order(rank, cells.len()) - 1;
    let (_, value, _) = cells.select_nth_unstable_by(order, f32::total_cmp);
    *value
}
