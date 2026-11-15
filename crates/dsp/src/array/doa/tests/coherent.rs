use super::*;

fn coherent(geometry: &Geometry, first: f64, second: f64, seed: u64) -> ArrayScene {
    let direct = SceneSource::new(
        Direction::horizon(first),
        0.0,
        SceneSignal::Tone { offset_hz: 2e3 },
    );
    let bounce = SceneSource {
        direction: Direction::horizon(second),
        copy_of: Some(SceneCopy {
            source: 0,
            amplitude: 0.9,
            phase_deg: 60.0,
        }),
        ..direct
    };
    ArrayScene::new(geometry.clone(), FREQ, 1e5)
        .with_source(direct)
        .with_source(bounce)
        .with_noise_db(-25.0)
        .with_seed(seed)
}

#[test]
fn smoothing_separates_coherent_sources_on_a_ula() {
    let geometry = ula(8, 180.0);
    let r = covariance(&mut coherent(&geometry, 70.0, 100.0, 65), 8192);
    let raw = estimate(&geometry, &DoaConfig::default(), &r);
    assert_eq!(raw.sources, 1);
    let smoothed = DoaConfig {
        smoothing: 2,
        forward_backward: true,
        ..DoaConfig::default()
    };
    let report = estimate(&geometry, &smoothed, &r);
    assert_eq!(report.order, 6);
    assert_eq!(report.peak_count, 2, "{report:?}");
    let found = sorted_azimuths(&report);
    assert!(error_deg(found[0], 70.0) < 2.0, "{found:?}");
    assert!(error_deg(found[1], 100.0) < 2.0, "{found:?}");
    let rooted = DoaConfig {
        estimator: Estimator::RootMusic,
        ..smoothed
    };
    let found = sorted_azimuths(&estimate(&geometry, &rooted, &r));
    assert!(error_deg(found[0], 70.0) < 2.0, "{found:?}");
    assert!(error_deg(found[1], 100.0) < 2.0, "{found:?}");
}

#[test]
fn smoothing_separates_coherent_sources_on_a_uca() {
    let geometry = uca_for_beta(8, 2.0, FREQ);
    let r = covariance(&mut coherent(&geometry, 40.0, 150.0, 67), 8192);
    let smoothed = DoaConfig {
        smoothing: 2,
        forward_backward: true,
        ..DoaConfig::default()
    };
    let report = estimate(&geometry, &smoothed, &r);
    assert_eq!(report.order, 5);
    assert_eq!(report.peak_count, 2, "{report:?}");
    let found = sorted_azimuths(&report);
    assert!(error_deg(found[0], 40.0) < 2.0, "{found:?}");
    assert!(error_deg(found[1], 150.0) < 2.0, "{found:?}");
    let esprit = DoaConfig {
        estimator: Estimator::Esprit,
        ..smoothed
    };
    let found = sorted_azimuths(&estimate(&geometry, &esprit, &r));
    assert!(error_deg(found[0], 40.0) < 2.0, "{found:?}");
    assert!(error_deg(found[1], 150.0) < 2.0, "{found:?}");
}

#[test]
fn forward_backward_on_an_even_uca_keeps_the_bearing() {
    let geometry = uca_for_chord(6, 0.4, FREQ);
    let r = covariance(&mut scene(&geometry, &[(40.0, 10.0)], 69), SNAPSHOTS);
    let config = DoaConfig {
        forward_backward: true,
        ..DoaConfig::default()
    };
    let found = primary(&estimate(&geometry, &config, &r)).azimuth_deg;
    assert!(error_deg(found, 40.0) < 1.0, "{found}");
    assert!(matches!(
        Doa::new(&Manifold::ideal(half_wave_uca5()), &config, FREQ),
        Err(DoaError::Unsupported(FB_NEEDS_SYMMETRY))
    ));
}

#[test]
fn a_two_element_circle_runs_as_a_line() {
    let pair = Manifold::ideal(Geometry::uca(0.2, 2, 90.0, Winding::Clockwise).unwrap());
    let doa = Doa::new(&pair, &with(Estimator::RootMusic), FREQ).unwrap();
    assert_eq!(doa.mode_bias_deg(), None);
    assert!(Doa::new(&pair, &with(Estimator::Music), FREQ).is_ok());
}
