use num_complex::Complex;

/// Orders past this skip the exact column solve and read the diagonal of the factor alone.
const MAX_EXACT_ORDER: usize = 64;

/// The `k`th diagonal entry of the inverse of the matrix whose Cholesky factor is `l`.
pub(super) fn inverse_diagonal(l: &[Complex<f64>], n: usize, k: usize) -> f64 {
    let mut total = 0.0;
    let mut column = [Complex::new(0.0f64, 0.0); MAX_EXACT_ORDER];
    if n > column.len() {
        return 1.0 / l[k * n + k].re.powi(2);
    }
    column[k] = Complex::new(1.0 / l[k * n + k].re, 0.0);
    total += column[k].norm_sqr();
    for i in k + 1..n {
        let mut v = Complex::new(0.0, 0.0);
        for j in k..i {
            v -= l[i * n + j] * column[j];
        }
        column[i] = v / l[i * n + i].re;
        total += column[i].norm_sqr();
    }
    total
}

#[cfg(test)]
mod tests {
    use sdrmm_dsp::math::linalg::{cholesky_c64, cholesky_solve_c64};

    use super::*;

    #[test]
    fn the_inverse_diagonal_matches_solving_for_each_unit_vector() {
        let n = 3;
        let a = [
            Complex::new(4.0, 0.0),
            Complex::new(1.0, 1.0),
            Complex::new(0.0, 0.5),
            Complex::new(1.0, -1.0),
            Complex::new(3.0, 0.0),
            Complex::new(0.2, 0.0),
            Complex::new(0.0, -0.5),
            Complex::new(0.2, 0.0),
            Complex::new(2.0, 0.0),
        ];
        let mut factor = [Complex::new(0.0, 0.0); 9];
        cholesky_c64(&a, n, 0.0, &mut factor).unwrap();
        for k in 0..n {
            let mut unit = [Complex::new(0.0, 0.0); 3];
            unit[k] = Complex::new(1.0, 0.0);
            cholesky_solve_c64(&factor, n, &mut unit);
            assert!((inverse_diagonal(&factor, n, k) - unit[k].re).abs() < 1e-12);
        }
    }
}
