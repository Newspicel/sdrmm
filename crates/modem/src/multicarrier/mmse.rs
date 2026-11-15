use num_complex::Complex;

/// The MMSE weight for a bin of gain `h` under `noise_var`, and the bias it leaves on the bin.
pub(super) fn mmse(h: Complex<f32>, noise_var: f32) -> (Complex<f32>, f32) {
    let gain = h.norm_sqr();
    (h.conj() / (gain + noise_var), gain / (gain + noise_var))
}

/// Divides out the mean MMSE bias, `bias_sum` over every value, so decisions see unit gain.
pub(super) fn unbias(values: &mut [Complex<f32>], bias_sum: f64) {
    let mean = (bias_sum / values.len() as f64).max(f64::MIN_POSITIVE) as f32;
    for v in values {
        *v /= mean;
    }
}
