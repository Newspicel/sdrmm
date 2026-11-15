use super::vec3::{add, norm, sub};
use super::*;

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

#[test]
fn a_balloon_overhead_and_one_on_the_horizon_look_right() {
    let overhead = look(
        MUNICH,
        Geodetic {
            alt_m: MUNICH.alt_m + 20_000.0,
            ..MUNICH
        },
    );
    assert!((overhead.elevation_deg - 90.0).abs() < 1e-6);
    assert!((overhead.range_m - 20_000.0).abs() < 1e-3);
    let east = look(MUNICH, at(MUNICH, 50_000.0, 0.0, MUNICH.alt_m + 10_000.0));
    assert!((east.azimuth_deg - 90.0).abs() < 0.1);
    assert!(east.elevation_deg > 10.0 && east.elevation_deg < 11.5);
    let north = look(MUNICH, at(MUNICH, 0.0, 30_000.0, 0.0));
    assert!(north.azimuth_deg.abs() < 0.1 || (north.azimuth_deg - 360.0).abs() < 0.1);
    assert!(north.elevation_deg < 0.0);
}

#[test]
fn ecef_round_trips_through_enu() {
    let origin_equator = ecef(Geodetic::default());
    assert!((origin_equator[0] - WGS84_A_M).abs() < 1e-9);
    let pole = ecef(Geodetic {
        lat_deg: 90.0,
        lon_deg: 0.0,
        alt_m: 0.0,
    });
    assert!((pole[2] - WGS84_A_M * (1.0 - WGS84_F)).abs() < 1e-6);
    let origins = [
        MUNICH,
        Geodetic::default(),
        Geodetic {
            lat_deg: -33.87,
            lon_deg: 151.21,
            alt_m: 30.0,
        },
        Geodetic {
            lat_deg: 89.95,
            lon_deg: -40.0,
            alt_m: 2_000.0,
        },
        Geodetic {
            lat_deg: -60.0,
            lon_deg: -179.9,
            alt_m: -20.0,
        },
    ];
    let offsets = [
        [0.0, 0.0, 0.0],
        [1_000.0, -2_000.0, 50.0],
        [-30_000.0, 45_000.0, 10_000.0],
        [150_000.0, 80_000.0, -200.0],
    ];
    for origin in origins {
        for offset in offsets {
            let point = geodetic_from_enu(origin, offset);
            let back = enu(origin, ecef(point));
            assert!(
                norm(sub(back, offset)) < 1e-6,
                "{origin:?} {offset:?}: {back:?}"
            );
            let again = geodetic_from_enu(origin, enu(origin, ecef(point)));
            assert!((again.lat_deg - point.lat_deg).abs() < 1e-10);
            assert!((again.lon_deg - point.lon_deg).abs() < 1e-10);
            assert!((again.alt_m - point.alt_m).abs() < 1e-6);
        }
    }
}

#[test]
fn enu_vectors_rotate_back() {
    let equator = Geodetic::default();
    let east = enu_vector_to_ecef(equator, [1.0, 0.0, 0.0]);
    let up = enu_vector_to_ecef(equator, [0.0, 0.0, 1.0]);
    assert!(norm(sub(east, [0.0, 1.0, 0.0])) < 1e-12);
    assert!(norm(sub(up, [1.0, 0.0, 0.0])) < 1e-12);
    let v = [3.0, -4.0, 12.0];
    let rotated = enu_vector_to_ecef(MUNICH, v);
    assert!((norm(rotated) - 13.0).abs() < 1e-12);
    let back = enu(MUNICH, add(ecef(MUNICH), rotated));
    assert!(norm(sub(back, v)) < 1e-6);
    assert!(norm(sub(ecef_vector_to_enu(MUNICH, rotated), v)) < 1e-12);
}

#[test]
fn the_poles_and_the_equator_read_back() {
    let equator = geodetic([WGS84_A_M + 400_000.0, 0.0, 0.0]);
    assert!(equator.lat_deg.abs() < 1e-9, "{equator:?}");
    assert!(equator.lon_deg.abs() < 1e-9, "{equator:?}");
    assert!((equator.alt_m - 400_000.0).abs() < 1e-6, "{equator:?}");
    let polar = WGS84_A_M * (1.0 - WGS84_F);
    let pole = geodetic([0.0, 0.0, -polar - 800_000.0]);
    assert!((pole.lat_deg + 90.0).abs() < 1e-9, "{pole:?}");
    assert!((pole.alt_m - 800_000.0).abs() < 1e-6, "{pole:?}");
}

#[test]
fn bearing_follows_the_compass() {
    for (east, north, expected) in [
        (0.0, 5_000.0, 0.0),
        (5_000.0, 0.0, 90.0),
        (0.0, -5_000.0, 180.0),
        (-5_000.0, 0.0, 270.0),
        (3_000.0, 3_000.0, 45.0),
    ] {
        let target = geodetic_from_enu(MUNICH, [east, north, 0.0]);
        let got = bearing_deg(MUNICH, target);
        let diff = (got - expected).rem_euclid(360.0);
        assert!(diff.min(360.0 - diff) < 1e-6, "{east}, {north}: {got}");
        assert!((0.0..360.0).contains(&got));
    }
}
