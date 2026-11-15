mod mrc;
mod mvdr;
mod slc;
#[cfg(test)]
mod tests;

use num_complex::Complex;

use super::{BeamError, MAX_CONSTRAINTS, WeightSet, ZERO};
use crate::math::linalg::{
    CMat, Cholesky, Eigen, HermitianEigen, LinalgError, MAX_ORDER, norm_sqr,
};

/// Scratch row that holds the solved weights; the rows before it hold constraint columns.
const OUT: usize = MAX_CONSTRAINTS;

pub struct BlockSolver {
    n: usize,
    chol: Cholesky,
    eigen: HermitianEigen,
    values: Eigen,
    small: Cholesky,
    work: CMat,
    scratch: [[Complex<f32>; MAX_ORDER]; MAX_CONSTRAINTS + 1],
}

impl BlockSolver {
    pub fn new(n: usize) -> Result<Self, BeamError> {
        if !(2..=MAX_ORDER).contains(&n) {
            return Err(BeamError::Lanes(n));
        }
        Ok(Self {
            n,
            chol: Cholesky::new(n)?,
            eigen: HermitianEigen::new(n)?,
            values: Eigen::new(),
            small: Cholesky::new(MAX_CONSTRAINTS)?,
            work: CMat::zeros(n)?,
            scratch: [[ZERO; MAX_ORDER]; MAX_CONSTRAINTS + 1],
        })
    }

    #[must_use]
    pub const fn order(&self) -> usize {
        self.n
    }

    pub fn das(&mut self, steer: &[Complex<f32>], out: &mut WeightSet) -> Result<(), BeamError> {
        self.check_vector(steer)?;
        let energy = norm_sqr(steer);
        if !energy.is_finite() {
            return Err(LinalgError::NonFinite.into());
        }
        if energy <= 0.0 {
            return Err(LinalgError::RankDeficient(0).into());
        }
        let textbook = &mut self.scratch[OUT][..self.n];
        for (value, a) in textbook.iter_mut().zip(steer) {
            *value = a / energy;
        }
        *out = WeightSet::from_textbook(textbook);
        Ok(())
    }

    pub(super) fn check_matrix(&self, r: &CMat) -> Result<(), BeamError> {
        if r.order() != self.n {
            return Err(BeamError::Lanes(r.order()));
        }
        Ok(())
    }

    pub(super) fn check_vector(&self, vector: &[Complex<f32>]) -> Result<(), BeamError> {
        if vector.len() != self.n {
            return Err(BeamError::Lanes(vector.len()));
        }
        Ok(())
    }
}

pub(super) fn check_loading(loading: f32) -> Result<(), BeamError> {
    if loading.is_finite() && loading >= 0.0 {
        Ok(())
    } else {
        Err(BeamError::Setting("loading"))
    }
}

pub(super) fn check_references(
    n: usize,
    primary: usize,
    references: &[usize],
) -> Result<(), BeamError> {
    let fits = !references.is_empty()
        && references.len() < n
        && primary < n
        && references.iter().enumerate().all(|(index, &lane)| {
            lane < n && lane != primary && !references[..index].contains(&lane)
        });
    if fits {
        Ok(())
    } else {
        Err(BeamError::Lanes(references.len() + 1))
    }
}
