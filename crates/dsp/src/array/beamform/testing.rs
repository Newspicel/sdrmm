use num_complex::Complex;

use super::LaneNoise;
use crate::array::covariance::SampleCovariance;
use crate::array::manifold::{Direction, Manifold};
use crate::math::linalg::CMat;
use crate::scene::{ArrayScene, SceneSignal, SceneSource};
use crate::testutil;

pub(crate) const FREQ: f64 = 433.92e6;

pub(crate) fn kraken() -> Manifold {
    Manifold::ideal(testutil::kraken())
}

pub(crate) fn steering(manifold: &Manifold, azimuth: f64) -> Vec<Complex<f32>> {
    let mut out = vec![Complex::new(0.0, 0.0); manifold.len()];
    manifold.steer(FREQ, Direction::horizon(azimuth), &mut out);
    out
}

pub(crate) fn views(lanes: &[Vec<Complex<f32>>]) -> Vec<&[Complex<f32>]> {
    lanes.iter().map(Vec::as_slice).collect()
}

pub(crate) fn covariance(lanes: &[Vec<Complex<f32>>]) -> CMat {
    let mut estimator = SampleCovariance::new(lanes.len()).unwrap();
    estimator.accumulate(&views(lanes));
    let mut r = CMat::zeros(lanes.len()).unwrap();
    assert!(estimator.matrix(&mut r));
    r
}

pub(crate) fn combine(lanes: &[Vec<Complex<f32>>], weights: &[Complex<f32>]) -> Vec<Complex<f32>> {
    (0..lanes[0].len())
        .map(|t| lanes.iter().zip(weights).map(|(lane, w)| lane[t] * w).sum())
        .collect()
}

/// The kraken ring hearing one source at 137 degrees over per-lane noise.
pub(crate) fn noise_scene(signal: SceneSignal, power_db: f32, noise_db: &[f32]) -> ArrayScene {
    let mut scene = ArrayScene::new(kraken().geometry().clone(), FREQ, 1e6)
        .with_seed(3)
        .with_source(SceneSource::new(
            Direction::horizon(137.0),
            power_db,
            signal,
        ));
    scene.noise_db = noise_db.to_vec();
    scene
}

pub(crate) fn settle(scene: &mut ArrayScene, noise: &mut LaneNoise) -> Vec<Vec<Complex<f32>>> {
    let mut last = Vec::new();
    for _ in 0..20 {
        last = scene.render(2_048).unwrap();
        noise.push(&views(&last)).unwrap();
    }
    last
}

pub(crate) const TONE: SceneSignal = SceneSignal::Tone { offset_hz: 3.1e4 };
