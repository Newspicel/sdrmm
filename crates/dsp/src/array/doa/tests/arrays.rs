use super::*;

#[test]
fn elevation_is_found_on_a_raised_centre_element() {
    let ring = uca_for_chord(5, 0.5, FREQ);
    let mut positions: Vec<Vec3> = ring.positions().to_vec();
    positions.push(Vec3::new(0.0, 0.0, 0.3 * wavelength_m(FREQ)));
    let geometry = Geometry::explicit(&positions).unwrap();
    let mut raised = ArrayScene::new(geometry.clone(), FREQ, RATE)
        .with_noise_db(0.0)
        .with_seed(57)
        .with_source(broadband(Direction::new(60.0, 30.0), 20.0));
    let r = covariance(&mut raised, SNAPSHOTS);
    let config = DoaConfig {
        elevation: Some(ElevationSpan::default()),
        ..DoaConfig::default()
    };
    let peak = primary(&estimate(&geometry, &config, &r));
    assert!(error_deg(peak.azimuth_deg, 60.0) < 2.0, "{peak:?}");
    assert!((peak.elevation_deg - 30.0).abs() < 2.0, "{peak:?}");
    assert!(peak.sigma_el_deg > 0.0);
}

#[test]
fn elevation_is_refused_on_a_line() {
    let manifold = Manifold::ideal(ula(4, 90.0));
    let config = DoaConfig {
        elevation: Some(ElevationSpan::default()),
        ..DoaConfig::default()
    };
    assert!(matches!(
        Doa::new(&manifold, &config, FREQ),
        Err(DoaError::Unsupported(ELEVATION_NEEDS_2D))
    ));
}

fn warp(element: usize, azimuth_deg: f64) -> Complex<f32> {
    let phase = 0.9 * (2.0 * azimuth_deg.to_radians() + 1.3 * element as f64).sin();
    Complex::from_polar(1.0, phase as f32)
}

#[test]
fn measured_table_corrects_a_distorted_array() {
    let geometry = half_wave_uca5();
    let mut warped = scene(&geometry, &[(100.0, 20.0)], 59);
    warped.distortion = Some(warp);
    let r = covariance(&mut warped, SNAPSHOTS);
    let ideal = primary(&estimate(&geometry, &DoaConfig::default(), &r));
    assert!(error_deg(ideal.azimuth_deg, 100.0) > 5.0, "{ideal:?}");
    let table = warped.distortion_table(&[FREQ], 1.0).unwrap();
    let manifold = Manifold::measured(geometry, Arc::new(table)).unwrap();
    let measured = primary(&estimate_with(
        &manifold,
        &DoaConfig::default(),
        &r,
        SNAPSHOTS,
        NO_CAL_SIGMA_DEG,
    ));
    assert!(error_deg(measured.azimuth_deg, 100.0) < 1.0, "{measured:?}");
}

#[test]
fn a_measured_table_refuses_what_would_ignore_it() {
    let geometry = half_wave_uca5();
    let mut warped = scene(&geometry, &[(100.0, 20.0)], 59);
    warped.distortion = Some(warp);
    let table = Arc::new(
        warped
            .distortion_table(&[FREQ - 1e6, FREQ + 1e6], 1.0)
            .unwrap(),
    );
    let manifold = Manifold::measured(geometry, table).unwrap();
    for config in [
        with(Estimator::RootMusic),
        with(Estimator::Esprit),
        DoaConfig {
            smoothing: 1,
            ..DoaConfig::default()
        },
    ] {
        assert_eq!(
            Doa::new(&manifold, &config, FREQ).err(),
            Some(DoaError::Unsupported(TABLE_NEEDS_GRID)),
            "{config:?}"
        );
    }
    let mut doa = Doa::new(&manifold, &DoaConfig::default(), FREQ).unwrap();
    assert_eq!(
        doa.configure(&manifold, &with(Estimator::RootMusic)),
        Err(DoaError::Unsupported(TABLE_NEEDS_GRID))
    );
    let mut outside = Doa::new(&manifold, &with(Estimator::RootMusic), 2.0 * FREQ).unwrap();
    assert_eq!(
        outside.retune(&manifold, FREQ),
        Err(DoaError::Unsupported(TABLE_NEEDS_GRID))
    );
    assert!(outside.retune(&manifold, 2.0 * FREQ).is_ok());
    let square = Geometry::uca(0.2, 4, 0.0, Winding::Clockwise).unwrap();
    let mut flat = scene(&square, &[(100.0, 20.0)], 60);
    flat.distortion = Some(warp);
    let table = Arc::new(flat.distortion_table(&[FREQ], 1.0).unwrap());
    let measured = Manifold::measured(square.clone(), table).unwrap();
    let fb = DoaConfig {
        forward_backward: true,
        ..DoaConfig::default()
    };
    assert!(Doa::new(&Manifold::ideal(square), &fb, FREQ).is_ok());
    assert_eq!(
        Doa::new(&measured, &fb, FREQ).err(),
        Some(DoaError::Unsupported(FB_NEEDS_SYMMETRY))
    );
}

#[test]
fn a_circle_too_wide_for_phase_modes_keeps_the_grid_methods() {
    let geometry = uca_for_beta(16, 70.0, FREQ);
    let manifold = Manifold::ideal(geometry.clone());
    let r = covariance(&mut scene(&geometry, &[(137.0, 20.0)], 61), SNAPSHOTS);
    let found = primary(&estimate(&geometry, &with(Estimator::Music), &r));
    assert!(error_deg(found.azimuth_deg, 137.0) < 1.0, "{found:?}");
    assert!(matches!(
        Doa::new(&manifold, &with(Estimator::RootMusic), FREQ).err(),
        Some(DoaError::Covariance(CovarianceError::Special(_)))
    ));
    let kraken = Manifold::ideal(half_wave_uca5());
    let far = 12e9;
    let mut music = Doa::new(&kraken, &with(Estimator::Music), FREQ).unwrap();
    assert!(music.retune(&kraken, far).is_ok());
    let mut root = Doa::new(&kraken, &with(Estimator::RootMusic), FREQ).unwrap();
    assert!(matches!(
        root.retune(&kraken, far),
        Err(DoaError::Covariance(CovarianceError::Special(_)))
    ));
    assert_eq!(root.mode_bias_deg(), None);
    assert!(root.retune(&kraken, FREQ).is_ok());
    assert!(root.mode_bias_deg().is_some());
}
