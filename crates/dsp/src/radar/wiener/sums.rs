use num_complex::Complex;

use super::GroupPlan;
use crate::math::complex::widen;
use crate::radar::RadarDspError;
use crate::radar::batch::BatchShape;

type C32 = Complex<f32>;
type C64 = Complex<f64>;

/// Clutter correlation spectra summed per group, ready for `WienerSolver::solve`.
pub struct GroupSums {
    pub(super) groups: usize,
    pub(super) lanes: usize,
    pub(super) taps: usize,
    pub(super) shifts: usize,
    pub(super) fft_len: usize,
    pub(super) shared: Vec<C64>,
    pub(super) cross: Vec<C64>,
    pub(super) energy: Vec<f64>,
}

impl GroupSums {
    #[must_use]
    pub fn new(shape: &BatchShape, plan: &GroupPlan) -> Self {
        let (groups, m) = (plan.groups(), shape.fft_len);
        Self {
            groups,
            lanes: shape.lanes,
            taps: plan.taps(),
            shifts: plan.shifts(),
            fft_len: m,
            shared: vec![C64::default(); groups * plan.shifts() * m],
            cross: vec![C64::default(); groups * shape.lanes * plan.taps() * m],
            energy: vec![0.0; groups * shape.lanes],
        }
    }

    pub fn clear(&mut self) {
        self.shared.fill(C64::default());
        self.cross.fill(C64::default());
        self.energy.fill(0.0);
    }

    #[must_use]
    pub const fn vectors(&self) -> usize {
        self.shifts + self.lanes * self.taps
    }

    pub fn load(&mut self, group: usize, vectors: &[C32]) -> Result<(), RadarDspError> {
        let shared = self.shifts * self.fft_len;
        let cross = self.lanes * self.taps * self.fft_len;
        if group >= self.groups || vectors.len() != shared + cross {
            return Err(RadarDspError::Shape);
        }
        let (head, tail) = vectors.split_at(shared);
        widen_into(&mut self.shared[group * shared..(group + 1) * shared], head);
        widen_into(&mut self.cross[group * cross..(group + 1) * cross], tail);
        Ok(())
    }

    pub(super) fn shared(&self, group: usize, shift: usize) -> &[C64] {
        let at = (group * self.shifts + shift) * self.fft_len;
        &self.shared[at..at + self.fft_len]
    }

    pub(super) fn cross(&self, group: usize, lane: usize, tap: usize) -> &[C64] {
        let at = ((group * self.lanes + lane) * self.taps + tap) * self.fft_len;
        &self.cross[at..at + self.fft_len]
    }
}

fn widen_into(out: &mut [C64], source: &[C32]) {
    for (slot, value) in out.iter_mut().zip(source) {
        *slot = widen(*value);
    }
}
