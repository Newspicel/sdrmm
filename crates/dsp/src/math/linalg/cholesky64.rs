use num_complex::Complex;

use super::LinalgError;

type C64 = Complex<f64>;

/// Leading diagonal reciprocals kept on the stack; later columns divide, their dot products
/// dwarf it.
const RECIPROCALS: usize = 64;

#[inline]
fn dot_conj_c64(a: &[C64], b: &[C64]) -> C64 {
    #[cfg(target_arch = "aarch64")]
    {
        use std::arch::aarch64::{
            vaddq_f64, vaddvq_f64, vdupq_n_f64, vfmaq_f64, vfmsq_f64, vld2q_f64,
        };
        let (a4, a_rem) = a.as_chunks::<4>();
        let (b4, b_rem) = b.as_chunks::<4>();
        let (a2, a_tail) = a_rem.as_chunks::<2>();
        let (b2, b_tail) = b_rem.as_chunks::<2>();
        let (mut re, mut im) = unsafe {
            let (mut r0, mut i0) = (vdupq_n_f64(0.0), vdupq_n_f64(0.0));
            let (mut r1, mut i1) = (vdupq_n_f64(0.0), vdupq_n_f64(0.0));
            for (ac, bc) in a4.iter().zip(b4) {
                let ap = ac.as_ptr().cast::<f64>();
                let bp = bc.as_ptr().cast::<f64>();
                let va0 = vld2q_f64(ap);
                let vb0 = vld2q_f64(bp);
                let va1 = vld2q_f64(ap.add(4));
                let vb1 = vld2q_f64(bp.add(4));
                r0 = vfmaq_f64(vfmaq_f64(r0, va0.0, vb0.0), va0.1, vb0.1);
                i0 = vfmsq_f64(vfmaq_f64(i0, va0.1, vb0.0), va0.0, vb0.1);
                r1 = vfmaq_f64(vfmaq_f64(r1, va1.0, vb1.0), va1.1, vb1.1);
                i1 = vfmsq_f64(vfmaq_f64(i1, va1.1, vb1.0), va1.0, vb1.1);
            }
            for (ac, bc) in a2.iter().zip(b2) {
                let va = vld2q_f64(ac.as_ptr().cast());
                let vb = vld2q_f64(bc.as_ptr().cast());
                r0 = vfmaq_f64(vfmaq_f64(r0, va.0, vb.0), va.1, vb.1);
                i0 = vfmsq_f64(vfmaq_f64(i0, va.1, vb.0), va.0, vb.1);
            }
            (vaddvq_f64(vaddq_f64(r0, r1)), vaddvq_f64(vaddq_f64(i0, i1)))
        };
        for (&x, &y) in a_tail.iter().zip(b_tail) {
            let p = x * y.conj();
            re += p.re;
            im += p.im;
        }
        C64::new(re, im)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        let mut sum = C64::default();
        for (&x, &y) in a.iter().zip(b) {
            sum += x * y.conj();
        }
        sum
    }
}

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
    let mut reciprocal = [0.0f64; RECIPROCALS];
    for i in 0..n {
        let row_i = i * n;
        for j in 0..i {
            let row_j = j * n;
            let dot = dot_conj_c64(&factor[row_i..row_i + j], &factor[row_j..row_j + j]);
            let sum = matrix[row_i + j] - dot;
            let scale = reciprocal
                .get(j)
                .copied()
                .unwrap_or_else(|| factor[row_j + j].re.recip());
            factor[row_i + j] = sum * scale;
        }
        let dot = dot_conj_c64(&factor[row_i..row_i + i], &factor[row_i..row_i + i]);
        let diag = matrix[row_i + i].re + loading - dot.re;
        if !diag.is_finite() {
            return Err(LinalgError::NonFinite);
        }
        if diag <= 0.0 {
            return Err(LinalgError::NotPositiveDefinite(i));
        }
        let root = diag.sqrt();
        factor[row_i + i] = C64::new(root, 0.0);
        if let Some(slot) = reciprocal.get_mut(i) {
            *slot = root.recip();
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
