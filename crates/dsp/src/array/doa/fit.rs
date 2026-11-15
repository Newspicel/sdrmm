use num_complex::Complex;

use crate::math::linalg::{CMat, Cholesky, LinalgError, MAX_ORDER, inner};

/// Diagonal loading of the steering Gram matrix, as a share of its mean power.
const GRAM_LOADING: f32 = 1e-6;

/// Share of the covariance trace that the waves along `steering` explain together.
pub fn joint_fit(
    steering: &[Complex<f32>],
    count: usize,
    elements: usize,
    r: &CMat,
    chol: &mut Cholesky,
    scratch: &mut [Complex<f32>],
) -> Result<f32, LinalgError> {
    let trace = r.trace_re();
    if count == 0 || trace.is_nan() || trace <= 0.0 {
        return Ok(0.0);
    }
    let size = count * elements;
    if count > MAX_ORDER || r.order() != elements || steering.len() < size || scratch.len() < size {
        return Err(LinalgError::Order(count));
    }
    let columns = &steering[..size];
    let products = &mut scratch[..size];
    for (a, out) in columns
        .chunks_exact(elements)
        .zip(products.chunks_exact_mut(elements))
    {
        multiply(r, a, out);
    }
    let mut gram = CMat::zeros(count)?;
    for (i, left) in columns.chunks_exact(elements).enumerate() {
        for (j, right) in columns.chunks_exact(elements).enumerate() {
            gram.set(i, j, inner(left, right));
        }
    }
    chol.factor_loaded(&gram, GRAM_LOADING)?;
    let mut explained = 0.0f32;
    let mut column = [Complex::new(0.0f32, 0.0); MAX_ORDER];
    for (b, product) in products.chunks_exact(elements).enumerate() {
        for (value, left) in column.iter_mut().zip(columns.chunks_exact(elements)) {
            *value = inner(left, product);
        }
        chol.solve(&mut column[..count])?;
        explained += column[b].re;
    }
    Ok((explained / trace).clamp(0.0, 1.0))
}

fn multiply(r: &CMat, a: &[Complex<f32>], out: &mut [Complex<f32>]) {
    let n = r.order();
    for (row, value) in out.iter_mut().enumerate().take(n) {
        *value = (0..n).map(|col| r.get(row, col) * a[col]).sum();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::doa::testing::{FREQ, steering};
    use crate::array::manifold::{Direction, steer, testing::half_wave_uca5};

    #[test]
    fn joint_fit_matches_the_single_wave_formula() {
        let geometry = half_wave_uca5();
        let a = steering(&geometry, 37.0);
        let snr = 10.0f32;
        let mut r = CMat::identity(5).unwrap();
        r.add_outer(&a, snr);
        let mut chol = Cholesky::new(16).unwrap();
        let mut scratch = [Complex::new(0.0f32, 0.0); 80];
        let fit = joint_fit(&a, 1, 5, &r, &mut chol, &mut scratch).unwrap();
        let want = (5.0 * snr + 1.0) / (5.0 * (snr + 1.0));
        assert!((fit - want).abs() < 1e-4, "{fit} {want}");
        let mut b = [Complex::new(0.0f32, 0.0); 10];
        b[..5].copy_from_slice(&a);
        steer(
            geometry.positions(),
            FREQ,
            Direction::horizon(200.0),
            &mut b[5..],
        );
        let two = joint_fit(&b, 2, 5, &r, &mut chol, &mut scratch).unwrap();
        assert!(two > fit && two < 1.0, "{two}");
        let noise = CMat::identity(5).unwrap();
        let alone = joint_fit(&a, 1, 5, &noise, &mut chol, &mut scratch).unwrap();
        assert!((alone - 0.2).abs() < 1e-5);
        assert_eq!(joint_fit(&a, 0, 5, &r, &mut chol, &mut scratch), Ok(0.0));
        assert_eq!(
            joint_fit(&a, 2, 5, &r, &mut chol, &mut scratch),
            Err(LinalgError::Order(2))
        );
    }
}
