use super::vec3::{Vec3, add, dot, scale, sub};
use super::{Geodetic, ecef, geodetic};

struct Frame {
    east: Vec3,
    north: Vec3,
    up: Vec3,
}

impl Frame {
    fn at(origin: Geodetic) -> Self {
        let (sin_lat, cos_lat) = origin.lat_deg.to_radians().sin_cos();
        let (sin_lon, cos_lon) = origin.lon_deg.to_radians().sin_cos();
        Self {
            east: [-sin_lon, cos_lon, 0.0],
            north: [-sin_lat * cos_lon, -sin_lat * sin_lon, cos_lat],
            up: [cos_lat * cos_lon, cos_lat * sin_lon, sin_lat],
        }
    }
}

/// East, north, up of the earth-fixed `point` seen from `origin`.
#[must_use]
pub fn enu(origin: Geodetic, point: Vec3) -> Vec3 {
    ecef_vector_to_enu(origin, sub(point, ecef(origin)))
}

/// Rotates an earth-fixed vector, in any unit, into east, north, up at `origin`.
#[must_use]
pub fn ecef_vector_to_enu(origin: Geodetic, v: Vec3) -> Vec3 {
    let frame = Frame::at(origin);
    [dot(frame.east, v), dot(frame.north, v), dot(frame.up, v)]
}

/// Rotates an east, north, up vector, in any unit, at `origin` into earth-fixed axes.
#[must_use]
pub fn enu_vector_to_ecef(origin: Geodetic, v: Vec3) -> Vec3 {
    let frame = Frame::at(origin);
    add(
        add(scale(frame.east, v[0]), scale(frame.north, v[1])),
        scale(frame.up, v[2]),
    )
}

#[must_use]
pub fn geodetic_from_enu(origin: Geodetic, enu: Vec3) -> Geodetic {
    geodetic(add(ecef(origin), enu_vector_to_ecef(origin, enu)))
}
