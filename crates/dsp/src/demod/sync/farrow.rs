use num_complex::Complex;

const CURVATURE: f32 = 0.5;

/// Cubic Farrow interpolation between `w[1]` and `w[2]` at fraction `mu`, from four samples.
#[must_use]
pub fn farrow(w: &[Complex<f32>], mu: f32) -> Complex<f32> {
    let curvature = w[3] - w[2] - w[1] + w[0];
    w[1] + (w[2] - w[1]) * mu + curvature * (CURVATURE * mu * (mu - 1.0))
}
