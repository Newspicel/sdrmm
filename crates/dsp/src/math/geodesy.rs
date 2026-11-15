mod enu;
mod look;
pub(crate) mod vec3;

pub use enu::{ecef_vector_to_enu, enu, enu_vector_to_ecef, geodetic_from_enu};
pub use look::{Look, bearing_deg, look};
use vec3::Vec3;

pub const WGS84_A_M: f64 = 6_378_137.0;
pub const WGS84_F: f64 = 1.0 / 298.257_223_563;
pub const WGS84_E2: f64 = WGS84_F * (2.0 - WGS84_F);

const LATITUDE_STEPS: usize = 16;
const LATITUDE_TOLERANCE_RAD: f64 = 1e-15;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Geodetic {
    pub lat_deg: f64,
    pub lon_deg: f64,
    pub alt_m: f64,
}

/// Earth-centred, earth-fixed metres of a WGS84 point.
#[must_use]
pub fn ecef(point: Geodetic) -> Vec3 {
    let (sin_lat, cos_lat) = point.lat_deg.to_radians().sin_cos();
    let (sin_lon, cos_lon) = point.lon_deg.to_radians().sin_cos();
    let n = prime_vertical_m(sin_lat);
    let horizontal = (n + point.alt_m) * cos_lat;
    [
        horizontal * cos_lon,
        horizontal * sin_lon,
        (n * (1.0 - WGS84_E2) + point.alt_m) * sin_lat,
    ]
}

/// The WGS84 point at earth-centred, earth-fixed metres, latitude iterated to convergence.
#[must_use]
pub fn geodetic(point: Vec3) -> Geodetic {
    let [x, y, z] = point;
    let p = x.hypot(y);
    let mut lat = z.atan2(p * (1.0 - WGS84_E2));
    for _ in 0..LATITUDE_STEPS {
        let sin_lat = lat.sin();
        let next = (z + WGS84_E2 * prime_vertical_m(sin_lat) * sin_lat).atan2(p);
        let settled = (next - lat).abs() < LATITUDE_TOLERANCE_RAD;
        lat = next;
        if settled {
            break;
        }
    }
    let (sin_lat, cos_lat) = lat.sin_cos();
    Geodetic {
        lat_deg: lat.to_degrees(),
        lon_deg: y.atan2(x).to_degrees(),
        alt_m: p * cos_lat + z * sin_lat - WGS84_A_M * (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt(),
    }
}

fn prime_vertical_m(sin_lat: f64) -> f64 {
    WGS84_A_M / (1.0 - WGS84_E2 * sin_lat * sin_lat).sqrt()
}

#[cfg(test)]
mod tests;
