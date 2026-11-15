use num_complex::Complex;

use crate::{math::complex::narrow, math::linalg::CMat};

/// Puts ones on the diagonal of `out`, which must be zeroed.
pub(super) fn write_identity(out: &mut CMat) {
    for i in 0..out.order() {
        out.set(i, i, Complex::new(1.0, 0.0));
    }
}

/// Writes `value` at `(row, col)` and its conjugate at `(col, row)`.
pub(super) fn set_mirrored(out: &mut CMat, row: usize, col: usize, value: Complex<f64>) {
    let value = narrow(value);
    out.set(row, col, value);
    out.set(col, row, value.conj());
}
