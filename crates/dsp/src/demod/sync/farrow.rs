use num_complex::Complex;

/// Cubic Lagrange interpolation between `w[1]` and `w[2]` at fraction `mu`, from four samples,
/// in Farrow form. Its error falls with the fourth power of the signal bandwidth, so a signal
/// oversampled 16 times comes back about 70 dB clean, which dense QAM needs.
#[must_use]
#[inline(always)]
pub fn farrow(w: &[Complex<f32>], mu: f32) -> Complex<f32> {
    let [c1, c2, c3] = coefficients(w[0], w[1], w[2], w[3]);
    ((c3 * mu + c2) * mu + c1) * mu + w[1]
}

#[must_use]
#[inline(always)]
pub fn farrow_real(w: &[f32], mu: f32) -> f32 {
    let [c1, c2, c3] = coefficients(w[0], w[1], w[2], w[3]);
    ((c3 * mu + c2) * mu + c1) * mu + w[1]
}

#[inline(always)]
fn coefficients<T>(w0: T, w1: T, w2: T, w3: T) -> [T; 3]
where
    T: Copy
        + std::ops::Add<Output = T>
        + std::ops::Sub<Output = T>
        + std::ops::Mul<f32, Output = T>,
{
    let c1 = w2 - w0 * (1.0 / 3.0) - w1 * 0.5 - w3 * (1.0 / 6.0);
    let c2 = (w0 + w2) * 0.5 - w1;
    let c3 = (w3 - w0) * (1.0 / 6.0) + (w1 - w2) * 0.5;
    [c1, c2, c3]
}
