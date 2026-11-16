use super::{CovarianceError, forward_backward};
use crate::array::manifold::{MAX_ELEMENTS, Permutation};
use crate::math::linalg::{CMat, LinalgError};

pub fn smooth(
    r: &CMat,
    order: &Permutation,
    subarrays: usize,
    fb: bool,
    out: &mut CMat,
) -> Result<(), CovarianceError> {
    let n = r.order();
    if order.len() != n {
        return Err(LinalgError::Order(order.len()).into());
    }
    let len = subarray_len(n, subarrays).ok_or(CovarianceError::TooFewForSmoothing)?;
    let mut indices = [0usize; MAX_ELEMENTS];
    for (slot, index) in indices.iter_mut().zip(order.iter()) {
        *slot = index;
    }
    let mut along = CMat::zeros(n)?;
    r.permuted(&indices[..n], &mut along)?;
    out.resize(len)?;
    for start in 0..subarrays {
        for i in 0..len {
            for j in 0..len {
                out.add(i, j, along.get(start + i, start + j));
            }
        }
    }
    out.scale(1.0 / subarrays as f32);
    if fb {
        forward_backward(out, &Permutation::reversal(len))?;
    }
    Ok(())
}

pub fn smooth_diagonal(d: &[f32], subarrays: usize, fb: bool, out: &mut [f32]) -> usize {
    let Some(len) = subarray_len(d.len(), subarrays).filter(|&len| out.len() >= len) else {
        return 0;
    };
    for (i, value) in out[..len].iter_mut().enumerate() {
        *value = d[i..i + subarrays].iter().sum::<f32>() / subarrays as f32;
    }
    if fb {
        for i in 0..len / 2 {
            let mean = f32::midpoint(out[i], out[len - 1 - i]);
            out[i] = mean;
            out[len - 1 - i] = mean;
        }
    }
    len
}

fn subarray_len(elements: usize, subarrays: usize) -> Option<usize> {
    if subarrays == 0 || subarrays > elements {
        return None;
    }
    let len = elements - subarrays + 1;
    (len >= 2).then_some(len)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::covariance::testing::{
        FREQ, covariance_of, line_spectrum, peaks, sharpness, signal_count,
    };
    use crate::array::manifold::{Direction, Geometry, Manifold, wavelength_m};
    use crate::math::special::norm_deg;
    use crate::scene::{ArrayScene, SceneCopy, SceneSignal, SceneSource};

    #[test]
    fn smoothing_resolves_coherent_multipath_on_a_ula() {
        let geometry = Geometry::ula(wavelength_m(FREQ) / 2.0, 8, 180.0).unwrap();
        let direct = SceneSource::new(
            Direction::horizon(70.0),
            0.0,
            SceneSignal::Tone { offset_hz: 2e3 },
        );
        let bounce = SceneSource {
            direction: Direction::horizon(100.0),
            copy_of: Some(SceneCopy {
                source: 0,
                amplitude: 0.9,
                phase_deg: 60.0,
            }),
            ..direct
        };
        let mut scene = ArrayScene::new(geometry.clone(), FREQ, 1e5)
            .with_source(direct)
            .with_source(bounce)
            .with_noise_db(-25.0)
            .with_seed(4);
        let r = covariance_of(&scene.render(8192).unwrap());
        assert_eq!(signal_count(&r), 1);
        let raw = line_spectrum(&r, 1, geometry.positions());
        assert!(sharpness(&raw) < 10.0, "{}", sharpness(&raw));

        let order = geometry.axis_order().unwrap();
        let mut smoothed = CMat::zeros(1).unwrap();
        smooth(&r, &order, 3, true, &mut smoothed).unwrap();
        assert_eq!(smoothed.order(), 6);
        assert_eq!(signal_count(&smoothed), 2);
        let sub: Vec<_> = order
            .iter()
            .take(6)
            .map(|i| geometry.positions()[i])
            .collect();
        let resolved = line_spectrum(&smoothed, 2, &sub);
        assert!(sharpness(&resolved) > 1e3, "{}", sharpness(&resolved));
        let found = peaks(&resolved, 0.01);
        assert_eq!(found.len(), 2, "{found:?}");
        let mut found: Vec<f64> = found.iter().map(|&k| norm_deg(k as f64)).collect();
        found.sort_by(f64::total_cmp);
        assert!((found[0] - 70.0).abs() < 2.0, "{found:?}");
        assert!((found[1] - 100.0).abs() < 2.0, "{found:?}");
        let manifold = Manifold::ideal(geometry);
        assert_eq!(manifold.len(), 8);
    }

    #[test]
    fn smoothing_needs_two_elements_per_subarray() {
        let r = CMat::identity(4).unwrap();
        let mut out = CMat::zeros(1).unwrap();
        let order = Permutation::identity(4);
        assert_eq!(
            smooth(&r, &order, 4, false, &mut out),
            Err(CovarianceError::TooFewForSmoothing)
        );
        assert_eq!(
            smooth(&r, &order, 0, false, &mut out),
            Err(CovarianceError::TooFewForSmoothing)
        );
        smooth(&r, &order, 3, false, &mut out).unwrap();
        assert_eq!(out, CMat::identity(2).unwrap());
        let mut diagonal = [0.0f32; 4];
        assert_eq!(
            smooth_diagonal(&[1.0, 2.0, 3.0, 6.0], 2, false, &mut diagonal),
            3
        );
        assert_eq!(&diagonal[..3], &[1.5, 2.5, 4.5]);
        assert_eq!(
            smooth_diagonal(&[1.0, 2.0, 3.0, 6.0], 2, true, &mut diagonal),
            3
        );
        assert_eq!(&diagonal[..3], &[3.0, 2.5, 3.0]);
        assert_eq!(smooth_diagonal(&[1.0, 2.0], 2, false, &mut diagonal), 0);
    }
}
