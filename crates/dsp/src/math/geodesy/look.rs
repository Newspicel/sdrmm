use super::vec3::norm;
use super::{Geodetic, ecef, enu};
use crate::math::special::norm_deg;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub range_m: f64,
}

/// Where `to` stands in the sky seen from `from`.
#[must_use]
pub fn look(from: Geodetic, to: Geodetic) -> Look {
    let local = enu(from, ecef(to));
    let [east, north, up] = local;
    Look {
        azimuth_deg: norm_deg(east.atan2(north).to_degrees()),
        elevation_deg: up.atan2(east.hypot(north)).to_degrees(),
        range_m: norm(local),
    }
}

#[must_use]
pub fn bearing_deg(from: Geodetic, to: Geodetic) -> f64 {
    let [east, north, _] = enu(from, ecef(to));
    norm_deg(east.atan2(north).to_degrees())
}
