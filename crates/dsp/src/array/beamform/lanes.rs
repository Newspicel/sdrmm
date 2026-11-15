use num_complex::Complex;

use super::BeamError;
use crate::math::linalg::MAX_ORDER;

/// Copies sample `t` of every lane into `out` and returns its energy.
pub(super) fn snapshot(
    lanes: &[&[Complex<f32>]],
    t: usize,
    out: &mut [Complex<f32>; MAX_ORDER],
) -> f32 {
    let mut energy = 0.0f32;
    for (slot, lane) in out.iter_mut().zip(lanes) {
        *slot = lane[t];
        energy += slot.norm_sqr();
    }
    energy
}

pub(super) fn common_len(lanes: &[&[Complex<f32>]], n: usize) -> Result<usize, BeamError> {
    if lanes.len() != n {
        return Err(BeamError::Lanes(lanes.len()));
    }
    equal_len(lanes.iter().map(|lane| lane.len()))
}

pub(super) fn equal_len(mut lengths: impl Iterator<Item = usize>) -> Result<usize, BeamError> {
    let first = lengths.next().unwrap_or(0);
    if lengths.all(|len| len == first) {
        Ok(first)
    } else {
        Err(BeamError::LaneLength)
    }
}
