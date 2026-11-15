use super::{DoaConfig, UlaSide};
use crate::array::manifold::{AzimuthSpan, Geometry, GridSpec, Permutation, Shape};
use crate::math::special::norm_deg;

/// How far past the endfire a line phase may sit and still be read as endfire.
const LINE_SLACK: f64 = 0.01;
const BROADSIDE_DEG: f64 = 90.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Line {
    pub order: Permutation,
    pub spacing_m: f64,
    pub sort_axis_deg: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Layout {
    pub elements: usize,
    pub antipodes: Option<Permutation>,
    pub collinear: bool,
    pub axis_deg: Option<f64>,
    pub line: Option<Line>,
}

impl Layout {
    pub fn of(geometry: &Geometry) -> Self {
        let axis_deg = match geometry.shape() {
            Shape::Ula { axis_deg, .. } => Some(axis_deg),
            _ => geometry.line_axis_deg(),
        };
        let line = match (
            geometry.uniform_line_spacing_m(),
            geometry.axis_order(),
            geometry.line_axis_deg(),
        ) {
            (Some(spacing_m), Some(order), Some(sort_axis_deg)) => Some(Line {
                order,
                spacing_m,
                sort_axis_deg,
            }),
            _ => None,
        };
        Self {
            elements: geometry.len(),
            antipodes: geometry.antipodes(),
            collinear: geometry.is_collinear(),
            axis_deg,
            line,
        }
    }

    pub fn grid_spec(&self, config: &DoaConfig) -> GridSpec {
        let span = match (config.ula_side, self.axis_deg) {
            (UlaSide::Front, Some(axis)) => AzimuthSpan::Half {
                centre_deg: norm_deg(axis - BROADSIDE_DEG),
            },
            (UlaSide::Back, Some(axis)) => AzimuthSpan::Half {
                centre_deg: norm_deg(axis + BROADSIDE_DEG),
            },
            _ => AzimuthSpan::Full,
        };
        GridSpec {
            azimuth_step_deg: config.azimuth_step_deg,
            span,
            elevation: config.elevation,
        }
    }

    pub fn side_centre_deg(&self, side: UlaSide) -> Option<f64> {
        let axis = self.axis_deg?;
        Some(match side {
            UlaSide::Both | UlaSide::Front => axis - BROADSIDE_DEG,
            UlaSide::Back => axis + BROADSIDE_DEG,
        })
    }
}

/// Both bearings, mirrored about `axis_deg`, of a plane wave that turns `phase_rad` between
/// neighbours `k_d` radians apart.
#[must_use]
pub fn line_azimuths(phase_rad: f64, k_d: f64, axis_deg: f64) -> Option<(f64, f64)> {
    if !(k_d.is_finite() && k_d > 0.0 && phase_rad.is_finite() && axis_deg.is_finite()) {
        return None;
    }
    let ratio = phase_rad / k_d;
    if ratio.abs() > 1.0 + LINE_SLACK {
        return None;
    }
    let offset = ratio.clamp(-1.0, 1.0).acos().to_degrees();
    Some((norm_deg(axis_deg + offset), norm_deg(axis_deg - offset)))
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{FRAC_PI_2, PI};

    use super::*;

    #[test]
    fn line_azimuths_give_both_sides_of_the_axis() {
        let (a, b) = line_azimuths(FRAC_PI_2 * 0.5, PI, 0.0).unwrap();
        assert!((a - 75.522).abs() < 1e-3, "{a}");
        assert!((b - 284.478).abs() < 1e-3, "{b}");
        assert!(line_azimuths(3.3, PI, 0.0).is_none());
        let (edge, _) = line_azimuths(PI * 1.005, PI, 90.0).unwrap();
        assert!((edge - 90.0).abs() < 1e-9);
        assert!(line_azimuths(1.0, 0.0, 0.0).is_none());
    }
}
