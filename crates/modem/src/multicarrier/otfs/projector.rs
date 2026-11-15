/// Basis vectors shorter than this after orthogonalisation are dependent and dropped.
const DEPENDENT_NORM: f64 = 1e-9;

/// The projector onto polynomials of degree up to `order` over `symbols` samples, row-major,
/// and the dimension it keeps.
pub(super) fn polynomial_projector(symbols: usize, order: usize) -> (Vec<f64>, f64) {
    let columns = (order + 1).min(symbols).max(1);
    let mut q: Vec<Vec<f64>> = Vec::with_capacity(columns);
    for power in 0..columns {
        let mut v: Vec<f64> = (0..symbols)
            .map(|n| {
                let t = if symbols > 1 {
                    2.0 * n as f64 / (symbols - 1) as f64 - 1.0
                } else {
                    0.0
                };
                t.powi(power as i32)
            })
            .collect();
        for u in &q {
            let dot: f64 = v.iter().zip(u).map(|(a, b)| a * b).sum();
            for (a, b) in v.iter_mut().zip(u) {
                *a -= dot * b;
            }
        }
        let norm = v.iter().map(|a| a * a).sum::<f64>().sqrt();
        if norm > DEPENDENT_NORM {
            q.push(v.into_iter().map(|a| a / norm).collect());
        }
    }
    let mut p = vec![0.0; symbols * symbols];
    for u in &q {
        for n in 0..symbols {
            for m in 0..symbols {
                p[n * symbols + m] += u[n] * u[m];
            }
        }
    }
    (p, q.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_smoother_keeps_a_polynomial_and_rejects_a_fast_tone() {
        let (p, dof) = polynomial_projector(16, 3);
        assert!((dof - 4.0).abs() < 1e-12);
        let cubic: Vec<f64> = (0..16).map(|n| (n as f64 / 15.0).powi(3) - 0.2).collect();
        for n in 0..16 {
            let v: f64 = (0..16).map(|m| p[n * 16 + m] * cubic[m]).sum();
            assert!((v - cubic[n]).abs() < 1e-9);
        }
        let fast: Vec<f64> = (0..16)
            .map(|n| if n % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        let kept: f64 = (0..16)
            .map(|n| {
                (0..16)
                    .map(|m| p[n * 16 + m] * fast[m])
                    .sum::<f64>()
                    .powi(2)
            })
            .sum();
        assert!(kept < 1.0, "a Nyquist tone kept {kept} of 16");
    }
}
