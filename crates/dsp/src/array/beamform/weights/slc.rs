use super::{BlockSolver, OUT, check_references};
use crate::array::beamform::{BeamError, WeightSet};
use crate::math::linalg::{CMat, LinalgError};

/// Diagonal loading of the reference covariance, as a share of its mean power.
const SLC_LOADING: f32 = 1e-5;

impl BlockSolver {
    /// Sidelobe canceller: the primary lane minus its least-squares fit from the references.
    pub fn slc(
        &mut self,
        r: &CMat,
        primary: usize,
        references: &[usize],
        out: &mut WeightSet,
    ) -> Result<f32, BeamError> {
        self.check_matrix(r)?;
        let n = self.n;
        check_references(n, primary, references)?;
        let m = references.len();
        self.work.resize(m)?;
        for (row, &from_row) in references.iter().enumerate() {
            for (col, &from_col) in references.iter().enumerate() {
                self.work.set(row, col, r.get(from_row, from_col));
            }
        }
        let delta = self.chol.factor_loaded(&self.work, SLC_LOADING)?;
        let z = &mut self.scratch[OUT][..m];
        for (value, &lane) in z.iter_mut().zip(references) {
            *value = r.get(lane, primary);
        }
        self.chol.solve(z)?;
        if !z.iter().all(|value| value.is_finite()) {
            return Err(LinalgError::NonFinite.into());
        }
        *out = WeightSet::unit(n, primary);
        let weights = out.as_mut_slice();
        for (&lane, value) in references.iter().zip(z.iter()) {
            weights[lane] = -value.conj();
        }
        Ok(delta)
    }
}
