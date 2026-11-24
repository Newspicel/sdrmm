use num_complex::Complex;

/// Share of the step a coordinate may miss its lattice line by and still sit on it.
const LATTICE_TOLERANCE: f64 = 1e-4;

/// A square table on an evenly spaced, axis-aligned lattice. The nearest point is found by
/// rounding each axis, so slicing costs the same at 65536 points as at four.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Grid {
    origin: Complex<f32>,
    step: f32,
    side: usize,
    index_at: Vec<u32>,
}

impl Grid {
    pub(super) fn detect(points: &[Complex<f32>]) -> Option<Self> {
        let side = points.len().isqrt();
        if side < 2 || side * side != points.len() {
            return None;
        }
        let (re_lo, re_hi) = extent(points.iter().map(|p| f64::from(p.re)));
        let (im_lo, im_hi) = extent(points.iter().map(|p| f64::from(p.im)));
        let lines = (side - 1) as f64;
        let step = (re_hi - re_lo) / lines;
        let im_step = (im_hi - im_lo) / lines;
        if !(step > 0.0 && (im_step - step).abs() <= LATTICE_TOLERANCE * step) {
            return None;
        }
        let mut index_at = vec![u32::MAX; points.len()];
        for (index, p) in points.iter().enumerate() {
            let col = lattice_line((f64::from(p.re) - re_lo) / step, side)?;
            let row = lattice_line((f64::from(p.im) - im_lo) / step, side)?;
            let slot = &mut index_at[row * side + col];
            if *slot != u32::MAX {
                return None;
            }
            *slot = u32::try_from(index).ok()?;
        }
        Some(Self {
            origin: Complex::new(re_lo as f32, im_lo as f32),
            step: step as f32,
            side,
            index_at,
        })
    }

    #[must_use]
    pub(super) fn nearest_index(&self, y: Complex<f32>) -> usize {
        let col = self.cell(y.re - self.origin.re);
        let row = self.cell(y.im - self.origin.im);
        self.index_at[row * self.side + col] as usize
    }

    #[must_use]
    pub(super) fn step(&self) -> f32 {
        self.step
    }

    #[must_use]
    pub(super) fn side(&self) -> usize {
        self.side
    }

    /// The index of the point in column `col` and row `row`, counted from the lowest.
    #[must_use]
    pub(super) fn point_at(&self, col: usize, row: usize) -> usize {
        self.index_at[row * self.side + col] as usize
    }

    fn cell(&self, offset: f32) -> usize {
        (offset / self.step)
            .round()
            .clamp(0.0, (self.side - 1) as f32) as usize
    }
}

fn extent(values: impl Iterator<Item = f64>) -> (f64, f64) {
    values.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v), hi.max(v))
    })
}

/// The lattice line a coordinate in steps sits on, if it sits on one inside the grid.
fn lattice_line(steps: f64, side: usize) -> Option<usize> {
    let line = steps.round();
    let on_line = (steps - line).abs() <= LATTICE_TOLERANCE;
    (on_line && line >= 0.0 && line < side as f64).then_some(line as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constellation::tables;

    #[test]
    fn square_qam_and_qpsk_are_grids() {
        for table in [
            tables::qam_square(4).unwrap(),
            tables::qam_square(16).unwrap(),
            tables::qam_square(65_536).unwrap(),
        ] {
            assert!(
                Grid::detect(table.points()).is_some(),
                "{} points",
                table.len()
            );
        }
    }

    #[test]
    fn tables_off_a_square_lattice_are_not() {
        for table in [
            tables::pam(4).unwrap(),
            tables::psk(8).unwrap(),
            tables::qam_cross(32).unwrap(),
            tables::apsk16_dvbs2(2.57).unwrap(),
        ] {
            assert!(
                Grid::detect(table.points()).is_none(),
                "{} points",
                table.len()
            );
        }
    }

    #[test]
    fn a_repeated_point_is_not_a_grid() {
        let points = vec![
            Complex::new(-1.0, -1.0),
            Complex::new(1.0, -1.0),
            Complex::new(-1.0, 1.0),
            Complex::new(-1.0, 1.0),
        ];
        assert!(Grid::detect(&points).is_none());
    }
}
