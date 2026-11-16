use std::f64::consts::TAU;

use num_complex::Complex;

/// Removes a carrier offset of `cycles_per_sample` from `source`, whose first sample sits at
/// `first_index` of the stream so the phase continues across calls.
pub(super) fn derotate(
    source: &[Complex<f32>],
    first_index: usize,
    cycles_per_sample: f64,
    out: &mut [Complex<f32>],
) {
    if cycles_per_sample == 0.0 {
        out[..source.len()].copy_from_slice(source);
        return;
    }
    for (index, (value, sample)) in out.iter_mut().zip(source).enumerate() {
        let turns = (cycles_per_sample * (first_index + index) as f64).fract();
        let (sin, cos) = (-TAU * turns).sin_cos();
        *value = sample * Complex::new(cos as f32, sin as f32);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derotation_removes_a_known_offset() {
        let source: Vec<Complex<f32>> = (0..1000)
            .map(|n| Complex::from_polar(1.0, (TAU * 0.01 * f64::from(n + 50)) as f32))
            .collect();
        let mut out = vec![Complex::default(); source.len()];
        derotate(&source, 50, 0.01, &mut out);
        assert!(
            out.iter()
                .all(|value| (value - Complex::new(1.0, 0.0)).norm() < 1e-4)
        );
    }
}
