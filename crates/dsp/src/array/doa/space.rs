use super::layout::Layout;
use super::{DoaConfig, DoaError, Estimator, MAX_ESPRIT_SOURCES, MAX_PEAKS, SourceCount, UlaSide};
use crate::array::covariance::{CovarianceError, PhaseMode};

pub const NEEDS_LINE_OR_CIRCLE: &str = "Needs a line or circle";
pub const ARRAY_CHANGED: &str = "Array changed";
pub const FB_NEEDS_SYMMETRY: &str = "FB needs a symmetric array";
pub const ELEVATION_NEEDS_2D: &str = "Elevation needs a 2D array";
pub const ELEVATION_NEEDS_GRID: &str = "Elevation needs a grid method";
pub const SIDE_NEEDS_LINE: &str = "Side needs a line";
pub const TOO_MANY_SOURCES: &str = "Too many sources";
pub const TOO_FEW_ELEMENTS: &str = "Too few elements";
pub const PEAKS_OUT_OF_RANGE: &str = "Peaks out of range";
pub const LOADING_OUT_OF_RANGE: &str = "Loading out of range";
pub const SQUELCH_OUT_OF_RANGE: &str = "Squelch out of range";
pub const PEAK_RANGE_OUT_OF_RANGE: &str = "Peak range out of range";
pub const TABLE_NEEDS_GRID: &str = "Table needs a grid method";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Space {
    Element,
    Line { subarrays: usize },
    Modes,
    Vandermonde { subarrays: usize },
}

impl Space {
    pub fn order(self, elements: usize, phase: Option<&PhaseMode>) -> usize {
        match self {
            Self::Element => elements,
            Self::Line { subarrays } => (elements + 1).saturating_sub(subarrays),
            Self::Modes => phase.map_or(0, PhaseMode::virtual_len),
            Self::Vandermonde { subarrays } => phase.map_or(0, |phase| {
                (phase.vandermonde_len() + 1).saturating_sub(subarrays)
            }),
        }
    }
}

pub(super) const fn unsupported(text: &'static str) -> DoaError {
    DoaError::Unsupported(text)
}

pub(super) fn plan(
    config: &DoaConfig,
    layout: &Layout,
    phase: Option<&PhaseMode>,
) -> Result<Space, DoaError> {
    check_scalars(config)?;
    let structured = structured(config);
    if config.elevation.is_some() {
        if layout.collinear {
            return Err(unsupported(ELEVATION_NEEDS_2D));
        }
        if structured {
            return Err(unsupported(ELEVATION_NEEDS_GRID));
        }
    }
    if config.ula_side != UlaSide::Both && layout.axis_deg.is_none() {
        return Err(unsupported(SIDE_NEEDS_LINE));
    }
    let space = choose(config, layout, phase, structured)?;
    let order = space.order(layout.elements, phase);
    let fixed = match config.sources {
        SourceCount::Fixed(d) => Some(usize::from(d)),
        SourceCount::Auto => None,
    };
    if order < 2 || (config.smoothing > 0 && fixed.is_some_and(|d| d >= order)) {
        return Err(unsupported(TOO_FEW_ELEMENTS));
    }
    let esprit_limit = config.estimator == Estimator::Esprit;
    if fixed.is_some_and(|d| d >= order || (esprit_limit && d > MAX_ESPRIT_SOURCES)) {
        return Err(unsupported(TOO_MANY_SOURCES));
    }
    Ok(space)
}

pub(super) fn check_table(config: &DoaConfig, measured: bool) -> Result<(), DoaError> {
    if !measured {
        return Ok(());
    }
    if structured(config) {
        return Err(unsupported(TABLE_NEEDS_GRID));
    }
    if config.forward_backward {
        return Err(unsupported(FB_NEEDS_SYMMETRY));
    }
    Ok(())
}

const fn structured(config: &DoaConfig) -> bool {
    matches!(config.estimator, Estimator::RootMusic | Estimator::Esprit) || config.smoothing > 0
}

fn check_scalars(config: &DoaConfig) -> Result<(), DoaError> {
    if !(1..=MAX_PEAKS).contains(&usize::from(config.max_peaks)) {
        return Err(unsupported(PEAKS_OUT_OF_RANGE));
    }
    if !(config.loading.is_finite() && config.loading >= 0.0) {
        return Err(unsupported(LOADING_OUT_OF_RANGE));
    }
    if !(config.peak_range_db.is_finite() && config.peak_range_db >= 0.0) {
        return Err(unsupported(PEAK_RANGE_OUT_OF_RANGE));
    }
    let squelch_ok = config.squelch.is_none_or(|squelch| {
        squelch.open_db.is_finite()
            && squelch.hysteresis_db.is_finite()
            && squelch.hysteresis_db >= 0.0
    });
    if !squelch_ok {
        return Err(unsupported(SQUELCH_OUT_OF_RANGE));
    }
    Ok(())
}

fn choose(
    config: &DoaConfig,
    layout: &Layout,
    phase: Option<&PhaseMode>,
    structured: bool,
) -> Result<Space, DoaError> {
    let subarrays = usize::from(config.smoothing) + 1;
    if !structured {
        if config.forward_backward && layout.antipodes.is_none() {
            return Err(unsupported(FB_NEEDS_SYMMETRY));
        }
        return Ok(Space::Element);
    }
    if layout.line.is_some() {
        return Ok(Space::Line { subarrays });
    }
    let Some(phase) = phase else {
        return Err(unsupported(NEEDS_LINE_OR_CIRCLE));
    };
    if config.estimator == Estimator::RootMusic && config.smoothing == 0 {
        return Ok(Space::Modes);
    }
    if phase.vandermonde_modes().is_none() {
        return Err(CovarianceError::BesselNull.into());
    }
    Ok(Space::Vandermonde { subarrays })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::doa::testing::uca_layout as uca;
    use crate::array::manifold::{AzimuthSpan, Geometry, Vec3};

    fn config(estimator: Estimator) -> DoaConfig {
        DoaConfig {
            estimator,
            ..DoaConfig::default()
        }
    }

    #[test]
    fn plans_follow_the_geometry() {
        let (layout, phase) = uca(8, 2.0);
        let phase = Some(&phase);
        assert_eq!(
            plan(&config(Estimator::Music), &layout, phase),
            Ok(Space::Element)
        );
        assert_eq!(
            plan(&config(Estimator::RootMusic), &layout, phase),
            Ok(Space::Modes)
        );
        assert_eq!(
            plan(&config(Estimator::Esprit), &layout, phase),
            Ok(Space::Vandermonde { subarrays: 1 })
        );
        let smoothed = DoaConfig {
            smoothing: 2,
            ..config(Estimator::Music)
        };
        let space = plan(&smoothed, &layout, phase).unwrap();
        assert_eq!(space, Space::Vandermonde { subarrays: 3 });
        assert_eq!(space.order(8, phase), 5);
        let line = Layout::of(&Geometry::ula(0.3, 6, 180.0).unwrap());
        assert_eq!(line.axis_deg, Some(180.0));
        assert_eq!(
            plan(&smoothed, &line, None),
            Ok(Space::Line { subarrays: 3 })
        );
        assert_eq!(line.grid_spec(&smoothed).span, AzimuthSpan::Full);
        let front = DoaConfig {
            ula_side: UlaSide::Front,
            ..smoothed
        };
        assert_eq!(
            line.grid_spec(&front).span,
            AzimuthSpan::Half { centre_deg: 90.0 }
        );
    }

    #[test]
    fn refusals_carry_the_node_texts() {
        let triangle = Layout::of(
            &Geometry::explicit(&[
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.3, 0.0, 0.0),
                Vec3::new(0.0, 0.4, 0.0),
            ])
            .unwrap(),
        );
        assert_eq!(
            plan(&config(Estimator::RootMusic), &triangle, None),
            Err(DoaError::Unsupported(NEEDS_LINE_OR_CIRCLE))
        );
        let fb = DoaConfig {
            forward_backward: true,
            ..config(Estimator::Music)
        };
        assert_eq!(
            plan(&fb, &triangle, None),
            Err(DoaError::Unsupported(FB_NEEDS_SYMMETRY))
        );
        let side = DoaConfig {
            ula_side: UlaSide::Back,
            ..config(Estimator::Music)
        };
        assert_eq!(
            plan(&side, &triangle, None),
            Err(DoaError::Unsupported(SIDE_NEEDS_LINE))
        );
        let line = Layout::of(&Geometry::ula(0.3, 4, 90.0).unwrap());
        let many = DoaConfig {
            sources: SourceCount::Fixed(4),
            ..config(Estimator::Music)
        };
        assert_eq!(
            plan(&many, &line, None),
            Err(DoaError::Unsupported(TOO_MANY_SOURCES))
        );
        let deep = DoaConfig {
            smoothing: 3,
            ..config(Estimator::Music)
        };
        assert_eq!(
            plan(&deep, &line, None),
            Err(DoaError::Unsupported(TOO_FEW_ELEMENTS))
        );
        let crowded = DoaConfig {
            smoothing: 1,
            sources: SourceCount::Fixed(3),
            ..config(Estimator::Music)
        };
        assert_eq!(
            plan(&crowded, &line, None),
            Err(DoaError::Unsupported(TOO_FEW_ELEMENTS))
        );
        let (circle, phase) = uca(5, 2.0);
        let raised = DoaConfig {
            elevation: Some(crate::array::manifold::ElevationSpan::default()),
            ..config(Estimator::Esprit)
        };
        assert_eq!(
            plan(&raised, &circle, Some(&phase)),
            Err(DoaError::Unsupported(ELEVATION_NEEDS_GRID))
        );
        assert_eq!(
            plan(&raised, &line, None),
            Err(DoaError::Unsupported(ELEVATION_NEEDS_2D))
        );
        let peaks = DoaConfig {
            max_peaks: 0,
            ..config(Estimator::Music)
        };
        assert_eq!(
            plan(&peaks, &line, None),
            Err(DoaError::Unsupported(PEAKS_OUT_OF_RANGE))
        );
        let (null, phase) = uca(5, 2.405);
        assert_eq!(
            plan(&config(Estimator::Esprit), &null, Some(&phase)),
            Err(DoaError::Covariance(CovarianceError::BesselNull))
        );
        assert_eq!(
            plan(&config(Estimator::RootMusic), &null, Some(&phase)),
            Ok(Space::Modes)
        );
    }
}
