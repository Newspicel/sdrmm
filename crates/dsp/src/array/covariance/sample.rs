use num_complex::Complex;

use super::{
    CovarianceError,
    hermitian::{set_mirrored, write_identity},
};
use crate::{
    math::complex::cross_sum,
    math::linalg::{CMat, LinalgError, MAX_ORDER},
};

pub struct SampleCovariance {
    order: usize,
    sum: [Complex<f64>; MAX_ORDER * MAX_ORDER],
    weight: f64,
    weight_sq: f64,
}

impl SampleCovariance {
    pub fn new(order: usize) -> Result<Self, CovarianceError> {
        if order == 0 || order > MAX_ORDER {
            return Err(LinalgError::Order(order).into());
        }
        Ok(Self {
            order,
            sum: [Complex::new(0.0, 0.0); MAX_ORDER * MAX_ORDER],
            weight: 0.0,
            weight_sq: 0.0,
        })
    }

    #[must_use]
    pub const fn order(&self) -> usize {
        self.order
    }

    pub fn reset(&mut self) {
        self.sum.fill(Complex::new(0.0, 0.0));
        self.weight = 0.0;
        self.weight_sq = 0.0;
    }

    pub fn decay(&mut self, factor: f32) {
        let factor = f64::from(factor);
        for value in &mut self.sum {
            *value *= factor;
        }
        self.weight *= factor;
        self.weight_sq *= factor * factor;
    }

    pub fn accumulate(&mut self, lanes: &[&[Complex<f32>]]) -> usize {
        if lanes.len() != self.order {
            return 0;
        }
        let len = lanes.iter().map(|lane| lane.len()).min().unwrap_or(0);
        let n = self.order;
        for (i, left) in lanes.iter().enumerate() {
            for (j, right) in lanes.iter().enumerate().skip(i) {
                self.sum[i * n + j] += cross_sum(&left[..len], &right[..len]);
            }
        }
        self.weight += len as f64;
        self.weight_sq += len as f64;
        len
    }

    #[must_use]
    pub const fn weight(&self) -> f64 {
        self.weight
    }

    #[must_use]
    pub fn effective_snapshots(&self) -> f64 {
        if self.weight_sq > 0.0 {
            self.weight * self.weight / self.weight_sq
        } else {
            0.0
        }
    }

    pub fn matrix(&self, out: &mut CMat) -> bool {
        let n = self.order;
        if out.resize(n).is_err() {
            return false;
        }
        if self.weight <= 0.0 {
            write_identity(out);
            return false;
        }
        let scale = 1.0 / self.weight;
        for i in 0..n {
            for j in i..n {
                set_mirrored(out, i, j, self.sum[i * n + j] * scale);
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::covariance::testing::random_lanes;
    use crate::math::complex::widen;

    #[test]
    fn upper_triangle_accumulation_matches_the_outer_product() {
        let lanes = random_lanes(5, 1003, 11);
        let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        let mut covariance = SampleCovariance::new(5).unwrap();
        assert_eq!(covariance.accumulate(&views[..3]), 0);
        assert_eq!(covariance.accumulate(&views), 1003);
        let mut matrix = CMat::zeros(1).unwrap();
        assert!(covariance.matrix(&mut matrix));
        for i in 0..5 {
            for j in 0..5 {
                let want: Complex<f64> = lanes[i]
                    .iter()
                    .zip(&lanes[j])
                    .map(|(a, b)| widen(a * b.conj()))
                    .sum::<Complex<f64>>()
                    / 1003.0;
                let got = widen(matrix.get(i, j));
                assert!(
                    (got - want).norm() <= 1e-4 * want.norm().max(1e-3),
                    "{i},{j}"
                );
            }
        }
    }

    #[test]
    fn effective_snapshots_follow_decay() {
        let lanes = random_lanes(2, 1000, 3);
        let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        let mut covariance = SampleCovariance::new(2).unwrap();
        covariance.accumulate(&views);
        assert!((covariance.effective_snapshots() - 1000.0).abs() < 1e-9);
        covariance.decay(0.5);
        covariance.accumulate(&views);
        assert!((covariance.weight() - 1500.0).abs() < 1e-9);
        assert!((covariance.effective_snapshots() - 1800.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_average_reports_the_identity() {
        let mut covariance = SampleCovariance::new(3).unwrap();
        let mut matrix = CMat::zeros(1).unwrap();
        assert!(!covariance.matrix(&mut matrix));
        assert_eq!(matrix, CMat::identity(3).unwrap());
        assert_eq!(covariance.effective_snapshots(), 0.0);
        let lanes = random_lanes(3, 16, 5);
        let views: Vec<&[Complex<f32>]> = lanes.iter().map(Vec::as_slice).collect();
        covariance.accumulate(&views);
        covariance.reset();
        assert!(!covariance.matrix(&mut matrix));
        assert!(SampleCovariance::new(0).is_err());
        assert!(SampleCovariance::new(17).is_err());
    }
}
