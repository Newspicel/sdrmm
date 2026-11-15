use super::layout::Layout;
use super::space::{self, ARRAY_CHANGED, NEEDS_LINE_OR_CIRCLE, Space};
use super::workspace::Workspace;
use super::{
    Doa, DoaConfig, DoaError, RING_STEP_DEG, Reconfigure, Solvers, SourceCounter, Squelch,
};
use crate::array::covariance::{CovarianceError, PhaseMode};
use crate::array::manifold::{
    AliasReport, AzimuthSpan, Geometry, GridSpec, Manifold, ManifoldError, Shape, SteeringGrid,
    alias_check,
};
use crate::math::linalg::{CMat, LinalgError};

impl Doa {
    pub fn new(manifold: &Manifold, config: &DoaConfig, freq_hz: f64) -> Result<Self, DoaError> {
        if !(freq_hz.is_finite() && freq_hz > 0.0) {
            return Err(ManifoldError::Frequency.into());
        }
        let geometry = manifold.geometry();
        let layout = Layout::of(geometry);
        let (phase, phase_fault) = phase_modes(geometry, freq_hz);
        let plan = plan_space(config, &layout, phase.as_ref(), phase_fault)?;
        space::check_table(config, manifold.uses_table_at(freq_hz))?;
        let grid = SteeringGrid::new(manifold, layout.grid_spec(config), freq_hz)?;
        let ring = SteeringGrid::new(manifold, GridSpec::ring(RING_STEP_DEG), freq_hz)?;
        let alias = alias_check(&ring, geometry, freq_hz);
        let elements = geometry.len();
        Ok(Self {
            config: *config,
            freq_hz,
            spectrum: vec![0.0; grid.points()],
            manifold: manifold.clone(),
            layout,
            phase,
            phase_fault,
            plan: Ok(plan),
            grid,
            ring,
            alias,
            workspace: Workspace::new(elements)?,
            solvers: Solvers::new(elements)?,
            counter: SourceCounter::new(),
            squelch: Squelch::new(),
        })
    }

    pub fn configure(
        &mut self,
        manifold: &Manifold,
        config: &DoaConfig,
    ) -> Result<Reconfigure, DoaError> {
        if !self.same_array(manifold) {
            return Ok(Reconfigure::NeedsNew);
        }
        let plan = plan_space(config, &self.layout, self.phase.as_ref(), self.phase_fault)?;
        space::check_table(config, manifold.uses_table_at(self.freq_hz))?;
        let spec = self.layout.grid_spec(config);
        let current = self.grid.spec();
        let same_storage = spec.azimuth_step_deg == current.azimuth_step_deg
            && spec.elevation == current.elevation
            && matches!(
                (spec.span, current.span),
                (AzimuthSpan::Full, AzimuthSpan::Full)
                    | (AzimuthSpan::Half { .. }, AzimuthSpan::Half { .. })
            );
        if !same_storage {
            return Ok(Reconfigure::NeedsNew);
        }
        if spec.span != current.span {
            self.grid.respan(manifold, spec.span)?;
        }
        self.config = *config;
        self.plan = Ok(plan);
        self.reset();
        Ok(Reconfigure::InPlace)
    }

    pub fn retune(&mut self, manifold: &Manifold, freq_hz: f64) -> Result<AliasReport, DoaError> {
        if !(freq_hz.is_finite() && freq_hz > 0.0) {
            return Err(ManifoldError::Frequency.into());
        }
        if !self.same_array(manifold) {
            return Err(DoaError::Unsupported(ARRAY_CHANGED));
        }
        if let Some(phase) = self.phase.as_mut() {
            self.phase_fault = phase.retune(freq_hz).err();
        }
        self.grid.rebuild(manifold, freq_hz)?;
        self.ring.rebuild(manifold, freq_hz)?;
        self.freq_hz = freq_hz;
        self.alias = alias_check(&self.ring, manifold.geometry(), freq_hz);
        self.reset();
        self.plan = plan_space(
            &self.config,
            &self.layout,
            self.phase.as_ref(),
            self.phase_fault,
        )
        .and_then(|plan| {
            space::check_table(&self.config, manifold.uses_table_at(freq_hz)).map(|()| plan)
        });
        self.plan.map(|_| self.alias)
    }

    pub const fn reset(&mut self) {
        self.counter.reset();
        self.squelch.reset();
    }

    #[must_use]
    pub fn spectrum(&self) -> &[f32] {
        &self.spectrum
    }

    #[must_use]
    pub const fn grid(&self) -> &SteeringGrid {
        &self.grid
    }

    #[must_use]
    pub const fn alias(&self) -> &AliasReport {
        &self.alias
    }

    #[must_use]
    pub const fn config(&self) -> &DoaConfig {
        &self.config
    }

    #[must_use]
    pub const fn freq_hz(&self) -> f64 {
        self.freq_hz
    }

    #[must_use]
    pub fn mode_bias_deg(&self) -> Option<f32> {
        let uses_modes = matches!(self.plan, Ok(Space::Modes | Space::Vandermonde { .. }));
        usable(self.phase.as_ref(), self.phase_fault)
            .filter(|_| uses_modes)
            .map(PhaseMode::mode_bias_deg)
    }

    pub(super) fn same_array(&self, manifold: &Manifold) -> bool {
        let same_table = match (manifold.table(), self.manifold.table()) {
            (None, None) => true,
            (Some(given), Some(kept)) => std::ptr::eq(given, kept),
            _ => false,
        };
        same_table && manifold.geometry() == self.manifold.geometry()
    }

    pub(super) fn check_input(&self, manifold: &Manifold, r: &CMat) -> Result<(), DoaError> {
        if !self.same_array(manifold) {
            return Err(DoaError::Unsupported(ARRAY_CHANGED));
        }
        let n = self.manifold.len();
        if r.order() != n {
            return Err(LinalgError::Order(r.order()).into());
        }
        if !r.is_finite() {
            return Err(LinalgError::NonFinite.into());
        }
        let trace = r.trace_re();
        if trace.is_nan() || trace <= 0.0 {
            return Err(LinalgError::NotPositiveDefinite(0).into());
        }
        Ok(())
    }
}

fn phase_modes(geometry: &Geometry, freq_hz: f64) -> (Option<PhaseMode>, Option<CovarianceError>) {
    if !matches!(geometry.shape(), Shape::Uca { .. }) {
        return (None, None);
    }
    match PhaseMode::new(geometry, freq_hz) {
        Ok(phase) => (Some(phase), None),
        Err(CovarianceError::NotStructured) => (None, None),
        Err(fault) => (None, Some(fault)),
    }
}

pub(super) const fn usable(
    phase: Option<&PhaseMode>,
    fault: Option<CovarianceError>,
) -> Option<&PhaseMode> {
    match (phase, fault) {
        (Some(phase), None) => Some(phase),
        _ => None,
    }
}

fn plan_space(
    config: &DoaConfig,
    layout: &Layout,
    phase: Option<&PhaseMode>,
    fault: Option<CovarianceError>,
) -> Result<Space, DoaError> {
    match (space::plan(config, layout, usable(phase, fault)), fault) {
        (Err(DoaError::Unsupported(NEEDS_LINE_OR_CIRCLE)), Some(fault)) => Err(fault.into()),
        (plan, _) => plan,
    }
}
