use num_complex::Complex;

use crate::math::linalg::CMat;

/// Adds `fraction` of the mean element power to the diagonal and returns what was added.
pub fn load_diagonal(r: &mut CMat, fraction: f32) -> f32 {
    let n = r.order();
    let delta = fraction * r.trace_re() / n as f32;
    for i in 0..n {
        r.add(i, i, Complex::new(delta, 0.0));
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loading_adds_a_fraction_of_the_mean_power() {
        let mut r = CMat::identity(4).unwrap();
        r.set(0, 0, Complex::new(5.0, 0.0));
        let delta = load_diagonal(&mut r, 0.1);
        assert!((delta - 0.2).abs() < 1e-6);
        assert!((r.get(0, 0).re - 5.2).abs() < 1e-6);
        assert!((r.get(3, 3).re - 1.2).abs() < 1e-6);
    }
}
