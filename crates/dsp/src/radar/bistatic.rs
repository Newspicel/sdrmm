mod locate;

use crate::math::geodesy::vec3::{Vec3, add, dot, norm, sub, unit};
pub use crate::math::geodesy::{
    Geodetic, Look, WGS84_A_M, WGS84_F, bearing_deg, ecef, enu, enu_vector_to_ecef,
    geodetic_from_enu, look,
};
pub use locate::{Located, locate, locate_with_sigma};

#[must_use]
pub fn bistatic_range_m(tx: Vec3, rx: Vec3, target: Vec3) -> f64 {
    norm(sub(target, tx)) + norm(sub(target, rx)) - norm(sub(tx, rx))
}

#[must_use]
pub fn bistatic_rate_mps(tx: Vec3, rx: Vec3, target: Vec3, velocity: Vec3) -> f64 {
    dot(velocity, add(unit(sub(target, tx)), unit(sub(target, rx))))
}

#[cfg(test)]
mod tests;
