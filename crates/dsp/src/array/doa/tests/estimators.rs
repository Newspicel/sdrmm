use super::*;

#[test]
fn bartlett_capon_music_find_one_source_on_the_kraken_uca() {
    let geometry = half_wave_uca5();
    for (index, bearing) in [0.0, 37.0, 137.0, 271.0, 359.5].into_iter().enumerate() {
        let r = covariance(
            &mut scene(&geometry, &[(bearing, 10.0)], index as u64 + 1),
            SNAPSHOTS,
        );
        for (estimator, limit) in [
            (Estimator::Bartlett, 2.0),
            (Estimator::Capon, 1.0),
            (Estimator::Music, 1.0),
        ] {
            let report = estimate(&geometry, &with(estimator), &r);
            let found = primary(&report).azimuth_deg;
            assert!(
                error_deg(found, bearing) < limit,
                "{estimator:?} at {bearing}: {found}"
            );
            assert_eq!(report.sources, 1);
        }
    }
}

#[test]
fn music_resolves_two_sources_20_degrees_apart_on_an_8_element_ula() {
    let geometry = ula(8, 180.0);
    let r = covariance(
        &mut scene(&geometry, &[(70.0, 20.0), (90.0, 20.0)], 3),
        SNAPSHOTS,
    );
    let report = estimate(&geometry, &DoaConfig::default(), &r);
    assert_eq!(report.peak_count, 2, "{report:?}");
    let found = sorted_azimuths(&report);
    assert!(error_deg(found[0], 70.0) < 1.5, "{found:?}");
    assert!(error_deg(found[1], 90.0) < 1.5, "{found:?}");
}

#[test]
fn root_music_matches_music_on_a_ula_and_reports_the_mirror() {
    let geometry = ula(8, 90.0);
    let r = covariance(&mut scene(&geometry, &[(30.0, 20.0)], 5), SNAPSHOTS);
    let rooted = primary(&estimate(&geometry, &with(Estimator::RootMusic), &r));
    let gridded = primary(&estimate(&geometry, &with(Estimator::Music), &r));
    assert!(error_deg(rooted.azimuth_deg, 30.0) < 0.5, "{rooted:?}");
    assert!(error_deg(rooted.azimuth_deg, gridded.azimuth_deg) < 0.5);
    let mirror = rooted.mirror_deg.unwrap();
    assert!(error_deg(mirror, 180.0 - rooted.azimuth_deg) < 1e-9);
    assert_eq!(rooted.ambiguity, 0.5);
}

#[test]
fn root_music_on_a_uca_uses_phase_mode() {
    let geometry = uca_for_beta(8, 2.0, FREQ);
    let manifold = Manifold::ideal(geometry.clone());
    for (seed, bearing) in [(7, 37.0), (8, 200.0)] {
        let r = covariance(&mut scene(&geometry, &[(bearing, 20.0)], seed), SNAPSHOTS);
        let report = estimate(&geometry, &with(Estimator::RootMusic), &r);
        let found = primary(&report).azimuth_deg;
        assert!(error_deg(found, bearing) < 1.0, "{found}");
    }
    let doa = Doa::new(&manifold, &with(Estimator::RootMusic), FREQ).unwrap();
    assert!(doa.mode_bias_deg().is_some_and(|bias| bias < 1.0));
    let element = Doa::new(&manifold, &with(Estimator::Music), FREQ).unwrap();
    assert_eq!(element.mode_bias_deg(), None);
}

#[test]
fn root_music_on_the_kraken_uca_near_the_bessel_null() {
    for chord in [0.45, 0.5] {
        let geometry = uca_for_chord(5, chord, FREQ);
        for (seed, bearing) in [0.0, 37.0, 137.0, 271.0].into_iter().enumerate() {
            let r = covariance(
                &mut scene(&geometry, &[(bearing, 20.0)], seed as u64 + 11),
                SNAPSHOTS,
            );
            let report = estimate(&geometry, &with(Estimator::RootMusic), &r);
            let found = primary(&report).azimuth_deg;
            assert!(
                error_deg(found, bearing) < 1.5,
                "chord {chord} at {bearing}: {found}"
            );
        }
    }
}

#[test]
fn esprit_near_the_bessel_null_says_so() {
    let manifold = Manifold::ideal(uca_for_chord(5, 0.45, FREQ));
    assert!(matches!(
        Doa::new(&manifold, &with(Estimator::Esprit), FREQ),
        Err(DoaError::Covariance(CovarianceError::BesselNull))
    ));
    let smoothed = DoaConfig {
        smoothing: 1,
        ..DoaConfig::default()
    };
    assert!(matches!(
        Doa::new(&manifold, &smoothed, FREQ),
        Err(DoaError::Covariance(CovarianceError::BesselNull))
    ));
}

#[test]
fn esprit_finds_two_sources_on_a_ula() {
    let geometry = ula(8, 180.0);
    let r = covariance(
        &mut scene(&geometry, &[(60.0, 20.0), (110.0, 20.0)], 13),
        SNAPSHOTS,
    );
    let report = estimate(&geometry, &with(Estimator::Esprit), &r);
    assert_eq!(report.peak_count, 2, "{report:?}");
    let found = sorted_azimuths(&report);
    assert!(error_deg(found[0], 60.0) < 1.0, "{found:?}");
    assert!(error_deg(found[1], 110.0) < 1.0, "{found:?}");
    assert!(report.peaks().iter().all(|peak| peak.mirror_deg.is_some()));
}

#[test]
fn esprit_on_a_uca_uses_phase_mode() {
    let geometry = uca_for_beta(8, 2.0, FREQ);
    for (seed, bearing) in [(17, 37.0), (18, 305.0)] {
        let r = covariance(&mut scene(&geometry, &[(bearing, 20.0)], seed), SNAPSHOTS);
        let report = estimate(&geometry, &with(Estimator::Esprit), &r);
        let found = primary(&report).azimuth_deg;
        assert!(error_deg(found, bearing) < 1.0, "{found}");
    }
}
