use super::layout::Layout;
use super::space::Space;
use super::workspace::Workspace;
use super::{C32, Pseudospectrum};
use crate::array::covariance::PhaseMode;
use crate::array::manifold::{Direction, MAX_ELEMENTS, Manifold, SteeringGrid};

/// The chosen pseudospectrum, read at any direction through the working space.
pub(super) struct Probe<'a> {
    pub(super) pseudo: Pseudospectrum<'a>,
    pub(super) space: Space,
    pub(super) workspace: &'a Workspace,
    pub(super) layout: &'a Layout,
    pub(super) phase: Option<&'a PhaseMode>,
    pub(super) manifold: &'a Manifold,
    pub(super) freq_hz: f64,
}

impl Probe<'_> {
    pub(super) fn power(&self, direction: Direction) -> f32 {
        let n = self.manifold.len();
        let mut element = [C32::new(0.0, 0.0); MAX_ELEMENTS];
        if matches!(self.space, Space::Element | Space::Line { .. }) {
            self.manifold
                .steer(self.freq_hz, direction, &mut element[..n]);
        }
        self.evaluate(&element[..n], direction.azimuth_deg)
    }

    pub(super) fn at_point(&self, grid: &SteeringGrid, point: usize) -> f32 {
        let mut scratch = [C32::new(0.0, 0.0); MAX_ELEMENTS];
        match self.space {
            Space::Element => self.pseudo.power(grid.vector(point), &mut scratch),
            _ => self.evaluate(grid.vector(point), grid.direction(point).azimuth_deg),
        }
    }

    fn evaluate(&self, element: &[C32], azimuth_deg: f64) -> f32 {
        let mut working = [C32::new(0.0, 0.0); MAX_ELEMENTS];
        let mut scratch = [C32::new(0.0, 0.0); MAX_ELEMENTS];
        let m = self.workspace.steer(
            self.space,
            self.layout,
            self.phase,
            element,
            azimuth_deg,
            &mut working,
        );
        self.pseudo.power(&working[..m], &mut scratch)
    }
}
