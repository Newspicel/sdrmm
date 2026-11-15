use super::{BlockSolver, OUT};
use crate::array::beamform::{BeamError, WeightSet, ZERO};
use crate::math::linalg::{CMat, LinalgError, MAX_ORDER, norm_sqr};

/// Floor on a lane's noise, as a share of the mean, so one quiet lane cannot dominate.
const NOISE_FLOOR: f32 = 1e-3;

impl BlockSolver {
    /// Maximum ratio combining: the principal eigenvector of the noise-whitened covariance.
    pub fn mrc(
        &mut self,
        r: &CMat,
        noise: Option<&[f32]>,
        out: &mut WeightSet,
    ) -> Result<(), BeamError> {
        self.check_matrix(r)?;
        let n = self.n;
        let d = noise_diagonal(n, noise)?;
        self.work.resize(n)?;
        for row in 0..n {
            for col in 0..n {
                let scale = (d[row] * d[col]).sqrt().recip();
                self.work.set(row, col, r.get(row, col) * scale);
            }
        }
        self.eigen.solve(&self.work, &mut self.values)?;
        if self.values.values()[n - 1] <= 0.0 {
            return Err(LinalgError::RankDeficient(0).into());
        }
        let principal = self.values.vector(n - 1);
        let mut steer = [ZERO; MAX_ORDER];
        for ((value, v), weight) in steer.iter_mut().zip(principal).zip(&d) {
            *value = v * weight.sqrt();
        }
        let energy = norm_sqr(&steer[..n]);
        let whitened: f32 = steer[..n]
            .iter()
            .zip(&d)
            .map(|(a, weight)| a.norm_sqr() / weight)
            .sum();
        if !(energy.is_finite() && whitened.is_finite() && energy > 0.0 && whitened > 0.0) {
            return Err(LinalgError::NonFinite.into());
        }
        let norm = (n as f32 / energy).sqrt();
        let scale = 1.0 / (norm * whitened);
        let textbook = &mut self.scratch[OUT][..n];
        for ((value, a), weight) in textbook.iter_mut().zip(&steer).zip(&d) {
            *value = a * (scale / weight);
        }
        *out = WeightSet::from_textbook(textbook);
        Ok(())
    }
}

fn noise_diagonal(n: usize, noise: Option<&[f32]>) -> Result<[f32; MAX_ORDER], BeamError> {
    let mut d = [1.0f32; MAX_ORDER];
    let Some(noise) = noise else {
        return Ok(d);
    };
    if noise.len() != n {
        return Err(BeamError::Lanes(noise.len()));
    }
    if !noise.iter().all(|sigma| sigma.is_finite() && *sigma >= 0.0) {
        return Err(LinalgError::NonFinite.into());
    }
    let mean = noise.iter().sum::<f32>() / n as f32;
    if mean <= 0.0 {
        return Err(LinalgError::RankDeficient(0).into());
    }
    for (slot, &sigma) in d.iter_mut().zip(noise) {
        *slot = sigma.max(NOISE_FLOOR * mean);
    }
    Ok(d)
}
