mod alias;
mod direction;
mod geometry;
mod grid;
mod permutation;
mod steering;
mod table;
#[cfg(test)]
pub(crate) mod testing;
mod vec3;

use std::sync::Arc;

use num_complex::Complex;

pub use alias::{ALIAS_THRESHOLD, AliasReport, alias_check};
pub use direction::Direction;
pub use geometry::{Geometry, Shape, Winding};
pub use grid::{AzimuthSpan, ElevationSpan, GridSpec, MAX_GRID_POINTS, SteeringGrid};
pub use permutation::Permutation;
pub use steering::{elevation_rates, phase_rates, steer, wavelength_m, wavenumber};
pub use table::ManifoldTable;
pub use vec3::Vec3;

use crate::{math::complex::widen, math::special::wrap_deg};

pub const LIGHT_SPEED_M_S: f64 = 299_792_458.0;
pub const MAX_ELEMENTS: usize = crate::math::linalg::MAX_ORDER;
pub const MAX_EXTENT_M: f64 = 100.0;

const TABLE_RATE_STEP_DEG: f64 = 0.5;
const ZENITH_DEG: f64 = 90.0;

#[derive(Clone, Copy, Debug, PartialEq, thiserror::Error)]
pub enum ManifoldError {
    #[error("an array needs 2 to 16 elements, not {0}")]
    Count(usize),
    #[error("element spacing must be positive and finite")]
    Spacing,
    #[error("element positions must be finite and within 100 m")]
    Position,
    #[error("two elements share a position")]
    Coincident,
    #[error("frequency must be positive and finite")]
    Frequency,
    #[error("grid would hold {0} points")]
    GridTooLarge(usize),
    #[error("grid step must be positive and finite")]
    GridStep,
    #[error("measured table does not cover {0} Hz")]
    OutOfTable(f64),
    #[error("measured table shape is inconsistent")]
    Table,
    #[error("elevation needs a 2D array")]
    ElevationOnLine,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Manifold {
    geometry: Geometry,
    table: Option<Arc<ManifoldTable>>,
}

impl Manifold {
    #[must_use]
    pub const fn ideal(geometry: Geometry) -> Self {
        Self {
            geometry,
            table: None,
        }
    }

    pub fn measured(geometry: Geometry, table: Arc<ManifoldTable>) -> Result<Self, ManifoldError> {
        if table.elements() != geometry.len() {
            return Err(ManifoldError::Table);
        }
        Ok(Self {
            geometry,
            table: Some(table),
        })
    }

    #[must_use]
    pub const fn geometry(&self) -> &Geometry {
        &self.geometry
    }

    #[must_use]
    pub fn table(&self) -> Option<&ManifoldTable> {
        self.table.as_deref()
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.geometry.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.geometry.is_empty()
    }

    #[must_use]
    pub fn uses_table_at(&self, freq_hz: f64) -> bool {
        self.covering_table(freq_hz).is_some()
    }

    pub fn steer(&self, freq_hz: f64, direction: Direction, out: &mut [Complex<f32>]) {
        if let Some(table) = self.covering_table(freq_hz)
            && table.response(freq_hz, direction, out).is_ok()
        {
            return;
        }
        steer(self.geometry.positions(), freq_hz, direction, out);
    }

    pub fn phase_rates(&self, freq_hz: f64, direction: Direction, out: &mut [f64]) {
        let step = Direction::new(TABLE_RATE_STEP_DEG, 0.0);
        if !self.table_rates(freq_hz, direction, step, out) {
            phase_rates(self.geometry.positions(), freq_hz, direction, out);
        }
    }

    pub fn elevation_rates(&self, freq_hz: f64, direction: Direction, out: &mut [f64]) {
        let step = Direction::new(0.0, TABLE_RATE_STEP_DEG);
        if !self.table_rates(freq_hz, direction, step, out) {
            elevation_rates(self.geometry.positions(), freq_hz, direction, out);
        }
    }

    fn covering_table(&self, freq_hz: f64) -> Option<&ManifoldTable> {
        self.table.as_deref().filter(|table| table.covers(freq_hz))
    }

    fn table_rates(
        &self,
        freq_hz: f64,
        direction: Direction,
        step: Direction,
        out: &mut [f64],
    ) -> bool {
        let Some(table) = self.covering_table(freq_hz) else {
            return false;
        };
        let n = self.len();
        let mut ahead = [Complex::new(0.0f32, 0.0); MAX_ELEMENTS];
        let mut behind = [Complex::new(0.0f32, 0.0); MAX_ELEMENTS];
        let back = Direction::new(-step.azimuth_deg, -step.elevation_deg);
        if table
            .response(freq_hz, direction.offset(step), &mut ahead[..n])
            .is_err()
            || table
                .response(freq_hz, direction.offset(back), &mut behind[..n])
                .is_err()
        {
            return false;
        }
        let common: Complex<f64> = ahead[..n]
            .iter()
            .zip(&behind[..n])
            .map(|(a, b)| widen(*a * b.conj()))
            .sum();
        let shift = common.arg();
        let span = 2.0 * (step.azimuth_deg + step.elevation_deg).to_radians();
        for ((rate, a), b) in out.iter_mut().zip(&ahead[..n]).zip(&behind[..n]) {
            let turn = widen(*a * b.conj()).arg() - shift;
            *rate = wrap_deg(turn.to_degrees()).to_radians() / span;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::kraken;

    pub(super) fn ideal_table(geometry: &Geometry, freqs: &[f64], step: f64) -> ManifoldTable {
        let n = geometry.len();
        let azimuths = (360.0 / step).round() as usize;
        let mut data = vec![Complex::new(0.0f32, 0.0); freqs.len() * azimuths * n];
        for (f, &freq) in freqs.iter().enumerate() {
            for a in 0..azimuths {
                let at = (f * azimuths + a) * n;
                steer(
                    geometry.positions(),
                    freq,
                    Direction::horizon(a as f64 * step),
                    &mut data[at..at + n],
                );
            }
        }
        ManifoldTable::new(n, freqs.to_vec(), step, vec![0.0], data).unwrap()
    }

    fn centred(values: &[f64]) -> Vec<f64> {
        let mean = values.iter().sum::<f64>() / values.len() as f64;
        values.iter().map(|v| v - mean).collect()
    }

    #[test]
    fn a_measured_manifold_steers_through_its_table_inside_its_range() {
        let geometry = kraken();
        let table = Arc::new(ideal_table(&geometry, &[430e6, 440e6], 5.0));
        let manifold = Manifold::measured(geometry.clone(), table).unwrap();
        assert!(manifold.uses_table_at(435e6));
        assert!(!manifold.uses_table_at(450e6));
        let direction = Direction::horizon(72.5);
        let mut from_table = [Complex::new(0.0f32, 0.0); 5];
        let mut ideal = [Complex::new(0.0f32, 0.0); 5];
        manifold.steer(435e6, direction, &mut from_table);
        steer(geometry.positions(), 435e6, direction, &mut ideal);
        let matched: Complex<f32> = from_table
            .iter()
            .zip(&ideal)
            .map(|(a, b)| a.conj() * b)
            .sum();
        assert!(matched.norm() / 5.0 > 0.999);
        let mut rates = [0.0f64; 5];
        let mut exact = [0.0f64; 5];
        manifold.phase_rates(435e6, direction, &mut rates);
        phase_rates(geometry.positions(), 435e6, direction, &mut exact);
        for (a, b) in centred(&rates).iter().zip(centred(&exact).iter()) {
            assert!((a - b).abs() < 0.05 * b.abs().max(1.0), "{a} vs {b}");
        }
        let short = Geometry::uca(0.35, 4, 0.0, Winding::Clockwise).unwrap();
        let table = Arc::new(ideal_table(&geometry, &[430e6], 5.0));
        assert_eq!(
            Manifold::measured(short, table).unwrap_err(),
            ManifoldError::Table
        );
    }

    #[test]
    fn a_manifold_outside_its_table_falls_back_to_the_geometry() {
        let geometry = Geometry::ula(0.5, 3, 90.0).unwrap();
        let table = Arc::new(ideal_table(&geometry, &[300e6], 10.0));
        let manifold = Manifold::measured(geometry.clone(), table).unwrap();
        let mut out = [Complex::new(0.0f32, 0.0); 3];
        let mut ideal = [Complex::new(0.0f32, 0.0); 3];
        manifold.steer(100e6, Direction::horizon(30.0), &mut out);
        steer(
            geometry.positions(),
            100e6,
            Direction::horizon(30.0),
            &mut ideal,
        );
        assert_eq!(out, ideal);
        assert_eq!(manifold.len(), 3);
        assert!(manifold.table().is_some());
    }
}
