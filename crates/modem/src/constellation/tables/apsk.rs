use std::f64::consts::{FRAC_PI_4, PI};

use super::{check_order, label_by_descent, polar};
use crate::constellation::{Constellation, ConstellationError};

/// EN 302 307-1 phase of the 32APSK middle ring.
const APSK32_MIDDLE_PHASE_RAD: f64 = PI / 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ApskRing {
    pub points: u32,
    pub radius: f64,
    pub phase_rad: f64,
}

pub fn apsk(rings: &[ApskRing]) -> Result<Constellation, ConstellationError> {
    let m = rings
        .iter()
        .map(|r| r.points)
        .fold(0u32, u32::saturating_add);
    let sane = !rings.is_empty()
        && rings
            .iter()
            .all(|r| r.points > 0 && r.radius.is_finite() && r.radius > 0.0);
    check_order("APSK", m, sane)?;
    let mut points = Vec::with_capacity(m as usize);
    for ring in rings {
        points.extend((0..ring.points).map(|k| polar(ring.radius, ring.phase_rad, k, ring.points)));
    }
    let labels = label_by_descent(&points);
    Constellation::from_points(points, labels)
}

pub fn apsk16_dvbs2(gamma: f64) -> Result<Constellation, ConstellationError> {
    if !(gamma.is_finite() && gamma > 1.0) {
        return Err(ConstellationError::UnsupportedOrder {
            family: "APSK",
            m: 16,
        });
    }
    apsk(&[
        ApskRing {
            points: 4,
            radius: 1.0,
            phase_rad: FRAC_PI_4,
        },
        ApskRing {
            points: 12,
            radius: gamma,
            phase_rad: 0.0,
        },
    ])
}

pub fn apsk32_dvbs2(gamma1: f64, gamma2: f64) -> Result<Constellation, ConstellationError> {
    if !(gamma1.is_finite() && gamma2.is_finite() && gamma1 > 1.0 && gamma2 > gamma1) {
        return Err(ConstellationError::UnsupportedOrder {
            family: "APSK",
            m: 32,
        });
    }
    apsk(&[
        ApskRing {
            points: 4,
            radius: 1.0,
            phase_rad: FRAC_PI_4,
        },
        ApskRing {
            points: 12,
            radius: gamma1,
            phase_rad: APSK32_MIDDLE_PHASE_RAD,
        },
        ApskRing {
            points: 16,
            radius: gamma2,
            phase_rad: 0.0,
        },
    ])
}
