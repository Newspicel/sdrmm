use std::f64::consts::TAU;

use num_complex::Complex;

/// The symbol-rate line in the squared envelope, whose phase is the timing offset.
pub(super) fn square_law_line(filtered: &[Complex<f32>], sps: usize) -> Complex<f64> {
    let mut acc = Complex::new(0.0f64, 0.0);
    for (n, y) in filtered.iter().enumerate() {
        let theta = -TAU * (n % sps) as f64 / sps as f64;
        acc += Complex::new(theta.cos(), theta.sin()) * f64::from(y.norm_sqr());
    }
    acc
}

#[must_use]
pub fn square_law_offset(filtered: &[Complex<f32>], sps: usize) -> f64 {
    let usable = filtered.len() - filtered.len() % sps;
    let acc = square_law_line(&filtered[..usable], sps);
    if acc.norm() <= 0.0 {
        return 0.0;
    }
    let tau = -acc.arg() / TAU * sps as f64;
    tau.rem_euclid(sps as f64)
}
