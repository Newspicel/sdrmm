use num_complex::Complex;

/// Magnitude of the normalised inner product: 1 when `a` and `b` are parallel.
pub(crate) fn inner(a: &[Complex<f32>], b: &[Complex<f32>]) -> f32 {
    let dot: Complex<f32> = a.iter().zip(b).map(|(x, y)| x.conj() * y).sum();
    let norms =
        a.iter().map(Complex::norm_sqr).sum::<f32>() * b.iter().map(Complex::norm_sqr).sum::<f32>();
    dot.norm() / norms.sqrt()
}
