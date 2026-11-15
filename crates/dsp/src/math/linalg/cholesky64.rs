use num_complex::Complex;

use super::LinalgError;

type C64 = Complex<f64>;

/// Factors the row-major Hermitian `matrix` of order `n`, plus `loading` on its diagonal,
/// into the lower triangular `factor`. Allocation free, for f64 normal equations.
pub fn cholesky_c64(
    matrix: &[C64],
    n: usize,
    loading: f64,
    factor: &mut [C64],
) -> Result<(), LinalgError> {
    if !loading.is_finite() {
        return Err(LinalgError::NonFinite);
    }
    if loading < 0.0 {
        return Err(LinalgError::NotPositiveDefinite(0));
    }
    factor[..n * n].fill(C64::default());
    for i in 0..n {
        for j in 0..=i {
            let mut sum = matrix[i * n + j];
            if i == j {
                sum += loading;
            }
            for k in 0..j {
                sum -= factor[i * n + k] * factor[j * n + k].conj();
            }
            if i == j {
                if !sum.re.is_finite() {
                    return Err(LinalgError::NonFinite);
                }
                if sum.re <= 0.0 {
                    return Err(LinalgError::NotPositiveDefinite(i));
                }
                factor[i * n + i] = C64::new(sum.re.sqrt(), 0.0);
            } else {
                factor[i * n + j] = sum / factor[j * n + j].re;
            }
        }
    }
    Ok(())
}

/// Solves `L L^H x = b` in place with a `factor` from [`cholesky_c64`].
pub fn cholesky_solve_c64(factor: &[C64], n: usize, b: &mut [C64]) {
    for i in 0..n {
        let mut sum = b[i];
        for k in 0..i {
            sum -= factor[i * n + k] * b[k];
        }
        b[i] = sum / factor[i * n + i].re;
    }
    for i in (0..n).rev() {
        let mut sum = b[i];
        for k in i + 1..n {
            sum -= factor[k * n + i].conj() * b[k];
        }
        b[i] = sum / factor[i * n + i].re;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hermitian() -> [C64; 9] {
        [
            C64::new(4.0, 0.0),
            C64::new(1.0, 2.0),
            C64::new(0.5, -1.0),
            C64::new(1.0, -2.0),
            C64::new(6.0, 0.0),
            C64::new(-1.0, 0.5),
            C64::new(0.5, 1.0),
            C64::new(-1.0, -0.5),
            C64::new(5.0, 0.0),
        ]
    }

    fn times(matrix: &[C64], n: usize, x: &[C64]) -> Vec<C64> {
        (0..n)
            .map(|i| (0..n).map(|k| matrix[i * n + k] * x[k]).sum())
            .collect()
    }

    #[test]
    fn a_hermitian_system_solves_back() {
        let matrix = hermitian();
        let mut factor = [C64::default(); 9];
        cholesky_c64(&matrix, 3, 0.0, &mut factor).unwrap();
        let truth = [
            C64::new(1.0, -1.0),
            C64::new(0.5, 2.0),
            C64::new(-3.0, 0.25),
        ];
        let mut x = times(&matrix, 3, &truth);
        cholesky_solve_c64(&factor, 3, &mut x);
        for (got, want) in x.iter().zip(truth) {
            assert!((got - want).norm() < 1e-12, "{got} {want}");
        }
    }

    #[test]
    fn loading_rescues_a_singular_matrix() {
        let ones = [C64::new(1.0, 0.0); 4];
        let mut factor = [C64::default(); 4];
        assert_eq!(
            cholesky_c64(&ones, 2, 0.0, &mut factor),
            Err(LinalgError::NotPositiveDefinite(1))
        );
        assert!(cholesky_c64(&ones, 2, 0.1, &mut factor).is_ok());
    }

    #[test]
    fn bad_loading_and_values_are_refused() {
        let matrix = hermitian();
        let mut factor = [C64::default(); 9];
        assert!(cholesky_c64(&matrix, 3, f64::NAN, &mut factor).is_err());
        assert!(cholesky_c64(&matrix, 3, -1.0, &mut factor).is_err());
        let mut broken = matrix;
        broken[4] = C64::new(f64::INFINITY, 0.0);
        assert_eq!(
            cholesky_c64(&broken, 3, 0.0, &mut factor),
            Err(LinalgError::NonFinite)
        );
    }
}
