use crate::math::geodesy::vec3::{Vec3, norm, scale, sub};
use crate::math::geodesy::{Geodetic, ecef, enu, geodetic_from_enu};

const ALTITUDE_STEPS: usize = 8;
const ALTITUDE_TOLERANCE_M: f64 = 1e-9;
const BISECTION_STEPS: usize = 60;
const BRACKET_DOUBLINGS: usize = 8;
const RANGE_STEP_M: f64 = 1.0;
const BEARING_STEP_DEG: f64 = 0.01;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    pub point: Geodetic,
    pub major_m: f64,
    pub minor_m: f64,
    pub orientation_deg: f64,
}

#[must_use]
pub fn locate(
    rx: Geodetic,
    tx: Geodetic,
    bistatic_range_m: f64,
    bearing_deg: f64,
    target_alt_m: f64,
) -> Option<Geodetic> {
    let usable = bistatic_range_m > 0.0
        && [bistatic_range_m, bearing_deg, target_alt_m]
            .into_iter()
            .chain(finite_parts(rx))
            .chain(finite_parts(tx))
            .all(f64::is_finite);
    if !usable {
        return None;
    }
    let transmitter = enu(rx, ecef(tx));
    let path_sum = bistatic_range_m + norm(transmitter);
    let heading = bearing_deg.to_radians().sin_cos();
    let path = |ground: f64| {
        let target = along(rx, heading, ground, target_alt_m);
        norm(target) + norm(sub(target, transmitter))
    };
    if path(0.0) >= path_sum {
        return None;
    }
    let mut high = path_sum;
    let mut doublings = 0;
    while path(high) < path_sum {
        if doublings == BRACKET_DOUBLINGS {
            return None;
        }
        high *= 2.0;
        doublings += 1;
    }
    let mut low = 0.0;
    for _ in 0..BISECTION_STEPS {
        let middle = 0.5 * (low + high);
        if path(middle) < path_sum {
            low = middle;
        } else {
            high = middle;
        }
    }
    Some(geodetic_from_enu(
        rx,
        along(rx, heading, 0.5 * (low + high), target_alt_m),
    ))
}

#[must_use]
pub fn locate_with_sigma(
    rx: Geodetic,
    tx: Geodetic,
    bistatic_range_m: f64,
    bearing_deg: f64,
    target_alt_m: f64,
    sigma_range_m: f64,
    sigma_bearing_deg: f64,
) -> Option<Located> {
    let sigmas_usable = [sigma_range_m, sigma_bearing_deg]
        .into_iter()
        .all(|sigma| sigma >= 0.0 && sigma.is_finite());
    if !sigmas_usable {
        return None;
    }
    let point = locate(rx, tx, bistatic_range_m, bearing_deg, target_alt_m)?;
    let longer = locate(
        rx,
        tx,
        bistatic_range_m + RANGE_STEP_M,
        bearing_deg,
        target_alt_m,
    )?;
    let turned = locate(
        rx,
        tx,
        bistatic_range_m,
        bearing_deg + BEARING_STEP_DEG,
        target_alt_m,
    )?;
    let center = enu(rx, ecef(point));
    let by_range = scale(sub(enu(rx, ecef(longer)), center), 1.0 / RANGE_STEP_M);
    let by_bearing = scale(sub(enu(rx, ecef(turned)), center), 1.0 / BEARING_STEP_DEG);
    let (range_var, bearing_var) = (sigma_range_m.powi(2), sigma_bearing_deg.powi(2));
    let east = by_range[0].powi(2) * range_var + by_bearing[0].powi(2) * bearing_var;
    let north = by_range[1].powi(2) * range_var + by_bearing[1].powi(2) * bearing_var;
    let cross = by_range[0] * by_range[1] * range_var + by_bearing[0] * by_bearing[1] * bearing_var;
    Some(ellipse(point, east, north, cross))
}

fn ellipse(point: Geodetic, east: f64, north: f64, cross: f64) -> Located {
    let mean = 0.5 * (east + north);
    let radius = (0.5 * (east - north)).hypot(cross);
    let from_east = 0.5 * (2.0 * cross).atan2(east - north);
    Located {
        point,
        major_m: (mean + radius).max(0.0).sqrt(),
        minor_m: (mean - radius).max(0.0).sqrt(),
        orientation_deg: (90.0 - from_east.to_degrees()).rem_euclid(180.0),
    }
}

fn finite_parts(point: Geodetic) -> [f64; 3] {
    [point.lat_deg, point.lon_deg, point.alt_m]
}

fn along(rx: Geodetic, heading: (f64, f64), ground: f64, target_alt_m: f64) -> Vec3 {
    let (sin, cos) = heading;
    let mut target = [ground * sin, ground * cos, target_alt_m - rx.alt_m];
    for _ in 0..ALTITUDE_STEPS {
        let error = geodetic_from_enu(rx, target).alt_m - target_alt_m;
        target[2] -= error;
        if error.abs() < ALTITUDE_TOLERANCE_M {
            break;
        }
    }
    target
}
