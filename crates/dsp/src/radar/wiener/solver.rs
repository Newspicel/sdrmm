use std::ops::Range;

use num_complex::Complex;

use super::{GroupPlan, GroupSums, WeightTable, rotation};
use crate::math::complex::{add_rotated_f64, narrow};
use crate::math::fft::FftPair;
use crate::math::linalg::{MAX_SOLVE_ORDER, Toeplitz, cholesky_c64, cholesky_solve_c64};
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
    /// With one Doppler tap the Gram matrix is Hermitian Toeplitz, solved by Levinson.
    toeplitz: Option<Toeplitz>,
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
            toeplitz: (groups.taps() == 1).then(|| Toeplitz::new(unknowns)),
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
        let d = self.plan.doppler_taps;
        for group in 0..self.plan.groups() {
            if !self.plan.estimation(group).contains(&batch) {
                continue;
            }
            let cycles = self.plan.cycles(group, batch);
            let shared_base = group * sums.shifts * m;
            accumulate_symmetric_shared(
                &mut sums.shared[shared_base..shared_base + sums.shifts * m],
                &shared[..m],
                d,
                cycles,
            );
            for lane in 0..lane_count {
                let source = &lanes[lane * m..(lane + 1) * m];
                let cross_base = (group * lane_count + lane) * sums.taps * m;
                accumulate_symmetric_cross(
                    &mut sums.cross[cross_base..cross_base + sums.taps * m],
                    source,
                    d,
                    cycles,
                );
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
            *slot = narrow(shared_turn(self.plan.doppler_taps, shift, cycles));
        }
        for (tap, slot) in cross.iter_mut().enumerate() {
            *slot = narrow(cross_turn(self.plan.doppler_taps, tap, cycles));
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
        if self.toeplitz.is_some() {
            for (m, value) in self.gram[..self.unknowns].iter_mut().enumerate() {
                *value = self.lags[order - 1 - m];
            }
            return;
        }
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
        if let Some(toeplitz) = &mut self.toeplitz {
            let row = &self.gram[..n];
            let base = self.loading * row[0].re;
            return [base, RETRY_LOADING * base]
                .into_iter()
                .any(|loading| toeplitz.factor(row, loading).is_ok());
        }
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
        match &self.toeplitz {
            Some(toeplitz) => toeplitz.solve(&mut self.solution[..n]),
            None => cholesky_solve_c64(&self.factor, n, &mut self.solution),
        }
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

fn shared_turn(doppler_taps: usize, shift: usize, cycles: f64) -> C64 {
    rotation((shift as f64 - 2.0 * doppler_taps as f64) * cycles)
}

fn cross_turn(doppler_taps: usize, tap: usize, cycles: f64) -> C64 {
    rotation(-(tap as f64 - doppler_taps as f64) * cycles)
}

fn accumulate_symmetric_shared(buf: &mut [C64], source: &[C32], doppler_taps: usize, cycles: f64) {
    let m = source.len();
    #[cfg(target_arch = "aarch64")]
    if doppler_taps == 1 && m.is_multiple_of(4) {
        let outer = shared_turn(1, 4, cycles);
        add_sym_5_neon(buf, source, shared_turn(1, 3, cycles), outer);
        return;
    }
    for (shift, sums) in buf.chunks_exact_mut(m).enumerate() {
        add_rotated_f64(sums, source, shared_turn(doppler_taps, shift, cycles));
    }
}

fn accumulate_symmetric_cross(buf: &mut [C64], source: &[C32], doppler_taps: usize, cycles: f64) {
    let m = source.len();
    #[cfg(target_arch = "aarch64")]
    if doppler_taps == 1 && m.is_multiple_of(4) {
        add_sym_3_neon(buf, source, cross_turn(1, 0, cycles));
        return;
    }
    for (tap, sums) in buf.chunks_exact_mut(m).enumerate() {
        add_rotated_f64(sums, source, cross_turn(doppler_taps, tap, cycles));
    }
}

#[cfg(target_arch = "aarch64")]
fn add_sym_5_neon(buf: &mut [C64], source: &[C32], t1: C64, t2: C64) {
    use std::arch::aarch64::{
        float64x2x2_t, vaddq_f64, vcvt_f64_f32, vcvt_high_f64_f32, vdupq_n_f64, vfmaq_f64,
        vfmsq_f64, vget_low_f32, vld2q_f32, vld2q_f64, vst2q_f64,
    };
    let m = source.len();
    assert!(buf.len() == 5 * m && m.is_multiple_of(4));
    unsafe {
        let xp = source.as_ptr().cast::<f32>();
        let p0 = buf.as_mut_ptr().cast::<f64>();
        let p1 = p0.add(2 * m);
        let p2 = p0.add(4 * m);
        let p3 = p0.add(6 * m);
        let p4 = p0.add(8 * m);
        let (c1, s1) = (vdupq_n_f64(t1.re), vdupq_n_f64(t1.im));
        let (c2, s2) = (vdupq_n_f64(t2.re), vdupq_n_f64(t2.im));
        let mut i = 0;
        while i < m {
            let vs = vld2q_f32(xp.add(i * 2));
            let halves = [
                (
                    vcvt_f64_f32(vget_low_f32(vs.0)),
                    vcvt_f64_f32(vget_low_f32(vs.1)),
                    i * 2,
                ),
                (vcvt_high_f64_f32(vs.0), vcvt_high_f64_f32(vs.1), i * 2 + 4),
            ];
            for (xr, xi, off) in halves {
                let a2 = vld2q_f64(p2.add(off));
                vst2q_f64(
                    p2.add(off),
                    float64x2x2_t(vaddq_f64(a2.0, xr), vaddq_f64(a2.1, xi)),
                );
                let a3 = vld2q_f64(p3.add(off));
                vst2q_f64(
                    p3.add(off),
                    float64x2x2_t(
                        vfmsq_f64(vfmaq_f64(a3.0, c1, xr), s1, xi),
                        vfmaq_f64(vfmaq_f64(a3.1, c1, xi), s1, xr),
                    ),
                );
                let a1 = vld2q_f64(p1.add(off));
                vst2q_f64(
                    p1.add(off),
                    float64x2x2_t(
                        vfmaq_f64(vfmaq_f64(a1.0, c1, xr), s1, xi),
                        vfmsq_f64(vfmaq_f64(a1.1, c1, xi), s1, xr),
                    ),
                );
                let a4 = vld2q_f64(p4.add(off));
                vst2q_f64(
                    p4.add(off),
                    float64x2x2_t(
                        vfmsq_f64(vfmaq_f64(a4.0, c2, xr), s2, xi),
                        vfmaq_f64(vfmaq_f64(a4.1, c2, xi), s2, xr),
                    ),
                );
                let a0 = vld2q_f64(p0.add(off));
                vst2q_f64(
                    p0.add(off),
                    float64x2x2_t(
                        vfmaq_f64(vfmaq_f64(a0.0, c2, xr), s2, xi),
                        vfmsq_f64(vfmaq_f64(a0.1, c2, xi), s2, xr),
                    ),
                );
            }
            i += 4;
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn add_sym_3_neon(buf: &mut [C64], source: &[C32], t1: C64) {
    use std::arch::aarch64::{
        float64x2x2_t, vaddq_f64, vcvt_f64_f32, vcvt_high_f64_f32, vdupq_n_f64, vfmaq_f64,
        vfmsq_f64, vget_low_f32, vld2q_f32, vld2q_f64, vst2q_f64,
    };
    let m = source.len();
    assert!(buf.len() == 3 * m && m.is_multiple_of(4));
    unsafe {
        let xp = source.as_ptr().cast::<f32>();
        let p0 = buf.as_mut_ptr().cast::<f64>();
        let p1 = p0.add(2 * m);
        let p2 = p0.add(4 * m);
        let (c1, s1) = (vdupq_n_f64(t1.re), vdupq_n_f64(t1.im));
        let mut i = 0;
        while i < m {
            let vs = vld2q_f32(xp.add(i * 2));
            let halves = [
                (
                    vcvt_f64_f32(vget_low_f32(vs.0)),
                    vcvt_f64_f32(vget_low_f32(vs.1)),
                    i * 2,
                ),
                (vcvt_high_f64_f32(vs.0), vcvt_high_f64_f32(vs.1), i * 2 + 4),
            ];
            for (xr, xi, off) in halves {
                let a1 = vld2q_f64(p1.add(off));
                vst2q_f64(
                    p1.add(off),
                    float64x2x2_t(vaddq_f64(a1.0, xr), vaddq_f64(a1.1, xi)),
                );
                let a0 = vld2q_f64(p0.add(off));
                vst2q_f64(
                    p0.add(off),
                    float64x2x2_t(
                        vfmsq_f64(vfmaq_f64(a0.0, c1, xr), s1, xi),
                        vfmaq_f64(vfmaq_f64(a0.1, c1, xi), s1, xr),
                    ),
                );
                let a2 = vld2q_f64(p2.add(off));
                vst2q_f64(
                    p2.add(off),
                    float64x2x2_t(
                        vfmaq_f64(vfmaq_f64(a2.0, c1, xr), s1, xi),
                        vfmsq_f64(vfmaq_f64(a2.1, c1, xi), s1, xr),
                    ),
                );
            }
            i += 4;
        }
    }
}
