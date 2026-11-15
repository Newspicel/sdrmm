use super::*;
use crate::math::geodesy::vec3::scale;

const MUNICH: Geodetic = Geodetic {
    lat_deg: 48.137,
    lon_deg: 11.575,
    alt_m: 519.0,
};

fn at(origin: Geodetic, east: f64, north: f64, alt_m: f64) -> Geodetic {
    Geodetic {
        alt_m,
        ..geodetic_from_enu(origin, [east, north, 0.0])
    }
}

fn distance(a: Geodetic, b: Geodetic) -> f64 {
    norm(sub(ecef(a), ecef(b)))
}

fn horizontal_error(truth: Geodetic, got: Geodetic) -> f64 {
    let [east, north, _] = enu(truth, ecef(got));
    east.hypot(north)
}

#[test]
fn range_is_zero_on_the_baseline() {
    let rx = ecef(MUNICH);
    let tx = ecef(at(MUNICH, 25_000.0, 12_000.0, 900.0));
    for fraction in [0.0, 0.3, 0.77, 1.0] {
        let target = add(rx, scale(sub(tx, rx), fraction));
        assert!(bistatic_range_m(tx, rx, target).abs() < 1e-6);
    }
    let off = ecef(at(MUNICH, 10_000.0, -8_000.0, 3_000.0));
    assert!(bistatic_range_m(tx, rx, off) > 1_000.0);
}

#[test]
fn rate_matches_the_numeric_derivative() {
    let rx = ecef(MUNICH);
    let tx = ecef(at(MUNICH, 40_000.0, -5_000.0, 300.0));
    let dt = 1e-3;
    for (east, north, up, velocity) in [
        (10_000.0, 20_000.0, 9_000.0, [220.0, -40.0, 5.0]),
        (-35_000.0, 3_000.0, 11_000.0, [-100.0, 180.0, -12.0]),
        (60_000.0, -1_000.0, 2_000.0, [0.0, 0.0, 30.0]),
    ] {
        let site = at(MUNICH, east, north, up);
        let target = ecef(site);
        let v = enu_vector_to_ecef(site, velocity);
        let ahead = bistatic_range_m(tx, rx, add(target, scale(v, dt)));
        let behind = bistatic_range_m(tx, rx, sub(target, scale(v, dt)));
        let numeric = (ahead - behind) / (2.0 * dt);
        let got = bistatic_rate_mps(tx, rx, target, v);
        assert!(
            (got - numeric).abs() < 1e-5 * numeric.abs().max(1.0),
            "{got} vs {numeric}"
        );
    }
}

#[test]
fn ellipse_and_bearing_meet_at_the_target() {
    let tx = at(MUNICH, 26_000.0, 15_000.0, 800.0);
    let (rx_ecef, tx_ecef) = (ecef(MUNICH), ecef(tx));
    let mut found_count = 0;
    for (dlat, dlon) in [
        (0.05, 0.02),
        (0.3, -0.4),
        (-0.2, 0.6),
        (-0.5, -0.1),
        (0.01, 0.45),
    ] {
        for alt_m in [300.0, 3_000.0, 11_000.0] {
            let target = Geodetic {
                lat_deg: MUNICH.lat_deg + dlat,
                lon_deg: MUNICH.lon_deg + dlon,
                alt_m,
            };
            let range = bistatic_range_m(tx_ecef, rx_ecef, ecef(target));
            let bearing = bearing_deg(MUNICH, target);
            match locate(MUNICH, tx, range, bearing, alt_m) {
                Some(found) => {
                    found_count += 1;
                    assert!(
                        distance(found, target) < 1.0,
                        "{dlat}, {dlon}, {alt_m}: {} m off",
                        distance(found, target)
                    );
                }
                None => {
                    let overhead = ecef(at(MUNICH, 0.0, 0.0, alt_m));
                    let overhead_range = bistatic_range_m(tx_ecef, rx_ecef, overhead);
                    assert!(
                        overhead_range >= range,
                        "{dlat}, {dlon}, {alt_m}: no fix although the ray is unambiguous"
                    );
                }
            }
        }
    }
    assert!(found_count >= 12, "only {found_count} fixes");
}

#[test]
fn no_fix_for_non_positive_range() {
    let tx = at(MUNICH, 30_000.0, 0.0, 500.0);
    for range in [0.0, -250.0, f64::NAN, f64::INFINITY] {
        assert_eq!(locate(MUNICH, tx, range, 45.0, 1_000.0), None);
        assert_eq!(
            locate_with_sigma(MUNICH, tx, range, 45.0, 1_000.0, 100.0, 1.0),
            None
        );
    }
    assert_eq!(locate(MUNICH, tx, 5_000.0, f64::NAN, 1_000.0), None);
    assert_eq!(locate(MUNICH, tx, 1_000.0, 90.0, 20_000.0), None);
    assert_eq!(
        locate_with_sigma(MUNICH, tx, 5_000.0, 45.0, 1_000.0, -1.0, 1.0),
        None
    );
}

#[test]
fn sigma_ellipse_grows_with_bearing_sigma() {
    let tx = at(MUNICH, 30_000.0, 0.0, 500.0);
    let fix = |sigma_bearing| {
        locate_with_sigma(MUNICH, tx, 25_000.0, 20.0, 3_000.0, 150.0, sigma_bearing).unwrap()
    };
    let [tight, wide, wider] = [fix(0.5), fix(2.0), fix(6.0)];
    assert!(tight.major_m < wide.major_m && wide.major_m < wider.major_m);
    assert!(wider.major_m > 2.0 * wide.major_m);
    for located in [tight, wide, wider] {
        assert!(located.minor_m >= 0.0 && located.minor_m <= located.major_m);
        assert!((0.0..180.0).contains(&located.orientation_deg));
    }
    let point_only = locate(MUNICH, tx, 25_000.0, 20.0, 3_000.0).unwrap();
    assert_eq!(tight.point, point_only);
    let target = enu(MUNICH, ecef(point_only));
    let transmitter = enu(MUNICH, ecef(tx));
    let normal = add(unit(target), unit(sub(target, transmitter)));
    let contour = (normal[0].atan2(normal[1]).to_degrees() + 90.0).rem_euclid(180.0);
    let skew = (wider.orientation_deg - contour).abs();
    assert!(
        skew.min(180.0 - skew) < 3.0,
        "{wider:?} vs iso-range contour {contour}"
    );
    let still = locate_with_sigma(MUNICH, tx, 25_000.0, 20.0, 3_000.0, 0.0, 0.0).unwrap();
    assert!(still.major_m < 1e-9);
}

#[test]
fn an_aircraft_at_10_km_altitude_is_located_within_200_m() {
    let tx = geodetic_from_enu(MUNICH, [30_000.0, 0.0, 0.0]);
    let aircraft = geodetic_from_enu(MUNICH, [0.0, 20_000.0, 10_000.0]);
    let range = bistatic_range_m(ecef(tx), ecef(MUNICH), ecef(aircraft));
    assert!((range - 29_777.3).abs() < 1.0, "range {range}");
    let bearing = bearing_deg(MUNICH, aircraft);
    let found = locate(MUNICH, tx, range, bearing, aircraft.alt_m).unwrap();
    assert!(horizontal_error(aircraft, found) < 200.0);
    let flat = locate(MUNICH, tx, range, bearing, MUNICH.alt_m).unwrap();
    assert!(horizontal_error(aircraft, flat) > 1_000.0);
}
