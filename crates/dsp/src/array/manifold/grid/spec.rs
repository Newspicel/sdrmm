use crate::array::manifold::{ManifoldError, ZENITH_DEG};

pub const MAX_GRID_POINTS: usize = 65_536;

const DEFAULT_ELEVATION_STEP_DEG: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AzimuthSpan {
    Full,
    Half { centre_deg: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ElevationSpan {
    pub min_deg: f64,
    pub max_deg: f64,
    pub step_deg: f64,
}

impl Default for ElevationSpan {
    fn default() -> Self {
        Self {
            min_deg: 0.0,
            max_deg: ZENITH_DEG,
            step_deg: DEFAULT_ELEVATION_STEP_DEG,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSpec {
    pub azimuth_step_deg: f64,
    pub span: AzimuthSpan,
    pub elevation: Option<ElevationSpan>,
}

impl GridSpec {
    #[must_use]
    pub const fn ring(azimuth_step_deg: f64) -> Self {
        Self {
            azimuth_step_deg,
            span: AzimuthSpan::Full,
            elevation: None,
        }
    }
}

/// Azimuth and elevation counts of a grid, or why it cannot be built.
pub(super) fn shape_of(spec: GridSpec) -> Result<(usize, usize), ManifoldError> {
    let step = spec.azimuth_step_deg;
    if !(step.is_finite() && step > 0.0 && step <= 360.0) {
        return Err(ManifoldError::GridStep);
    }
    let azimuths = match spec.span {
        AzimuthSpan::Full => (360.0 / step).round().max(1.0),
        AzimuthSpan::Half { centre_deg } if centre_deg.is_finite() => {
            (180.0 / step).round().max(1.0) + 1.0
        }
        AzimuthSpan::Half { .. } => return Err(ManifoldError::GridStep),
    };
    let elevations = match spec.elevation {
        None => 1.0,
        Some(span) => {
            let valid = span.step_deg.is_finite()
                && span.step_deg > 0.0
                && (0.0..=ZENITH_DEG).contains(&span.min_deg)
                && (span.min_deg..=ZENITH_DEG).contains(&span.max_deg);
            if !valid {
                return Err(ManifoldError::GridStep);
            }
            ((span.max_deg - span.min_deg) / span.step_deg).round() + 1.0
        }
    };
    let points = azimuths * elevations;
    if points > MAX_GRID_POINTS as f64 {
        return Err(ManifoldError::GridTooLarge(
            points.min(usize::MAX as f64) as usize
        ));
    }
    Ok((azimuths as usize, elevations as usize))
}
