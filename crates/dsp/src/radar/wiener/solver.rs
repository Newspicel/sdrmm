use std::ops::Range;

use num_complex::Complex;

use super::{GroupPlan, GroupSums, WeightTable, rotation};
use crate::math::complex::{add_rotated_f64, narrow};
use crate::math::fft::FftPair;
use crate::math::linalg::{MAX_SOLVE_ORDER, cholesky_c64, cholesky_solve_c64};
use crate::radar::RadarDspError;
use crate::radar::batch::{BatchShape, MAX_SURVEILLANCE};
use crate::radar::ratio::ratio_db;

type C32 = Complex<f32>;
type C64 = Complex<f64>;

const RETRY_LOADING: f64 = 100.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SolveStats {
    pub suppression_db: [f32; MAX_SURVEILLANCE],
    pub unsuppressed_groups: u32,
}

pub struct WienerSolver {
    shape: BatchShape,
    plan: GroupPlan,
    loading: f64,
    unknowns: usize,
    fft: FftPair<f64>,
    buffer: Vec<C64>,
    lags: Vec<C64>,
    gram: Vec<C64>,
    factor: Vec<C64>,
    rhs: Vec<C64>,
    solution: Vec<C64>,
}

impl WienerSolver {
    pub fn new(shape: BatchShape, groups: &GroupPlan, loading: f32) -> Result<Self, RadarDspError> {
        if !shape.eca() || !groups.valid_for(&shape) {
            return Err(RadarDspError::Shape);
        }
        if !(loading.is_finite() && loading >= 0.0) {
            return Err(RadarDspError::Setting);
        }
        let unknowns = shape.order() * groups.taps();
        if unknowns > MAX_SOLVE_ORDER {
            return Err(RadarDspError::Order);
        }
        let span = 2 * shape.order() - 1;
        Ok(Self {
            shape,
            plan: groups.clone(),
            loading: f64::from(loading),
            unknowns,
            fft: FftPair::new(shape.fft_len),
            buffer: vec![C64::default(); shape.fft_len],
            lags: vec![C64::default(); groups.shifts() * span],
            gram: vec![C64::default(); unknowns * unknowns],
            factor: vec![C64::default(); unknowns * unknowns],
            rhs: vec![C64::default(); unknowns],
            solution: vec![C64::default(); unknowns],
        })
    }

    #[must_use]
    pub const fn unknowns(&self) -> usize {
        self.unknowns
    }

    pub fn accumulate(
        &self,
        sums: &mut GroupSums,
        batch: usize,
        shared: &[C32],
        lanes: &[C32],
        energy: &[f64],
    ) -> Result<(), RadarDspError> {
        let (m, lane_count) = (self.shape.fft_len, self.shape.lanes);
        let fits = shared.len() >= m && lanes.len() >= lane_count * m;
        if !fits || !self.holds(sums, batch, energy) {
            return Err(RadarDspError::Shape);
        }
        for group in 0..self.plan.groups() {
            if !self.plan.estimation(group).contains(&batch) {
                continue;
            }
            let cycles = self.plan.cycles(group, batch);
            for shift in 0..sums.shifts {
                let at = (group * sums.shifts + shift) * m;
                add_rotated(
                    &mut sums.shared[at..at + m],
                    &shared[..m],
                    self.shared_turn(shift, cycles),
                );
            }
            for lane in 0..lane_count {
                let source = &lanes[lane * m..(lane + 1) * m];
                for tap in 0..sums.taps {
                    let at = ((group * lane_count + lane) * sums.taps + tap) * m;
                    add_rotated(
                        &mut sums.cross[at..at + m],
                        source,
                        self.cross_turn(tap, cycles),
                    );
                }
            }
        }
        self.accumulate_energy(sums, batch, energy)
    }

    pub fn accumulate_energy(
        &self,
        sums: &mut GroupSums,
        batch: usize,
        energy: &[f64],
    ) -> Result<(), RadarDspError> {
        if !self.holds(sums, batch, energy) {
            return Err(RadarDspError::Shape);
        }
        let lanes = self.shape.lanes;
        for group in 0..self.plan.groups() {
            if self.plan.estimation(group).contains(&batch) {
                for (total, value) in sums.energy[group * lanes..(group + 1) * lanes]
                    .iter_mut()
                    .zip(energy)
                {
                    *total += value;
                }
            }
        }
        Ok(())
    }

    pub fn estimation(&self, group: usize) -> Result<Range<usize>, RadarDspError> {
        if group >= self.plan.groups() {
            return Err(RadarDspError::Shape);
        }
        Ok(self.plan.estimation(group))
    }

    pub fn turns(&self, group: usize, batch: usize, out: &mut [C32]) -> Result<(), RadarDspError> {
        let (shifts, taps) = (self.plan.shifts(), self.plan.taps());
        let inside = group < self.plan.groups() && batch < self.shape.batches;
        let Some(out) = out.get_mut(..shifts + taps).filter(|_| inside) else {
            return Err(RadarDspError::Shape);
        };
        out.fill(C32::default());
        if !self.plan.estimation(group).contains(&batch) {
            return Ok(());
        }
        let cycles = self.plan.cycles(group, batch);
        let (shared, cross) = out.split_at_mut(shifts);
        for (shift, slot) in shared.iter_mut().enumerate() {
            *slot = narrow(self.shared_turn(shift, cycles));
        }
        for (tap, slot) in cross.iter_mut().enumerate() {
            *slot = narrow(self.cross_turn(tap, cycles));
        }
        Ok(())
    }

    pub fn solve(
        &mut self,
        sums: &GroupSums,
        table: &mut WeightTable,
    ) -> Result<SolveStats, RadarDspError> {
        let mut stats = SolveStats::default();
        let groups = self.plan.groups();
        let lanes = self.shape.lanes;
        let fits = sums.groups == groups
            && sums.lanes == lanes
            && sums.fft_len == self.shape.fft_len
            && table.lanes == lanes
            && table.taps == self.plan.taps()
            && table.order == self.shape.order()
            && table.fft_len == self.shape.fft_len
            && table.blends.len() == self.shape.batches
            && table.spectra.len() == groups * lanes * self.plan.taps() * self.shape.fft_len;
        if !fits {
            table.active = false;
            return Err(RadarDspError::Shape);
        }
        let mut totals = [0.0f64; MAX_SURVEILLANCE];
        for group in 0..groups {
            self.load_lags(sums, group);
            self.build_gram();
            if self.factor_loaded() {
                for (lane, total) in totals.iter_mut().enumerate().take(lanes) {
                    *total += f64::from(self.solve_lane(sums, table, group, lane));
                }
            } else {
                stats.unsuppressed_groups += 1;
                for lane in 0..lanes {
                    table.weights_mut(group, lane).fill(C64::default());
                    for tap in 0..self.plan.taps() {
                        table.spectrum_mut(group, lane, tap).fill(C32::default());
                    }
                }
            }
        }
        table.active = true;
        for (out, total) in stats.suppression_db.iter_mut().zip(totals).take(lanes) {
            *out = (total / groups as f64) as f32;
        }
        Ok(stats)
    }

    fn holds(&self, sums: &GroupSums, batch: usize, energy: &[f64]) -> bool {
        batch < self.shape.batches
            && energy.len() >= self.shape.lanes
            && sums.groups == self.plan.groups()
            && sums.fft_len == self.shape.fft_len
            && sums.lanes == self.shape.lanes
    }

    fn shared_turn(&self, shift: usize, cycles: f64) -> C64 {
        rotation((shift as f64 - 2.0 * self.plan.doppler_taps as f64) * cycles)
    }

    fn cross_turn(&self, tap: usize, cycles: f64) -> C64 {
        rotation(-(tap as f64 - self.plan.doppler_taps as f64) * cycles)
    }

    fn inverse_into_buffer(&mut self, source: &[C64]) {
        self.buffer.copy_from_slice(source);
        self.fft.inverse(&mut self.buffer);
        let scale = 1.0 / self.shape.fft_len as f64;
        for value in &mut self.buffer {
            *value *= scale;
        }
    }

    fn load_lags(&mut self, sums: &GroupSums, group: usize) {
        let span = 2 * self.shape.order() - 1;
        for shift in 0..sums.shifts {
            self.inverse_into_buffer(sums.shared(group, shift));
            self.lags[shift * span..(shift + 1) * span].copy_from_slice(&self.buffer[..span]);
        }
    }

    fn build_gram(&mut self) {
        let order = self.shape.order();
        let span = 2 * order - 1;
        let n = self.unknowns;
        for row in 0..n {
            let (i, k) = (row / order, row % order);
            for col in row..n {
                let (j, l) = (col / order, col % order);
                let shift = j + 2 * self.plan.doppler_taps - i;
                let value = self.lags[shift * span + k + order - 1 - l];
                self.gram[row * n + col] = value;
                self.gram[col * n + row] = value.conj();
            }
        }
    }

    fn factor_loaded(&mut self) -> bool {
        let n = self.unknowns;
        let mean = (0..n).map(|i| self.gram[i * n + i].re).sum::<f64>() / n as f64;
        let base = self.loading * mean;
        for loading in [base, RETRY_LOADING * base] {
            if cholesky_c64(&self.gram, n, loading, &mut self.factor).is_ok() {
                return true;
            }
        }
        false
    }

    fn solve_lane(
        &mut self,
        sums: &GroupSums,
        table: &mut WeightTable,
        group: usize,
        lane: usize,
    ) -> f32 {
        let order = self.shape.order();
        let lead = self.shape.lead;
        let taps = self.plan.taps();
        for tap in 0..taps {
            self.inverse_into_buffer(sums.cross(group, lane, tap));
            for k in 0..order {
                self.rhs[tap * order + k] = self.buffer[k];
            }
        }
        let n = self.unknowns;
        self.solution[..n].copy_from_slice(&self.rhs[..n]);
        cholesky_solve_c64(&self.factor, n, &mut self.solution);
        let explained: f64 = self
            .solution
            .iter()
            .zip(&self.rhs)
            .map(|(w, c)| (w.conj() * c).re)
            .sum();
        table
            .weights_mut(group, lane)
            .copy_from_slice(&self.solution);
        for tap in 0..taps {
            self.buffer.fill(C64::default());
            let m = self.shape.fft_len;
            for k in 0..order {
                let at = (k + m + 1 - self.shape.taps - lead) % m;
                self.buffer[at] = self.solution[tap * order + k];
            }
            self.fft.forward(&mut self.buffer);
            for (out, value) in table
                .spectrum_mut(group, lane, tap)
                .iter_mut()
                .zip(&self.buffer)
            {
                *out = narrow(*value);
            }
        }
        let energy = sums.energy[group * self.shape.lanes + lane];
        ratio_db(energy, energy - explained)
    }
}

fn add_rotated(sum: &mut [C64], source: &[C32], turn: C64) {
    add_rotated_f64(sum, source, turn);
}
