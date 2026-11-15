use super::*;

#[test]
fn configure_scalar_changes_in_place() {
    let geometry = ula(6, 90.0);
    let manifold = Manifold::ideal(geometry.clone());
    let r = covariance(&mut scene(&geometry, &[(20.0, 20.0)], 61), SNAPSHOTS);
    let mut doa = Doa::new(&manifold, &DoaConfig::default(), FREQ).unwrap();
    for estimator in [
        Estimator::Capon,
        Estimator::RootMusic,
        Estimator::Esprit,
        Estimator::Bartlett,
    ] {
        assert_eq!(
            doa.configure(&manifold, &with(estimator)),
            Ok(Reconfigure::InPlace)
        );
        let mut report = DoaReport::default();
        doa.estimate(&manifold, &r, SNAPSHOTS as f64, 3.0, &mut report)
            .unwrap();
        assert!(
            error_deg(primary(&report).azimuth_deg, 20.0) < 1.5,
            "{estimator:?}"
        );
        assert_eq!(doa.config().estimator, estimator);
    }
    let coarse = DoaConfig {
        azimuth_step_deg: 2.0,
        ..DoaConfig::default()
    };
    assert_eq!(doa.configure(&manifold, &coarse), Ok(Reconfigure::NeedsNew));
    let front = DoaConfig {
        ula_side: UlaSide::Front,
        ..DoaConfig::default()
    };
    assert_eq!(doa.configure(&manifold, &front), Ok(Reconfigure::NeedsNew));
    let mut half = Doa::new(&manifold, &front, FREQ).unwrap();
    let back = DoaConfig {
        ula_side: UlaSide::Back,
        ..front
    };
    assert_eq!(half.configure(&manifold, &back), Ok(Reconfigure::InPlace));
    let mut report = DoaReport::default();
    half.estimate(&manifold, &r, SNAPSHOTS as f64, 3.0, &mut report)
        .unwrap();
    assert!(
        error_deg(primary(&report).azimuth_deg, 160.0) < 1.0,
        "{report:?}"
    );
    let refused = DoaConfig {
        max_peaks: 9,
        ..DoaConfig::default()
    };
    assert_eq!(
        half.configure(&manifold, &refused),
        Err(DoaError::Unsupported(PEAKS_OUT_OF_RANGE))
    );
    assert_eq!(half.config().ula_side, UlaSide::Back);
}

#[test]
fn retune_rebuilds_the_grid_and_keeps_bearings() {
    let geometry = half_wave_uca5();
    let manifold = Manifold::ideal(geometry.clone());
    let mut doa = Doa::new(&manifold, &DoaConfig::default(), FREQ).unwrap();
    let alias = doa.retune(&manifold, 2.0 * FREQ).unwrap();
    assert!(alias.aliased);
    let shifted = 400e6;
    let alias = doa.retune(&manifold, shifted).unwrap();
    assert!(!alias.aliased);
    assert_eq!(doa.freq_hz(), shifted);
    let mut moved = ArrayScene::new(geometry, shifted, RATE)
        .with_noise_db(0.0)
        .with_seed(63)
        .with_source(broadband(Direction::horizon(250.0), 15.0));
    let r = covariance(&mut moved, SNAPSHOTS);
    let mut report = DoaReport::default();
    doa.estimate(&manifold, &r, SNAPSHOTS as f64, 3.0, &mut report)
        .unwrap();
    assert!(error_deg(primary(&report).azimuth_deg, 250.0) < 1.0);
    assert!(matches!(
        doa.retune(&manifold, f64::NAN),
        Err(DoaError::Manifold(ManifoldError::Frequency))
    ));
    let other = Manifold::ideal(uca_for_chord(5, 0.4, FREQ));
    assert_eq!(
        doa.configure(&other, &DoaConfig::default()),
        Ok(Reconfigure::NeedsNew)
    );
    assert_eq!(
        doa.retune(&other, FREQ),
        Err(DoaError::Unsupported(ARRAY_CHANGED))
    );
    assert_eq!(
        doa.estimate(&other, &r, SNAPSHOTS as f64, 3.0, &mut report),
        Err(DoaError::Unsupported(ARRAY_CHANGED))
    );
    let table = scene(manifold.geometry(), &[], 1)
        .distortion_table(&[FREQ], 5.0)
        .unwrap();
    let measured = Manifold::measured(manifold.geometry().clone(), Arc::new(table)).unwrap();
    assert_eq!(
        doa.configure(&measured, &DoaConfig::default()),
        Ok(Reconfigure::NeedsNew)
    );
}

#[test]
fn a_wrong_or_empty_matrix_is_an_error() {
    let manifold = Manifold::ideal(half_wave_uca5());
    let mut doa = Doa::new(&manifold, &DoaConfig::default(), FREQ).unwrap();
    let mut report = DoaReport::default();
    let small = CMat::identity(4).unwrap();
    assert_eq!(
        doa.estimate(&manifold, &small, 10.0, 3.0, &mut report),
        Err(DoaError::Linalg(LinalgError::Order(4)))
    );
    let empty = CMat::zeros(5).unwrap();
    assert_eq!(
        doa.estimate(&manifold, &empty, 10.0, 3.0, &mut report),
        Err(DoaError::Linalg(LinalgError::NotPositiveDefinite(0)))
    );
    let mut broken = CMat::identity(5).unwrap();
    broken.set(1, 2, Complex::new(f32::NAN, 0.0));
    assert_eq!(
        doa.estimate(&manifold, &broken, 10.0, 3.0, &mut report),
        Err(DoaError::Linalg(LinalgError::NonFinite))
    );
}
