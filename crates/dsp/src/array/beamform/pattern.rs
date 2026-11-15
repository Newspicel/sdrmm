use super::{BeamError, WeightSet, ZERO};
use crate::array::manifold::{Manifold, SteeringGrid};
use crate::math::linalg::MAX_ORDER;

/// Relative frequency change below which the grid's own steering vectors are reused.
const SAME_FREQ_TOLERANCE: f64 = 1e-9;

/// Beam power over `ring`, normalised to its peak.
pub fn pattern(
    manifold: &Manifold,
    freq_hz: f64,
    weights: &WeightSet,
    ring: &SteeringGrid,
    out: &mut [f32],
) -> Result<(), BeamError> {
    let n = weights.len();
    if manifold.len() != n || ring.elements() != n {
        return Err(BeamError::Lanes(manifold.len()));
    }
    if out.len() != ring.points() {
        return Err(BeamError::Setting("pattern length"));
    }
    if !(freq_hz.is_finite() && freq_hz > 0.0) {
        return Err(BeamError::Setting("frequency"));
    }
    let same = (ring.freq_hz() - freq_hz).abs() <= SAME_FREQ_TOLERANCE * freq_hz;
    let mut a = [ZERO; MAX_ORDER];
    let mut peak = 0.0f32;
    for (point, slot) in out.iter_mut().enumerate() {
        *slot = if same {
            weights.response(ring.vector(point)).norm_sqr()
        } else {
            manifold.steer(freq_hz, ring.direction(point), &mut a[..n]);
            weights.response(&a[..n]).norm_sqr()
        };
        peak = peak.max(*slot);
    }
    if peak.is_finite() && peak > 0.0 {
        let scale = peak.recip();
        for value in out.iter_mut() {
            *value *= scale;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;

    use super::*;
    use crate::array::beamform::testing::{FREQ, kraken, steering};
    use crate::array::beamform::{BlockSolver, Constraints};
    use crate::array::manifold::GridSpec;
    use crate::math::db::Decibels;

    #[test]
    fn pattern_is_unity_at_the_steer_and_deep_at_a_null() {
        let manifold = kraken();
        let ring = SteeringGrid::new(&manifold, GridSpec::ring(1.0), FREQ).unwrap();
        let mut solver = BlockSolver::new(5).unwrap();
        let mut weights = WeightSet::zeros(5);
        solver
            .das(&steering(&manifold, 90.0), &mut weights)
            .unwrap();
        let mut out = [0.0f32; 360];
        pattern(&manifold, FREQ, &weights, &ring, &mut out).unwrap();
        let top = out
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index);
        assert_eq!(top, Some(90));
        assert!((out[90] - 1.0).abs() < 1e-6);
        let moved = SteeringGrid::new(&manifold, GridSpec::ring(1.0), FREQ * 1.01).unwrap();
        pattern(&manifold, FREQ, &weights, &moved, &mut out).unwrap();
        assert!((out[90] - 1.0).abs() < 1e-6);

        let mut constraints = Constraints::new(5);
        constraints
            .push(&steering(&manifold, 90.0), Complex::new(1.0, 0.0))
            .unwrap();
        constraints
            .push(&steering(&manifold, 200.0), Complex::new(0.0, 0.0))
            .unwrap();
        solver.lcmv(None, &constraints, 0.0, &mut weights).unwrap();
        pattern(&manifold, FREQ, &weights, &ring, &mut out).unwrap();
        assert!(out[200].power_to_db() < -40.0, "{}", out[200]);
        assert!(out[90] > 0.1);
        assert_eq!(
            pattern(&manifold, FREQ, &weights, &ring, &mut [0.0; 10]),
            Err(BeamError::Setting("pattern length"))
        );
    }
}
