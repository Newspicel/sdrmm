/// Weighted least-squares line through `(x, y)` samples, accumulated one sample at a time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineFit {
    sw: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
}

impl LineFit {
    pub fn push(&mut self, x: f64, y: f64, weight: f64) {
        self.sw += weight;
        self.sx += weight * x;
        self.sy += weight * y;
        self.sxx += weight * x * x;
        self.sxy += weight * x * y;
    }

    #[must_use]
    pub fn weight(&self) -> f64 {
        self.sw
    }

    /// The intercept and slope. Samples too close to one abscissa, by `degenerate`, give the
    /// weighted mean and a flat line.
    #[must_use]
    pub fn line(&self, degenerate: f64) -> (f64, f64) {
        let det = self.sw * self.sxx - self.sx * self.sx;
        if det.abs() <= degenerate {
            return (self.sy / self.sw, 0.0);
        }
        (
            (self.sxx * self.sy - self.sx * self.sxy) / det,
            (self.sw * self.sxy - self.sx * self.sy) / det,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_weighted_line_is_recovered_and_a_point_cloud_is_flat() {
        let mut fit = LineFit::default();
        for x in [-2.0, 0.5, 3.0] {
            fit.push(x, 0.25 + 1.5 * x, 2.0);
        }
        let (intercept, slope) = fit.line(1e-12);
        assert!((intercept - 0.25).abs() < 1e-12 && (slope - 1.5).abs() < 1e-12);
        let mut stacked = LineFit::default();
        stacked.push(1.0, 2.0, 1.0);
        stacked.push(1.0, 4.0, 1.0);
        assert_eq!(stacked.line(1e-12), (3.0, 0.0));
    }
}
