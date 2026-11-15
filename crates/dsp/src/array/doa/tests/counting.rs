use super::*;

fn counted(rule: OrderRule, sources: &[(f64, f32)], seed: u64) -> usize {
    let geometry = half_wave_uca5();
    let r = covariance(&mut scene(&geometry, sources, seed), SNAPSHOTS);
    let config = DoaConfig {
        rule,
        ..DoaConfig::default()
    };
    estimate(&geometry, &config, &r).sources_raw
}

#[test]
fn dominance_counts_zero_one_two() {
    assert_eq!(counted(OrderRule::Dominance, &[], 21), 0);
    assert_eq!(counted(OrderRule::Dominance, &[(40.0, 10.0)], 22), 1);
    assert_eq!(
        counted(OrderRule::Dominance, &[(40.0, 10.0), (200.0, 4.0)], 23),
        2
    );
}

#[test]
fn mdl_counts_sources_on_ideal_data() {
    assert_eq!(counted(OrderRule::Mdl, &[], 21), 0);
    assert_eq!(counted(OrderRule::Mdl, &[(40.0, 10.0)], 22), 1);
    assert_eq!(
        counted(OrderRule::Mdl, &[(40.0, 10.0), (200.0, 4.0)], 23),
        2
    );
}

#[test]
fn dominance_ignores_calibration_leakage() {
    let geometry = half_wave_uca5();
    let mut leaky = scene(&geometry, &[(120.0, 40.0)], 25);
    leaky.lane_delay_samples = vec![0.0, 0.2, 0.0, 0.0, 0.0];
    let r = covariance(&mut leaky, SNAPSHOTS);
    let config = |rule| DoaConfig {
        rule,
        ..DoaConfig::default()
    };
    let dominance = estimate(&geometry, &config(OrderRule::Dominance), &r);
    let mdl = estimate(&geometry, &config(OrderRule::Mdl), &r);
    assert_eq!(dominance.sources_raw, 1, "{:?}", dominance.eigenvalues);
    assert!(mdl.sources_raw >= 2, "{:?}", mdl.eigenvalues);
}

#[test]
fn noise_alone_closes_the_squelch_and_reports_no_peaks() {
    let geometry = half_wave_uca5();
    let manifold = Manifold::ideal(geometry.clone());
    let r = covariance(&mut scene(&geometry, &[], 31), SNAPSHOTS);
    let mut doa = Doa::new(&manifold, &DoaConfig::default(), FREQ).unwrap();
    let mut report = DoaReport::default();
    doa.estimate(&manifold, &r, SNAPSHOTS as f64, 3.0, &mut report)
        .unwrap();
    assert!(!report.squelch_open);
    assert_eq!(report.peak_count, 0);
    assert!(report.eig_ratio_db < 1.0);
    let mut likelihood = [1.0f32; LIKELIHOOD_POINTS];
    assert!(
        !doa.likelihood(&manifold, &r, &report, &mut likelihood)
            .unwrap()
    );
    assert!(likelihood.iter().all(|&v| v == 1.0));
    assert_eq!(doa.spectrum().len(), 360);
}

#[test]
fn two_peaks_come_strongest_first() {
    let geometry = half_wave_uca5();
    let r = covariance(
        &mut scene(&geometry, &[(40.0, 10.0), (200.0, 4.0)], 33),
        SNAPSHOTS,
    );
    let report = estimate(&geometry, &DoaConfig::default(), &r);
    assert_eq!(report.peak_count, 2, "{report:?}");
    assert!(
        error_deg(report.peaks[0].azimuth_deg, 40.0) < 2.0,
        "{report:?}"
    );
    assert!(
        error_deg(report.peaks[1].azimuth_deg, 200.0) < 2.0,
        "{report:?}"
    );
    assert!(report.peaks[0].power > report.peaks[1].power);
    for estimator in [Estimator::Capon, Estimator::RootMusic, Estimator::Esprit] {
        let single = DoaConfig {
            max_peaks: 1,
            ..with(estimator)
        };
        let report = estimate(&geometry, &single, &r);
        assert_eq!(report.sources, 2, "{estimator:?}");
        assert_eq!(report.peak_count, 1, "{estimator:?}");
        assert!(
            error_deg(report.peaks[0].azimuth_deg, 40.0) < 6.0,
            "{estimator:?}: {report:?}"
        );
    }
}

#[test]
fn refinement_reaches_a_tenth_of_a_degree_off_grid() {
    let geometry = half_wave_uca5();
    let r = covariance(&mut scene(&geometry, &[(37.37, 20.0)], 35), SNAPSHOTS);
    let found = primary(&estimate(&geometry, &DoaConfig::default(), &r)).azimuth_deg;
    assert!(error_deg(found, 37.37) < 0.1, "{found}");
}
