use num_complex::Complex;

use super::DoaError;
use crate::math::complex::widen;
use crate::math::linalg::{CMat, Eigen, GeneralEigen, HermitianEigen, LinalgError, MAX_ORDER, Qr};

pub const MAX_ESPRIT_SOURCES: usize = MAX_ORDER / 2;

type C64 = Complex<f64>;

pub struct EspritSolvers {
    eigen: HermitianEigen,
    qr: Qr,
    general: GeneralEigen,
}

impl EspritSolvers {
    pub fn new() -> Result<Self, LinalgError> {
        Ok(Self {
            eigen: HermitianEigen::new(2)?,
            qr: Qr::new(1, 1)?,
            general: GeneralEigen::new(MAX_ESPRIT_SOURCES)?,
        })
    }
}

pub fn esprit(
    signal: &[Complex<f32>],
    rows: usize,
    sources: usize,
    row_weights: &[f32],
    solvers: &mut EspritSolvers,
    out: &mut [C64],
) -> Result<usize, DoaError> {
    let d = sources;
    let weighted = !row_weights.is_empty();
    if d == 0
        || d > MAX_ESPRIT_SOURCES
        || rows <= d
        || signal.len() < rows * d
        || out.len() < d
        || (weighted && row_weights.len() < rows - 1)
    {
        return Err(LinalgError::Order(d).into());
    }
    let stacked = shifted_gram(signal, rows, d, row_weights)?;
    let EspritSolvers { eigen, qr, general } = solvers;
    if eigen.order() != 2 * d {
        *eigen = HermitianEigen::new(2 * d)?;
    }
    let mut decomposition = Eigen::new();
    eigen.solve(&stacked, &mut decomposition)?;
    let mut lower = [Complex::new(0.0f32, 0.0); MAX_ESPRIT_SOURCES * MAX_ESPRIT_SOURCES];
    for c in 0..d {
        let vector = decomposition.vector(c);
        for r in 0..d {
            lower[r * d + c] = vector[d + r];
        }
    }
    *qr = Qr::new(d, d)?;
    qr.factor(&lower[..d * d])?;
    let mut psi = [C64::new(0.0, 0.0); MAX_ESPRIT_SOURCES * MAX_ESPRIT_SOURCES];
    let mut rhs = [Complex::new(0.0f32, 0.0); MAX_ESPRIT_SOURCES];
    for c in 0..d {
        let upper = &decomposition.vector(c)[..d];
        for (i, value) in rhs.iter_mut().enumerate().take(d) {
            *value = (0..d).map(|r| qr.q().get(r, i).conj() * upper[r]).sum();
        }
        qr.solve_upper(&mut rhs[..d])?;
        for (i, value) in rhs.iter().enumerate().take(d) {
            psi[i * d + c] = -widen(*value);
        }
    }
    general.eigenvalues(&psi[..d * d], &mut out[..d])?;
    Ok(d)
}

fn shifted_gram(
    signal: &[Complex<f32>],
    rows: usize,
    d: usize,
    row_weights: &[f32],
) -> Result<CMat, LinalgError> {
    let width = 2 * d;
    let entry = |row: usize, column: usize| {
        if column < d {
            signal[column * rows + row]
        } else {
            signal[(column - d) * rows + row + 1]
        }
    };
    let weight = |row: usize| row_weights.get(row).map_or(1.0, |w| w * w);
    let mut gram = CMat::zeros(width)?;
    for a in 0..width {
        for b in a..width {
            let value: Complex<f32> = (0..rows - 1)
                .map(|row| entry(row, a).conj() * entry(row, b) * weight(row))
                .sum();
            gram.set(a, b, value);
            gram.set(b, a, value.conj());
        }
    }
    Ok(gram)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::doa::testing::{closest_phase, decompose, line_covariance};

    #[test]
    fn esprit_rotates_the_signal_subspace_onto_the_phases() {
        let m = 8;
        let phases = [1.1, -0.4];
        let eigen = decompose(&line_covariance(m, &phases, 1e-3));
        let mut signal = vec![Complex::new(0.0f32, 0.0); 2 * m];
        for (slot, k) in [m - 1, m - 2].into_iter().enumerate() {
            signal[slot * m..(slot + 1) * m].copy_from_slice(eigen.vector(k));
        }
        let mut solvers = EspritSolvers::new().unwrap();
        let mut out = [C64::new(0.0, 0.0); MAX_ORDER];
        let count = esprit(&signal, m, 2, &[], &mut solvers, &mut out).unwrap();
        assert_eq!(count, 2);
        for phase in phases {
            assert!(closest_phase(&out[..2], phase) < 0.05, "{:?}", &out[..2]);
        }
        assert!(out[..2].iter().all(|z| (z.norm() - 1.0).abs() < 1e-2));
        let weights = [0.5, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5];
        esprit(&signal, m, 2, &weights, &mut solvers, &mut out).unwrap();
        for phase in phases {
            assert!(closest_phase(&out[..2], phase) < 0.05, "{:?}", &out[..2]);
        }
        assert!(esprit(&signal, m, 2, &weights[..3], &mut solvers, &mut out).is_err());
        assert!(esprit(&signal, 2, 2, &[], &mut solvers, &mut out).is_err());
        assert!(esprit(&signal, m, 0, &[], &mut solvers, &mut out).is_err());
    }
}
