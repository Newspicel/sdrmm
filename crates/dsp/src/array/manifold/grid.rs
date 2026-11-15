mod spec;

use num_complex::Complex;

use super::{Direction, Manifold, ManifoldError};
use crate::math::special::norm_deg;

use spec::shape_of;
pub use spec::{AzimuthSpan, ElevationSpan, GridSpec, MAX_GRID_POINTS};

#[derive(Clone, Debug)]
pub struct SteeringGrid {
    elements: usize,
    spec: GridSpec,
    azimuths: usize,
    elevations: usize,
    freq_hz: f64,
    vectors: Vec<Complex<f32>>,
}

impl SteeringGrid {
    pub fn new(manifold: &Manifold, spec: GridSpec, freq_hz: f64) -> Result<Self, ManifoldError> {
        let (azimuths, elevations) = shape_of(spec)?;
        if spec.elevation.is_some() && manifold.geometry().is_collinear() {
            return Err(ManifoldError::ElevationOnLine);
        }
        let points = azimuths * elevations;
        let elements = manifold.len();
        let mut grid = Self {
            elements,
            spec,
            azimuths,
            elevations,
            freq_hz,
            vectors: vec![Complex::new(0.0, 0.0); points * elements],
        };
        grid.rebuild(manifold, freq_hz)?;
        Ok(grid)
    }

    pub fn rebuild(&mut self, manifold: &Manifold, freq_hz: f64) -> Result<(), ManifoldError> {
        if !(freq_hz.is_finite() && freq_hz > 0.0) {
            return Err(ManifoldError::Frequency);
        }
        if manifold.len() != self.elements {
            return Err(ManifoldError::Count(manifold.len()));
        }
        self.freq_hz = freq_hz;
        let n = self.elements;
        for point in 0..self.points() {
            let direction = self.direction(point);
            manifold.steer(
                freq_hz,
                direction,
                &mut self.vectors[point * n..(point + 1) * n],
            );
        }
        Ok(())
    }

    pub fn respan(&mut self, manifold: &Manifold, span: AzimuthSpan) -> Result<(), ManifoldError> {
        let spec = GridSpec { span, ..self.spec };
        if shape_of(spec)? != (self.azimuths, self.elevations) {
            return Err(ManifoldError::GridStep);
        }
        let previous = self.spec;
        self.spec = spec;
        self.rebuild(manifold, self.freq_hz).inspect_err(|_| {
            self.spec = previous;
        })
    }

    #[must_use]
    pub const fn points(&self) -> usize {
        self.azimuths * self.elevations
    }

    #[must_use]
    pub const fn azimuths(&self) -> usize {
        self.azimuths
    }

    #[must_use]
    pub const fn elevations(&self) -> usize {
        self.elevations
    }

    #[must_use]
    pub const fn elements(&self) -> usize {
        self.elements
    }

    #[must_use]
    pub const fn wraps(&self) -> bool {
        matches!(self.spec.span, AzimuthSpan::Full)
    }

    #[must_use]
    pub const fn spec(&self) -> GridSpec {
        self.spec
    }

    #[must_use]
    pub const fn freq_hz(&self) -> f64 {
        self.freq_hz
    }

    #[must_use]
    pub fn azimuth_step_deg(&self) -> f64 {
        match self.spec.span {
            AzimuthSpan::Full => 360.0 / self.azimuths as f64,
            AzimuthSpan::Half { .. } => 180.0 / (self.azimuths - 1) as f64,
        }
    }

    #[must_use]
    pub fn direction(&self, point: usize) -> Direction {
        let azimuth = point % self.azimuths;
        let elevation = point / self.azimuths;
        let start = match self.spec.span {
            AzimuthSpan::Full => 0.0,
            AzimuthSpan::Half { centre_deg } => centre_deg - 90.0,
        };
        let elevation_deg = self.spec.elevation.map_or(0.0, |span| {
            if self.elevations > 1 {
                let step = (span.max_deg - span.min_deg) / (self.elevations - 1) as f64;
                span.min_deg + elevation as f64 * step
            } else {
                span.min_deg
            }
        });
        Direction::new(
            norm_deg(start + azimuth as f64 * self.azimuth_step_deg()),
            elevation_deg,
        )
    }

    #[must_use]
    pub fn vector(&self, point: usize) -> &[Complex<f32>] {
        &self.vectors[point * self.elements..(point + 1) * self.elements]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        array::manifold::{Geometry, Vec3},
        testutil,
    };

    fn kraken() -> Manifold {
        Manifold::ideal(testutil::kraken())
    }

    #[test]
    fn grid_covers_full_circle_and_half_plane() {
        let full = SteeringGrid::new(&kraken(), GridSpec::ring(1.0), 433.92e6).unwrap();
        assert_eq!(full.points(), 360);
        assert!(full.wraps());
        assert_eq!(full.direction(0), Direction::horizon(0.0));
        assert!((full.direction(359).azimuth_deg - 359.0).abs() < 1e-9);
        let spec = GridSpec {
            azimuth_step_deg: 1.0,
            span: AzimuthSpan::Half { centre_deg: 0.0 },
            elevation: None,
        };
        let half = SteeringGrid::new(&kraken(), spec, 433.92e6).unwrap();
        assert_eq!(half.points(), 181);
        assert!(!half.wraps());
        assert!((half.direction(0).azimuth_deg - 270.0).abs() < 1e-9);
        assert!((half.direction(90).azimuth_deg).abs() < 1e-9);
        assert!((half.direction(180).azimuth_deg - 90.0).abs() < 1e-9);
    }

    #[test]
    fn respan_moves_a_half_plane_in_place() {
        let manifold = kraken();
        let spec = GridSpec {
            azimuth_step_deg: 1.0,
            span: AzimuthSpan::Half { centre_deg: 0.0 },
            elevation: None,
        };
        let mut grid = SteeringGrid::new(&manifold, spec, 433.92e6).unwrap();
        grid.respan(&manifold, AzimuthSpan::Half { centre_deg: 180.0 })
            .unwrap();
        assert!((grid.direction(0).azimuth_deg - 90.0).abs() < 1e-9);
        let mut expected = [Complex::new(0.0f32, 0.0); 5];
        manifold.steer(433.92e6, Direction::horizon(90.0), &mut expected);
        assert_eq!(grid.vector(0), &expected);
        assert_eq!(
            grid.respan(&manifold, AzimuthSpan::Full),
            Err(ManifoldError::GridStep)
        );
        assert_eq!(grid.spec().span, AzimuthSpan::Half { centre_deg: 180.0 });
    }

    #[test]
    fn grid_vectors_follow_the_manifold_and_elevation_rows() {
        let manifold = kraken();
        let spec = GridSpec {
            azimuth_step_deg: 10.0,
            span: AzimuthSpan::Full,
            elevation: Some(ElevationSpan::default()),
        };
        let mut grid = SteeringGrid::new(&manifold, spec, 433.92e6).unwrap();
        assert_eq!(grid.azimuths(), 36);
        assert_eq!(grid.elevations(), 46);
        let point = 3 * 36 + 7;
        assert_eq!(grid.direction(point), Direction::new(70.0, 6.0));
        let mut expected = [Complex::new(0.0f32, 0.0); 5];
        manifold.steer(433.92e6, Direction::new(70.0, 6.0), &mut expected);
        assert_eq!(grid.vector(point), &expected);
        grid.rebuild(&manifold, 144e6).unwrap();
        manifold.steer(144e6, Direction::new(70.0, 6.0), &mut expected);
        assert_eq!(grid.vector(point), &expected);
        assert_eq!(grid.freq_hz(), 144e6);
    }

    #[test]
    fn invalid_grids_are_refused() {
        let manifold = kraken();
        assert_eq!(
            SteeringGrid::new(&manifold, GridSpec::ring(0.0), 1e8).unwrap_err(),
            ManifoldError::GridStep
        );
        assert_eq!(
            SteeringGrid::new(&manifold, GridSpec::ring(0.001), 1e8).unwrap_err(),
            ManifoldError::GridTooLarge(360_000)
        );
        assert_eq!(
            SteeringGrid::new(&manifold, GridSpec::ring(1.0), -1.0).unwrap_err(),
            ManifoldError::Frequency
        );
        let line = Manifold::ideal(Geometry::ula(0.5, 4, 90.0).unwrap());
        let spec = GridSpec {
            elevation: Some(ElevationSpan::default()),
            ..GridSpec::ring(1.0)
        };
        assert_eq!(
            SteeringGrid::new(&line, spec, 1e8).unwrap_err(),
            ManifoldError::ElevationOnLine
        );
        let upright = Manifold::ideal(
            Geometry::explicit(&[Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.5)]).unwrap(),
        );
        assert_eq!(
            SteeringGrid::new(&upright, spec, 1e8).unwrap_err(),
            ManifoldError::ElevationOnLine
        );
        let mut grid = SteeringGrid::new(&manifold, GridSpec::ring(1.0), 1e8).unwrap();
        assert_eq!(grid.rebuild(&line, 1e8), Err(ManifoldError::Count(4)));
    }
}
