use super::*;

fn likelihood_of(
    geometry: &Geometry,
    azimuth: f64,
    seed: u64,
) -> ([f32; LIKELIHOOD_POINTS], DoaReport) {
    let manifold = Manifold::ideal(geometry.clone());
    let r = covariance(&mut scene(geometry, &[(azimuth, 10.0)], seed), SNAPSHOTS);
    let mut doa = Doa::new(&manifold, &DoaConfig::default(), FREQ).unwrap();
    let mut report = DoaReport::default();
    doa.estimate(
        &manifold,
        &r,
        SNAPSHOTS as f64,
        NO_CAL_SIGMA_DEG,
        &mut report,
    )
    .unwrap();
    let mut out = [0.0f32; LIKELIHOOD_POINTS];
    assert!(doa.likelihood(&manifold, &r, &report, &mut out).unwrap());
    (out, report)
}

fn maxima(values: &[f32; LIKELIHOOD_POINTS]) -> Vec<usize> {
    (0..LIKELIHOOD_POINTS)
        .filter(|&i| {
            let left = values[(i + LIKELIHOOD_POINTS - 1) % LIKELIHOOD_POINTS];
            let right = values[(i + 1) % LIKELIHOOD_POINTS];
            values[i] > left && values[i] >= right
        })
        .collect()
}

#[test]
fn likelihood_curvature_matches_sigma() {
    let (out, report) = likelihood_of(&half_wave_uca5(), 37.0, 51);
    let top = maxima(&out)
        .into_iter()
        .max_by(|&a, &b| out[a].total_cmp(&out[b]))
        .unwrap();
    assert!(error_deg(top as f64, 37.0) <= 1.0);
    let step = 1f64.to_radians();
    let second = f64::from(out[top + 1] - 2.0 * out[top] + out[top - 1]) / (step * step);
    let sigma = f64::from(primary(&report).sigma_deg).to_radians();
    let measured = -1.0 / second;
    assert!(
        (measured - sigma * sigma).abs() < 0.1 * sigma * sigma,
        "{measured} vs {}",
        sigma * sigma
    );
}

#[test]
fn likelihood_is_bimodal_on_a_ula() {
    let (out, _) = likelihood_of(&ula(4, 90.0), 30.0, 53);
    let mut peaks = maxima(&out);
    peaks.sort_by(|&a, &b| out[b].total_cmp(&out[a]));
    let mut top: Vec<f64> = peaks.iter().take(2).map(|&i| i as f64).collect();
    top.sort_by(f64::total_cmp);
    assert!(error_deg(top[0], 30.0) <= 1.0, "{top:?}");
    assert!(error_deg(top[1], 150.0) <= 1.0, "{top:?}");
}

#[test]
fn likelihood_keeps_only_the_chosen_ula_side() {
    let geometry = ula(4, 90.0);
    let manifold = Manifold::ideal(geometry.clone());
    let r = covariance(&mut scene(&geometry, &[(30.0, 10.0)], 53), SNAPSHOTS);
    let floor = LIKELIHOOD_FLOOR.ln();
    for (side, kept, gone) in [(UlaSide::Front, 30, 150), (UlaSide::Back, 150, 30)] {
        let config = DoaConfig {
            ula_side: side,
            ..DoaConfig::default()
        };
        let mut doa = Doa::new(&manifold, &config, FREQ).unwrap();
        let mut report = DoaReport::default();
        doa.estimate(
            &manifold,
            &r,
            SNAPSHOTS as f64,
            NO_CAL_SIGMA_DEG,
            &mut report,
        )
        .unwrap();
        assert!(error_deg(primary(&report).azimuth_deg, kept as f64) < 1.0);
        let mut out = [0.0f32; LIKELIHOOD_POINTS];
        assert!(doa.likelihood(&manifold, &r, &report, &mut out).unwrap());
        assert!(out[kept] > floor + 1.0, "{side:?}: {}", out[kept]);
        assert_eq!(out[gone], floor, "{side:?}");
        let top = maxima(&out)
            .into_iter()
            .max_by(|&a, &b| out[a].total_cmp(&out[b]))
            .unwrap();
        assert!(error_deg(top as f64, kept as f64) <= 1.0, "{side:?}: {top}");
    }
}

#[test]
fn likelihood_never_drops_below_the_floor() {
    let (out, _) = likelihood_of(&half_wave_uca5(), 200.0, 55);
    let floor = LIKELIHOOD_FLOOR.ln();
    assert!(out.iter().all(|&v| v >= floor));
    assert!(out.contains(&floor));
    let mut bytes = [0u8; LIKELIHOOD_POINTS];
    quantize_likelihood(&out, &mut bytes);
    assert!(bytes[199..=201].contains(&255));
    assert!(bytes.contains(&0));
}
