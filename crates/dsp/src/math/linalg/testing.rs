use num_complex::Complex;

use super::CMat;
use crate::testutil::XorShift32;

pub(crate) fn hermitian(order: usize, entries: &[(usize, usize, f32, f32)]) -> CMat {
    let mut matrix = CMat::zeros(order).unwrap();
    for &(row, col, re, im) in entries {
        matrix.set(row, col, Complex::new(re, im));
        matrix.set(col, row, Complex::new(re, -im));
    }
    matrix
}

pub(crate) fn random_vector(rng: &mut XorShift32, len: usize) -> Vec<Complex<f32>> {
    (0..len)
        .map(|_| Complex::new(rng.next_f32(), rng.next_f32()))
        .collect()
}

pub(crate) fn outer_sum(order: usize, vectors: &[Vec<Complex<f32>>]) -> CMat {
    let mut matrix = CMat::zeros(order).unwrap();
    for x in vectors {
        matrix.add_outer(x, 1.0);
    }
    matrix
}

pub(crate) fn random_hpd(order: usize, seed: u32) -> CMat {
    let mut rng = XorShift32(seed);
    let vectors: Vec<_> = (0..order + 3)
        .map(|_| random_vector(&mut rng, order))
        .collect();
    outer_sum(order, &vectors)
}

pub(crate) fn mat_vec(matrix: &CMat, x: &[Complex<f32>]) -> Vec<Complex<f32>> {
    let n = matrix.order();
    (0..n)
        .map(|row| (0..n).map(|col| matrix.get(row, col) * x[col]).sum())
        .collect()
}
