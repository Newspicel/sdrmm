use num_complex::Complex;

/// Inner product `a^H b`.
#[must_use]
pub fn inner(a: &[Complex<f32>], b: &[Complex<f32>]) -> Complex<f32> {
    a.iter().zip(b).map(|(x, y)| x.conj() * y).sum()
}

/// Squared Euclidean norm `a^H a`.
#[must_use]
pub fn norm_sqr(a: &[Complex<f32>]) -> f32 {
    a.iter().map(Complex::norm_sqr).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inner_conjugates_the_left_side() {
        let a = [Complex::new(0.0f32, 1.0), Complex::new(2.0, 0.0)];
        let b = [Complex::new(0.0f32, 1.0), Complex::new(1.0, 1.0)];
        assert_eq!(inner(&a, &b), Complex::new(3.0, 2.0));
        assert_eq!(norm_sqr(&a), 5.0);
        assert_eq!(inner(&a, &a).re, norm_sqr(&a));
    }
}
