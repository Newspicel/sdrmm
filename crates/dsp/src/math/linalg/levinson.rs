use num_complex::Complex;

use super::LinalgError;

type C64 = Complex<f64>;

/// Smallest prediction-error ratio a step may leave before the matrix counts as not
/// positive definite.
const MIN_RATIO: f64 = 1e-14;

/// Levinson factor of a Hermitian Toeplitz matrix: the backward vector of every leading
/// block, kept so each solve costs O(n²) where a Cholesky factor costs O(n³) to build.
/// Buffers are sized once for the largest order, so factoring and solving never allocate.
pub struct Toeplitz {
    order: usize,
    row: Vec<C64>,
    diagonal: f64,
    forward: Vec<C64>,
    next: Vec<C64>,
    backward: Vec<C64>,
}

impl Toeplitz {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            order: 0,
            row: vec![C64::default(); capacity],
            diagonal: 0.0,
            forward: vec![C64::default(); capacity],
            next: vec![C64::default(); capacity],
            backward: vec![C64::default(); capacity * (capacity + 1) / 2],
        }
    }

    /// Factors the matrix whose first row is `row` (entry `(i, i + m)` is `row[m]`, the lower
    /// triangle its conjugate) with `loading` added to the real diagonal.
    pub fn factor(&mut self, row: &[C64], loading: f64) -> Result<(), LinalgError> {
        let n = row.len();
        if n == 0 || n > self.row.len() {
            return Err(LinalgError::Order(n));
        }
        self.order = 0;
        self.row[..n].copy_from_slice(row);
        self.diagonal = row[0].re + loading;
        if !self.diagonal.is_finite() {
            return Err(LinalgError::NonFinite);
        }
        if self.diagonal <= 0.0 {
            return Err(LinalgError::NotPositiveDefinite(0));
        }
        let first = C64::new(self.diagonal.recip(), 0.0);
        self.forward[0] = first;
        self.backward[0] = first;
        for k in 1..n {
            self.extend(k)?;
        }
        self.order = n;
        Ok(())
    }

    /// Grows the forward and backward vectors from block `k` to block `k + 1`.
    fn extend(&mut self, k: usize) -> Result<(), LinalgError> {
        let (row, forward) = (&self.row, &self.forward[..k]);
        let (done, rest) = self.backward.split_at_mut(packed(k));
        let backward = &done[packed(k - 1)..];
        let into_forward: C64 = (0..k).map(|i| row[k - i].conj() * forward[i]).sum();
        let into_backward: C64 = (0..k).map(|i| row[i + 1] * backward[i]).sum();
        let ratio = C64::new(1.0, 0.0) - into_forward * into_backward;
        if !(ratio.re.is_finite() && ratio.im.is_finite()) {
            return Err(LinalgError::NonFinite);
        }
        if ratio.re <= MIN_RATIO {
            return Err(LinalgError::NotPositiveDefinite(k));
        }
        let scale = ratio.inv();
        let grown = &mut rest[..=k];
        for i in 0..=k {
            let f = forward.get(i).copied().unwrap_or_default();
            let b = if i == 0 {
                C64::default()
            } else {
                backward[i - 1]
            };
            self.next[i] = (f - into_forward * b) * scale;
            grown[i] = (b - into_backward * f) * scale;
        }
        self.forward[..=k].copy_from_slice(&self.next[..=k]);
        Ok(())
    }

    /// Solves the factored system in place. Does nothing without a factor of `b`'s order.
    pub fn solve(&self, b: &mut [C64]) {
        let n = self.order;
        if n == 0 || b.len() != n {
            return;
        }
        for k in 0..n {
            let leak: C64 = (0..k).map(|i| self.row[k - i].conj() * b[i]).sum();
            let weight = b[k] - leak;
            b[k] = C64::default();
            let backward = &self.backward[packed(k)..=packed(k) + k];
            for (value, step) in b[..=k].iter_mut().zip(backward) {
                *value += weight * step;
            }
        }
    }
}

/// Where the backward vector of block `k + 1` starts in the packed store.
const fn packed(k: usize) -> usize {
    k * (k + 1) / 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::linalg::{cholesky_c64, cholesky_solve_c64};

    fn row(n: usize, seed: u64) -> Vec<C64> {
        let mut state = seed | 1;
        let mut next = move || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
        };
        (0..n)
            .map(|m| {
                let decay = 1.0 / (1.0 + m as f64);
                if m == 0 {
                    C64::new(n as f64, 0.0)
                } else {
                    C64::new(next(), next()) * decay
                }
            })
            .collect()
    }

    fn dense(row: &[C64]) -> Vec<C64> {
        let n = row.len();
        let mut matrix = vec![C64::default(); n * n];
        for i in 0..n {
            for j in 0..n {
                matrix[i * n + j] = if j >= i {
                    row[j - i]
                } else {
                    row[i - j].conj()
                };
            }
        }
        matrix
    }

    #[test]
    fn solves_match_cholesky() {
        for n in [1, 2, 3, 7, 32, 105] {
            let row = row(n, n as u64);
            let loading = 0.25;
            let mut toeplitz = Toeplitz::new(128);
            toeplitz.factor(&row, loading).unwrap();
            let mut factor = vec![C64::default(); n * n];
            cholesky_c64(&dense(&row), n, loading, &mut factor).unwrap();
            let rhs: Vec<C64> = (0..n).map(|i| C64::new(i as f64 - 3.0, 1.0)).collect();
            let (mut fast, mut slow) = (rhs.clone(), rhs);
            toeplitz.solve(&mut fast);
            cholesky_solve_c64(&factor, n, &mut slow);
            for (a, b) in fast.iter().zip(&slow) {
                assert!(
                    (a - b).norm() < 1e-9 * (1.0 + b.norm()),
                    "n {n}: {a} vs {b}"
                );
            }
        }
    }

    #[test]
    fn an_indefinite_matrix_is_refused_and_loading_rescues_it() {
        let row = [C64::new(1.0, 0.0), C64::new(1.0, 0.0)];
        let mut toeplitz = Toeplitz::new(4);
        assert!(matches!(
            toeplitz.factor(&row, 0.0),
            Err(LinalgError::NotPositiveDefinite(1))
        ));
        let mut b = [C64::new(1.0, 0.0); 2];
        toeplitz.solve(&mut b);
        assert_eq!(b, [C64::new(1.0, 0.0); 2]);
        toeplitz.factor(&row, 1.0).unwrap();
        toeplitz.solve(&mut b);
        assert!((b[0] - C64::new(1.0 / 3.0, 0.0)).norm() < 1e-12);
    }

    #[test]
    fn an_order_beyond_capacity_is_refused() {
        let mut toeplitz = Toeplitz::new(2);
        assert_eq!(
            toeplitz.factor(&[C64::new(1.0, 0.0); 3], 0.0),
            Err(LinalgError::Order(3))
        );
    }
}
