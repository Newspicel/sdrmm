use num_complex::Complex;

use super::{BeamError, MAX_CONSTRAINTS, NULL_OVERLAP_LIMIT, ZERO};
use crate::math::linalg::{LinalgError, MAX_ORDER, inner, norm_sqr};

#[derive(Clone, Debug, PartialEq)]
pub struct Constraints {
    n: usize,
    count: usize,
    vectors: [[Complex<f32>; MAX_ORDER]; MAX_CONSTRAINTS],
    response: [Complex<f32>; MAX_CONSTRAINTS],
}

impl Constraints {
    #[must_use]
    pub const fn new(n: usize) -> Self {
        Self {
            n: if n < MAX_ORDER { n } else { MAX_ORDER },
            count: 0,
            vectors: [[ZERO; MAX_ORDER]; MAX_CONSTRAINTS],
            response: [ZERO; MAX_CONSTRAINTS],
        }
    }

    pub fn push(
        &mut self,
        steering: &[Complex<f32>],
        response: Complex<f32>,
    ) -> Result<(), BeamError> {
        if steering.len() != self.n {
            return Err(BeamError::Lanes(steering.len()));
        }
        if !(steering.iter().all(|value| value.is_finite()) && response.is_finite()) {
            return Err(LinalgError::NonFinite.into());
        }
        let limit = self.limit();
        if self.count >= limit {
            return Err(BeamError::TooManyConstraints(limit));
        }
        if self.count > 0 && overlap(self.vector(0), steering) > NULL_OVERLAP_LIMIT {
            return Err(BeamError::NullInMainLobe(self.count));
        }
        self.vectors[self.count][..self.n].copy_from_slice(steering);
        self.response[self.count] = response;
        self.count += 1;
        Ok(())
    }

    #[must_use]
    pub const fn count(&self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn order(&self) -> usize {
        self.n
    }

    #[must_use]
    pub const fn limit(&self) -> usize {
        let free = self.n.saturating_sub(1);
        if free < MAX_CONSTRAINTS {
            free
        } else {
            MAX_CONSTRAINTS
        }
    }

    #[must_use]
    pub fn vector(&self, index: usize) -> &[Complex<f32>] {
        &self.vectors[index][..self.n]
    }

    #[must_use]
    pub fn response(&self, index: usize) -> Complex<f32> {
        self.response[index]
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.vectors = [[ZERO; MAX_ORDER]; MAX_CONSTRAINTS];
        self.response = [ZERO; MAX_CONSTRAINTS];
    }
}

fn overlap(a: &[Complex<f32>], b: &[Complex<f32>]) -> f32 {
    let dot = inner(a, b);
    let norms = norm_sqr(a) * norm_sqr(b);
    if norms > 0.0 {
        dot.norm() / norms.sqrt()
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use num_complex::Complex;

    use super::*;
    use crate::array::beamform::testing::{kraken, steering};

    #[test]
    fn constraints_refuse_too_many_and_wrong_lengths() {
        let manifold = kraken();
        let mut constraints = Constraints::new(5);
        assert_eq!(constraints.limit(), 4);
        assert_eq!(
            constraints.push(&[Complex::new(1.0, 0.0); 3], Complex::new(1.0, 0.0)),
            Err(BeamError::Lanes(3))
        );
        for azimuth in [0.0, 90.0, 140.0, 200.0] {
            constraints
                .push(&steering(&manifold, azimuth), Complex::new(0.0, 0.0))
                .unwrap();
        }
        assert_eq!(
            constraints.push(&steering(&manifold, 45.0), Complex::new(0.0, 0.0)),
            Err(BeamError::TooManyConstraints(4))
        );
        constraints.clear();
        assert_eq!(constraints.count(), 0);
        assert_eq!(constraints, Constraints::new(5));
        assert_eq!(Constraints::new(2).limit(), 1);
    }
}
