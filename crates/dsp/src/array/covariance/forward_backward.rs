use super::CovarianceError;
use crate::array::manifold::Permutation;
use crate::math::linalg::{CMat, LinalgError};

pub fn forward_backward(r: &mut CMat, perm: &Permutation) -> Result<(), CovarianceError> {
    let n = r.order();
    if perm.len() != n {
        return Err(LinalgError::Order(perm.len()).into());
    }
    let original = r.clone();
    for i in 0..n {
        for j in 0..n {
            let mirrored = original.get(perm.get(i), perm.get(j)).conj();
            r.set(i, j, (original.get(i, j) + mirrored) * 0.5);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::covariance::testing::{FREQ, covariance_of, music_at};
    use crate::array::manifold::{Direction, Geometry, Winding, wavelength_m};
    use crate::math::db::Decibels;
    use crate::math::special::wrap_deg;
    use crate::scene::{ArrayScene, SceneSignal, SceneSource};
    use crate::testutil::uca5;

    #[test]
    fn forward_backward_keeps_an_even_uca_bearing_without_a_ghost() {
        let geometry = Geometry::uca(0.4 * wavelength_m(FREQ), 6, 0.0, Winding::Clockwise).unwrap();
        let mut scene = ArrayScene::new(geometry.clone(), FREQ, 1e6)
            .with_source(SceneSource::new(
                Direction::horizon(40.0),
                0.0,
                SceneSignal::Noise {
                    offset_hz: 1e4,
                    bandwidth_hz: 2e5,
                },
            ))
            .with_noise_db(-10.0)
            .with_seed(9);
        let mut r = covariance_of(&scene.render(4096).unwrap());
        let pairs = geometry.antipodes().unwrap();
        forward_backward(&mut r, &pairs).unwrap();
        let positions = geometry.positions();
        let spectrum: Vec<f32> = (0..3600)
            .map(|k| music_at(&r, 1, positions, k as f64 * 0.1))
            .collect();
        let best = spectrum
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(k, _)| k as f64 * 0.1)
            .unwrap();
        assert!((best - 40.0).abs() < 1.0, "peak at {best}");
        let ghost = music_at(&r, 1, positions, 220.0) / music_at(&r, 1, positions, 40.0);
        assert!(ghost.power_to_db() < -20.0, "ghost {ghost}");
        let peak = spectrum.iter().copied().fold(0.0f32, f32::max);
        let elsewhere = spectrum
            .iter()
            .enumerate()
            .filter(|&(k, _)| wrap_deg(k as f64 * 0.1 - 40.0).abs() > 15.0)
            .map(|(_, &power)| power)
            .fold(0.0f32, f32::max);
        assert!(
            (elsewhere / peak).power_to_db() < -20.0,
            "{elsewhere} vs {peak}"
        );
    }

    #[test]
    fn forward_backward_is_refused_for_an_odd_uca() {
        let odd = uca5(0.3);
        assert_eq!(odd.antipodes(), None);
        let mut r = CMat::identity(5).unwrap();
        assert_eq!(
            forward_backward(&mut r, &Permutation::identity(4)),
            Err(CovarianceError::Linalg(LinalgError::Order(4)))
        );
    }
}
