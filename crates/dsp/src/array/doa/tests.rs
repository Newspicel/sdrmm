use std::sync::Arc;

use num_complex::Complex;

use super::*;
use crate::array::covariance::SampleCovariance;
use crate::array::doa::testing::{FREQ, rates_at};
use crate::array::manifold::{
    Direction, ElevationSpan, Geometry, Vec3, Winding,
    testing::{half_wave_uca5, uca_for_beta, uca_for_chord},
    wavelength_m,
};
use crate::math::special::wrap_deg;
use crate::scene::{ArrayScene, SceneCopy, SceneSignal, SceneSource};

mod accuracy;
mod arrays;
mod coherent;
mod counting;
mod estimators;
mod lifecycle;
mod likelihood;

const RATE: f64 = 2.4e6;
const SNAPSHOTS: usize = 4096;
const NO_CAL_SIGMA_DEG: f32 = 5.831;

fn ula(count: usize, axis_deg: f64) -> Geometry {
    Geometry::ula(wavelength_m(FREQ) / 2.0, count, axis_deg).unwrap()
}

fn broadband(direction: Direction, power_db: f32) -> SceneSource {
    SceneSource::new(direction, power_db, SceneSignal::Broadband)
}

fn scene(geometry: &Geometry, sources: &[(f64, f32)], seed: u64) -> ArrayScene {
    sources.iter().fold(
        ArrayScene::new(geometry.clone(), FREQ, RATE)
            .with_noise_db(0.0)
            .with_seed(seed),
        |scene, &(azimuth, power_db)| {
            scene.with_source(broadband(Direction::horizon(azimuth), power_db))
        },
    )
}

fn covariance(scene: &mut ArrayScene, len: usize) -> CMat {
    let lanes = scene.render(len).unwrap();
    let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
    let mut sum = SampleCovariance::new(lanes.len()).unwrap();
    sum.accumulate(&views);
    let mut r = CMat::zeros(lanes.len()).unwrap();
    assert!(sum.matrix(&mut r));
    r
}

fn estimate_with(
    manifold: &Manifold,
    config: &DoaConfig,
    r: &CMat,
    snapshots: usize,
    cal_sigma_deg: f32,
) -> DoaReport {
    let mut doa = Doa::new(manifold, config, FREQ).unwrap();
    let mut report = DoaReport::default();
    doa.estimate(manifold, r, snapshots as f64, cal_sigma_deg, &mut report)
        .unwrap();
    report
}

fn estimate(geometry: &Geometry, config: &DoaConfig, r: &CMat) -> DoaReport {
    estimate_with(
        &Manifold::ideal(geometry.clone()),
        config,
        r,
        SNAPSHOTS,
        NO_CAL_SIGMA_DEG,
    )
}

fn with(estimator: Estimator) -> DoaConfig {
    DoaConfig {
        estimator,
        ..DoaConfig::default()
    }
}

fn error_deg(found: f64, want: f64) -> f64 {
    wrap_deg(found - want).abs()
}

fn primary(report: &DoaReport) -> Peak {
    assert!(report.peak_count > 0, "{report:?}");
    report.peaks[0]
}

fn sorted_azimuths(report: &DoaReport) -> Vec<f64> {
    let mut found: Vec<f64> = report.peaks().iter().map(|p| p.azimuth_deg).collect();
    found.sort_by(f64::total_cmp);
    found
}
