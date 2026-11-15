use num_complex::Complex;

use super::check_loading;
use super::{BlockSolver, OUT};
use crate::array::beamform::{BeamError, Constraints, MAX_CONSTRAINTS, WeightSet, ZERO};
use crate::math::linalg::{CMat, LinalgError, inner};

impl BlockSolver {
    pub fn mvdr(
        &mut self,
        r: &CMat,
        steer: &[Complex<f32>],
        loading: f32,
        out: &mut WeightSet,
    ) -> Result<f32, BeamError> {
        self.check_matrix(r)?;
        self.check_vector(steer)?;
        check_loading(loading)?;
        let n = self.n;
        let delta = self.chol.factor_loaded(r, loading)?;
        let y = &mut self.scratch[OUT][..n];
        self.chol.solve_into(steer, y)?;
        let gain: Complex<f32> = steer.iter().zip(y.iter()).map(|(a, v)| a.conj() * v).sum();
        if !(gain.re.is_finite() && gain.re > 0.0) {
            return Err(LinalgError::NotPositiveDefinite(0).into());
        }
        let scale = gain.re.recip();
        for value in y.iter_mut() {
            *value *= scale;
        }
        *out = WeightSet::from_textbook(y);
        Ok(delta)
    }

    pub fn lcmv(
        &mut self,
        r: Option<&CMat>,
        constraints: &Constraints,
        loading: f32,
        out: &mut WeightSet,
    ) -> Result<f32, BeamError> {
        let n = self.n;
        if constraints.order() != n {
            return Err(BeamError::Lanes(constraints.order()));
        }
        let count = constraints.count();
        if count == 0 {
            return Err(LinalgError::Order(0).into());
        }
        let delta = match r {
            Some(r) => {
                self.check_matrix(r)?;
                check_loading(loading)?;
                Some(self.chol.factor_loaded(r, loading)?)
            }
            None => None,
        };
        for k in 0..count {
            let column = &mut self.scratch[k][..n];
            match delta {
                Some(_) => self.chol.solve_into(constraints.vector(k), column)?,
                None => column.copy_from_slice(constraints.vector(k)),
            }
        }
        self.work.resize(count)?;
        for row in 0..count {
            for col in 0..count {
                let value = inner(constraints.vector(row), &self.scratch[col][..n]);
                self.work.set(row, col, value);
            }
        }
        self.small.factor_cmat(&self.work)?;
        let mut g = [ZERO; MAX_CONSTRAINTS];
        for (k, value) in g.iter_mut().enumerate().take(count) {
            *value = constraints.response(k).conj();
        }
        self.small.solve(&mut g[..count])?;
        let (columns, rest) = self.scratch.split_at_mut(OUT);
        let textbook = &mut rest[0][..n];
        textbook.fill(ZERO);
        for (column, &weight) in columns.iter().zip(&g).take(count) {
            for (value, y) in textbook.iter_mut().zip(&column[..n]) {
                *value += y * weight;
            }
        }
        if !textbook.iter().all(|value| value.is_finite()) {
            return Err(LinalgError::NonFinite.into());
        }
        *out = WeightSet::from_textbook(textbook);
        Ok(delta.unwrap_or(0.0))
    }
}
