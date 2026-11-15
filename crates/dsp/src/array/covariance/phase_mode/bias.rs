use super::{C64, PhaseMode};
use crate::array::covariance::CovarianceError;
use crate::array::manifold::MAX_ELEMENTS;
use crate::math::linalg::LinalgError;
use crate::math::special::wrap_deg;

const BIAS_BEARINGS: usize = 36;

impl PhaseMode {
    /// Worst root-MUSIC bearing error over a ring of test bearings, from the modes the
    /// element count cannot carry.
    pub(super) fn measure_bias(&mut self, beta: f64) -> Result<f32, CovarianceError> {
        let mut worst = 0.0f64;
        for step in 0..BIAS_BEARINGS {
            let bearing = 360.0 * step as f64 / BIAS_BEARINGS as f64;
            if let Some(found) = self.root_music_bearing(beta, bearing)? {
                worst = worst.max(wrap_deg(found - bearing).abs());
            }
        }
        Ok(worst as f32)
    }

    fn root_music_bearing(
        &mut self,
        beta: f64,
        bearing: f64,
    ) -> Result<Option<f64>, CovarianceError> {
        let n = self.elements;
        let m = self.virtual_len();
        if m < 2 {
            return Ok(None);
        }
        let theta = bearing.to_radians();
        let mut steering = [C64::new(0.0, 0.0); MAX_ELEMENTS];
        for (value, angle) in steering.iter_mut().zip(&self.angles_rad[..n]) {
            *value = C64::from_polar(1.0, beta * (theta - angle).cos());
        }
        let mut virtual_ = [C64::new(0.0, 0.0); MAX_ELEMENTS];
        for (p, value) in virtual_.iter_mut().enumerate().take(m) {
            *value = (0..n).map(|i| self.unit[p * n + i] * steering[i]).sum();
        }
        let power: f64 = virtual_[..m].iter().map(C64::norm_sqr).sum();
        if power <= 0.0 {
            return Ok(None);
        }
        self.coeffs.fill(C64::new(0.0, 0.0));
        for p in 0..m {
            for q in 0..m {
                let identity = if p == q { 1.0 } else { 0.0 };
                let projector = identity - virtual_[p] * virtual_[q].conj() / power;
                let weight = f64::from(self.magnitudes[p]) * f64::from(self.magnitudes[q]);
                self.coeffs[q + m - 1 - p] += projector * weight;
            }
        }
        let degree = match self.roots.solve(&self.coeffs[..2 * m - 1], &mut self.found) {
            Ok(degree) => degree,
            Err(LinalgError::Degenerate) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        Ok(self.found[..degree]
            .iter()
            .min_by(|a, b| (a.norm() - 1.0).abs().total_cmp(&(b.norm() - 1.0).abs()))
            .map(|root| root.arg().to_degrees()))
    }
}
