/// Weighted least-squares line through `(x, y)` points.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct LineFit {
    weight: f64,
    x: f64,
    y: f64,
    xx: f64,
    xy: f64,
}

impl LineFit {
    pub(super) fn add(&mut self, weight: f64, x: f64, y: f64) {
        self.weight += weight;
        self.x += weight * x;
        self.y += weight * y;
        self.xx += weight * x * x;
        self.xy += weight * x * y;
    }

    /// Intercept and slope. With no weight the line is zero; with every point on one `x` it
    /// is flat through their mean.
    #[must_use]
    pub(super) fn solve(&self) -> (f64, f64) {
        if self.weight <= f64::MIN_POSITIVE {
            return (0.0, 0.0);
        }
        let determinant = self.weight * self.xx - self.x * self.x;
        if determinant.abs() <= f64::MIN_POSITIVE {
            return (self.y / self.weight, 0.0);
        }
        let slope = (self.weight * self.xy - self.x * self.y) / determinant;
        ((self.y - slope * self.x) / self.weight, slope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_recovered_and_degenerate_points_stay_finite() {
        let mut fit = LineFit::default();
        assert_eq!(fit.solve(), (0.0, 0.0));
        for x in [-1.0, 0.5, 2.0] {
            fit.add(2.0, x, 3.0 - 0.5 * x);
        }
        let (intercept, slope) = fit.solve();
        assert!((intercept - 3.0).abs() < 1e-12);
        assert!((slope + 0.5).abs() < 1e-12);
        let mut flat = LineFit::default();
        flat.add(1.0, 4.0, 1.0);
        flat.add(1.0, 4.0, 3.0);
        assert_eq!(flat.solve(), (2.0, 0.0));
    }
}
