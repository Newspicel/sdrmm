use crate::{
    array::beamform::WeightSet,
    math::linalg::{CMat, Eigen, HermitianEigen, LinalgError},
};

/// Weights that match a two-feed covariance's dominant polarisation, or the one orthogonal
/// to it. They keep the phase of `out` so a steady signal does not make them turn.
pub fn matched_weights(
    r: &CMat,
    orthogonal: bool,
    eigen: &mut HermitianEigen,
    values: &mut Eigen,
    out: &mut WeightSet,
) -> Result<(), LinalgError> {
    if r.order() != 2 {
        return Err(LinalgError::Order(r.order()));
    }
    eigen.solve(r, values)?;
    let index = usize::from(!orthogonal);
    let mut weights = WeightSet::from_textbook(values.vector(index));
    if !weights.is_finite() {
        return Err(LinalgError::NonFinite);
    }
    weights.align_phase_to(out);
    *out = weights;
    Ok(())
}
