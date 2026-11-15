use super::*;

#[test]
fn crb_sigma_matches_the_empirical_spread() {
    let geometry = half_wave_uca5();
    let manifold = Manifold::ideal(geometry.clone());
    let config = DoaConfig {
        sources: SourceCount::Fixed(1),
        squelch: None,
        ..DoaConfig::default()
    };
    let mut doa = Doa::new(&manifold, &config, FREQ).unwrap();
    let mut report = DoaReport::default();
    let snapshots = 1000;
    let errors: Vec<f64> = (0..200)
        .map(|trial| {
            let r = covariance(
                &mut scene(&geometry, &[(37.0, 0.0)], 1000 + trial),
                snapshots,
            );
            doa.estimate(&manifold, &r, snapshots as f64, 0.0, &mut report)
                .unwrap();
            wrap_deg(primary(&report).azimuth_deg - 37.0)
        })
        .collect();
    let mean = errors.iter().sum::<f64>() / errors.len() as f64;
    let spread = (errors.iter().map(|e| (e - mean) * (e - mean)).sum::<f64>()
        / (errors.len() - 1) as f64)
        .sqrt();
    let bound = crb_sigma_rad(&rates_at(&geometry, 37.0), 1.0, 5, snapshots as f64).to_degrees();
    let ratio = spread / bound;
    assert!((0.7..1.5).contains(&ratio), "{spread} vs {bound}");
}

#[test]
fn calibration_error_widens_sigma() {
    let geometry = half_wave_uca5();
    let r = covariance(&mut scene(&geometry, &[(37.0, 20.0)], 41), SNAPSHOTS);
    let manifold = Manifold::ideal(geometry.clone());
    let peak = primary(&estimate_with(
        &manifold,
        &DoaConfig::default(),
        &r,
        SNAPSHOTS,
        5.0,
    ));
    let spread = rate_spread(&rates_at(&geometry, peak.azimuth_deg));
    let model = (5f64.to_radians() / spread.sqrt()).to_degrees();
    assert!(
        f64::from(peak.sigma_deg) >= model,
        "{} vs {model}",
        peak.sigma_deg
    );
    let tight = primary(&estimate_with(
        &manifold,
        &DoaConfig::default(),
        &r,
        SNAPSHOTS,
        0.0,
    ));
    assert!(tight.sigma_deg < peak.sigma_deg);
    assert!(f64::from(tight.sigma_deg) >= SIGMA_FLOOR_DEG);
}

#[test]
fn confidence_is_low_for_noise_and_high_for_a_clean_source() {
    let geometry = half_wave_uca5();
    let open = DoaConfig {
        squelch: None,
        ..DoaConfig::default()
    };
    let noise = covariance(&mut scene(&geometry, &[], 43), SNAPSHOTS);
    let report = estimate(&geometry, &open, &noise);
    assert!(report.squelch_open);
    assert!(primary(&report).confidence < 0.1, "{report:?}");
    let clean = covariance(&mut scene(&geometry, &[(137.0, 20.0)], 44), SNAPSHOTS);
    let report = estimate(&geometry, &open, &clean);
    assert!(primary(&report).confidence > 0.9, "{report:?}");
}

#[test]
fn fit_drops_with_phase_errors() {
    let geometry = half_wave_uca5();
    let clean = covariance(&mut scene(&geometry, &[(137.0, 20.0)], 45), SNAPSHOTS);
    let report = estimate(&geometry, &DoaConfig::default(), &clean);
    let fit_clean = report.fit;
    assert!(fit_clean > 0.98, "{fit_clean}");
    assert!((primary(&report).fit - fit_clean).abs() < 1e-3);
    let mut skewed = scene(&geometry, &[(137.0, 20.0)], 45);
    skewed.lane_phase_deg = vec![0.0, 52.0, -31.0, 44.0, -38.0];
    let r = covariance(&mut skewed, SNAPSHOTS);
    let report = estimate(&geometry, &DoaConfig::default(), &r);
    assert!(
        report.fit <= fit_clean - 0.05,
        "{} vs {fit_clean}",
        report.fit
    );
}

#[test]
fn ula_mirror_is_reported_and_halves_confidence() {
    let geometry = ula(4, 90.0);
    let r = covariance(&mut scene(&geometry, &[(30.0, 20.0)], 47), SNAPSHOTS);
    let peak = primary(&estimate(&geometry, &DoaConfig::default(), &r));
    assert!(error_deg(peak.azimuth_deg, 30.0) < 1.0, "{peak:?}");
    assert!(error_deg(peak.mirror_deg.unwrap(), 150.0) < 1.0, "{peak:?}");
    assert_eq!(peak.ambiguity, 0.5);
    assert!(peak.confidence <= 0.5);
}

#[test]
fn ula_front_side_keeps_the_front_bearing_only() {
    let geometry = ula(4, 90.0);
    let r = covariance(&mut scene(&geometry, &[(30.0, 20.0)], 49), SNAPSHOTS);
    let front = DoaConfig {
        ula_side: UlaSide::Front,
        ..DoaConfig::default()
    };
    let manifold = Manifold::ideal(geometry.clone());
    let doa = Doa::new(&manifold, &front, FREQ).unwrap();
    assert_eq!(doa.grid().points(), 181);
    let peak = primary(&estimate(&geometry, &front, &r));
    assert!(error_deg(peak.azimuth_deg, 30.0) < 1.0, "{peak:?}");
    assert_eq!(peak.mirror_deg, None);
    assert_eq!(peak.ambiguity, 0.0);
    let back = DoaConfig {
        ula_side: UlaSide::Back,
        ..front
    };
    let peak = primary(&estimate(&geometry, &back, &r));
    assert!(error_deg(peak.azimuth_deg, 150.0) < 1.0, "{peak:?}");
    for estimator in [Estimator::RootMusic, Estimator::Esprit] {
        for (side, want) in [(front, 30.0), (back, 150.0)] {
            let config = DoaConfig { estimator, ..side };
            let peak = primary(&estimate(&geometry, &config, &r));
            assert!(
                error_deg(peak.azimuth_deg, want) < 1.0,
                "{estimator:?}: {peak:?}"
            );
            assert_eq!(peak.mirror_deg, None);
        }
    }
}

#[test]
fn aliasing_halves_the_confidence_of_a_single_source() {
    let geometry = half_wave_uca5();
    let manifold = Manifold::ideal(geometry.clone());
    let doubled = 2.0 * FREQ;
    let mut aliased = ArrayScene::new(geometry, doubled, RATE)
        .with_noise_db(0.0)
        .with_seed(71)
        .with_source(broadband(Direction::horizon(137.0), 20.0));
    let r = covariance(&mut aliased, SNAPSHOTS);
    let mut doa = Doa::new(&manifold, &DoaConfig::default(), doubled).unwrap();
    assert!(doa.alias().aliased);
    let mut report = DoaReport::default();
    doa.estimate(&manifold, &r, SNAPSHOTS as f64, 3.0, &mut report)
        .unwrap();
    let peak = primary(&report);
    assert_eq!(report.sources, 1);
    assert_eq!(peak.ambiguity, 0.5);
    assert_eq!(peak.mirror_deg, None);
    assert!(peak.confidence <= 0.5, "{peak:?}");
}

#[test]
fn doa_moves_between_threads() {
    fn sendable<T: Send>() {}
    sendable::<Doa>();
}
