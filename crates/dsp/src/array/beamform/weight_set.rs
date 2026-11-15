use num_complex::Complex;

use super::ZERO;
use crate::math::linalg::{MAX_ORDER, norm_sqr};

/// Weights as applied to the lanes, `y = sum(w * x)`: the conjugate of the textbook `w^H x`.
#[derive(Clone, Debug, PartialEq)]
pub struct WeightSet {
    n: usize,
    w: [Complex<f32>; MAX_ORDER],
}

impl WeightSet {
    #[must_use]
    pub const fn zeros(n: usize) -> Self {
        Self {
            n: if n < MAX_ORDER { n } else { MAX_ORDER },
            w: [ZERO; MAX_ORDER],
        }
    }

    #[must_use]
    pub fn unit(n: usize, lane: usize) -> Self {
        let mut set = Self::zeros(n);
        if lane < set.n {
            set.w[lane] = Complex::new(1.0, 0.0);
        }
        set
    }

    #[must_use]
    pub fn from_textbook(textbook: &[Complex<f32>]) -> Self {
        let mut set = Self::zeros(textbook.len());
        for (applied, value) in set.w.iter_mut().zip(textbook) {
            *applied = value.conj();
        }
        set
    }

    #[must_use]
    pub fn as_slice(&self) -> &[Complex<f32>] {
        &self.w[..self.n]
    }

    pub fn as_mut_slice(&mut self) -> &mut [Complex<f32>] {
        &mut self.w[..self.n]
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.n
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.n == 0
    }

    pub fn textbook(&self, out: &mut [Complex<f32>]) {
        for (value, applied) in out.iter_mut().zip(self.as_slice()) {
            *value = applied.conj();
        }
    }

    #[must_use]
    pub fn response(&self, vector: &[Complex<f32>]) -> Complex<f32> {
        self.as_slice().iter().zip(vector).map(|(w, a)| w * a).sum()
    }

    pub fn align_phase_to(&mut self, previous: &Self) {
        if previous.n != self.n {
            return;
        }
        let overlap: Complex<f32> = self
            .as_slice()
            .iter()
            .zip(previous.as_slice())
            .map(|(new, old)| new.conj() * old)
            .sum();
        let magnitude = overlap.norm();
        if !(magnitude.is_finite() && magnitude > f32::MIN_POSITIVE) {
            return;
        }
        let turn = overlap / magnitude;
        for value in self.as_mut_slice() {
            *value *= turn;
        }
    }

    #[must_use]
    pub fn is_finite(&self) -> bool {
        self.as_slice().iter().all(|value| value.is_finite())
    }

    #[must_use]
    pub fn norm_sqr(&self) -> f32 {
        norm_sqr(self.as_slice())
    }
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;

    use super::*;

    #[test]
    fn textbook_and_applied_weights_are_conjugates() {
        let textbook = [Complex::new(1.0f32, 2.0), Complex::new(-0.5, 0.25)];
        let set = WeightSet::from_textbook(&textbook);
        assert_eq!(
            set.as_slice(),
            [Complex::new(1.0, -2.0), Complex::new(-0.5, -0.25)]
        );
        let mut back = [Complex::new(0.0f32, 0.0); 2];
        set.textbook(&mut back);
        assert_eq!(back, textbook);
        assert_eq!(WeightSet::unit(3, 1).as_slice()[1], Complex::new(1.0, 0.0));
        assert_eq!(WeightSet::zeros(40).len(), MAX_ORDER);
    }

    #[test]
    fn align_phase_removes_a_common_rotation() {
        let old = WeightSet::from_textbook(&[Complex::new(1.0f32, 0.0), Complex::new(0.3, 0.4)]);
        let mut new = old.clone();
        let turn = Complex::from_polar(1.0f32, 2.1);
        for value in new.as_mut_slice() {
            *value *= turn;
        }
        new.align_phase_to(&old);
        for (a, b) in new.as_slice().iter().zip(old.as_slice()) {
            assert!((a - b).norm() < 1e-6);
        }
        let mut lone = WeightSet::zeros(2);
        lone.align_phase_to(&old);
        assert!(lone.is_finite());
    }
}
