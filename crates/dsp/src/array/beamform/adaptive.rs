use num_complex::Complex;

use super::{BeamError, ZERO};
use crate::math::linalg::MAX_ORDER;

/// Smoothing of the running input power that scales the NLMS regulariser.
pub(super) const POWER_EMA: f32 = 1e-3;
/// Share of the running input power added to the NLMS denominator.
pub(super) const REGULARISE: f32 = 1e-6;

const MAX_STEP: f32 = 2.0;

/// A filter that adapts once per snapshot and falls back to its start when it diverges.
pub(super) trait Adaptive {
    /// Output for sample `t`, or `None` when the weights blew up. `x` is scratch for the
    /// snapshot.
    fn adapt(
        &mut self,
        lanes: &[&[Complex<f32>]],
        t: usize,
        x: &mut [Complex<f32>; MAX_ORDER],
    ) -> Option<Complex<f32>>;

    fn restart(&mut self);

    fn count_reset(&mut self);
}

/// Runs `filter` over `len` samples. A divergence writes zero, restarts the filter and is
/// counted once per block.
pub(super) fn run(
    filter: &mut impl Adaptive,
    lanes: &[&[Complex<f32>]],
    len: usize,
    out: &mut Vec<Complex<f32>>,
) -> Result<(), BeamError> {
    let mut x = [ZERO; MAX_ORDER];
    let mut diverged = false;
    for t in 0..len {
        if let Some(y) = filter.adapt(lanes, t, &mut x) {
            out.push(y);
            continue;
        }
        out.push(ZERO);
        filter.restart();
        if !diverged {
            diverged = true;
            filter.count_reset();
        }
    }
    if diverged {
        Err(BeamError::Diverged)
    } else {
        Ok(())
    }
}

pub(super) fn check_step(step: f32) -> Result<(), BeamError> {
    if step.is_finite() && step > 0.0 && step < MAX_STEP {
        Ok(())
    } else {
        Err(BeamError::Setting("step"))
    }
}

/// One NLMS step `h += u * conj(y) * step / denominator`. False when the weight norm leaves
/// `limit` or stops being finite.
pub(super) fn nlms_update(
    h: &mut [Complex<f32>],
    u: &[Complex<f32>],
    y: Complex<f32>,
    step: f32,
    denominator: f32,
    limit: f32,
) -> bool {
    let gain = y.conj() * (step / denominator);
    let mut norm = 0.0f32;
    for (h, u) in h.iter_mut().zip(u) {
        *h += u * gain;
        norm += h.norm_sqr();
    }
    norm.is_finite() && norm <= limit * limit
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_must_sit_inside_the_stable_range() {
        assert!(check_step(0.5).is_ok());
        for bad in [0.0, -0.1, 2.0, f32::NAN] {
            assert_eq!(check_step(bad), Err(BeamError::Setting("step")));
        }
    }

    #[test]
    fn an_nlms_step_moves_toward_the_error_and_flags_a_blow_up() {
        let mut h = [ZERO; 2];
        let u = [Complex::new(1.0, 0.0), Complex::new(0.0, 1.0)];
        assert!(nlms_update(
            &mut h,
            &u,
            Complex::new(2.0, 0.0),
            0.5,
            2.0,
            10.0
        ));
        assert_eq!(h, [Complex::new(0.5, 0.0), Complex::new(0.0, 0.5)]);
        assert!(!nlms_update(
            &mut h,
            &u,
            Complex::new(1e6, 0.0),
            1.0,
            1.0,
            10.0
        ));
    }
}
