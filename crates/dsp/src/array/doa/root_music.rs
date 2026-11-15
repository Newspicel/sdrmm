use num_complex::Complex;

use super::DoaError;
use crate::math::complex::widen;
use crate::math::linalg::{CMat, LinalgError, MAX_POLY_DEGREE, Roots};

type C64 = Complex<f64>;

/// Roots of the root-MUSIC polynomial of `projector`, one of each conjugate-reciprocal pair,
/// nearest the unit circle first.
pub fn root_music(
    projector: &CMat,
    roots: &mut Roots,
    coeffs: &mut [C64],
    found: &mut [C64],
    out: &mut [C64],
) -> Result<usize, DoaError> {
    let m = projector.order();
    if m < 2 {
        return Err(LinalgError::Order(m).into());
    }
    let degree = 2 * m - 2;
    if coeffs.len() <= degree || found.len() < degree || out.len() < m - 1 {
        return Err(LinalgError::Order(m).into());
    }
    let coeffs = &mut coeffs[..=degree];
    coeffs.fill(C64::new(0.0, 0.0));
    for p in 0..m {
        for q in 0..m {
            coeffs[q + m - 1 - p] += widen(projector.get(p, q));
        }
    }
    let count = roots.solve(coeffs, found)?;
    let found = &mut found[..count];
    for root in found.iter_mut() {
        let radius = root.norm();
        if radius > 1.0 {
            *root = (*root / radius) / radius;
        }
    }
    Ok(pick_twins(found, m - 1, out))
}

fn pick_twins(inside: &[C64], wanted: usize, out: &mut [C64]) -> usize {
    let mut used = [false; MAX_POLY_DEGREE];
    let mut picked = 0;
    while picked < wanted {
        let Some(best) = (0..inside.len())
            .filter(|&i| !used[i])
            .max_by(|&a, &b| inside[a].norm().total_cmp(&inside[b].norm()))
        else {
            break;
        };
        used[best] = true;
        out[picked] = inside[best];
        picked += 1;
        let twin = (0..inside.len()).filter(|&i| !used[i]).min_by(|&a, &b| {
            (inside[a] - inside[best])
                .norm()
                .total_cmp(&(inside[b] - inside[best]).norm())
        });
        if let Some(twin) = twin {
            used[twin] = true;
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::array::doa::testing::{closest_phase, decompose, line_covariance};
    use crate::math::linalg::MAX_ORDER;

    #[test]
    fn root_music_places_roots_on_the_source_phases() {
        let m = 6;
        let phases = [0.7, -1.9];
        let eigen = decompose(&line_covariance(m, &phases, 1e-3));
        let mut projector = CMat::zeros(m).unwrap();
        for k in 0..m - 2 {
            projector.add_outer(eigen.vector(k), 1.0);
        }
        let mut roots = Roots::new(MAX_POLY_DEGREE).unwrap();
        let mut coeffs = [C64::new(0.0, 0.0); MAX_POLY_DEGREE + 1];
        let mut found = [C64::new(0.0, 0.0); MAX_POLY_DEGREE];
        let mut out = [C64::new(0.0, 0.0); MAX_ORDER];
        let count = root_music(&projector, &mut roots, &mut coeffs, &mut found, &mut out).unwrap();
        assert_eq!(count, m - 1);
        for phase in phases {
            assert!(closest_phase(&out[..2], phase) < 0.05, "{:?}", &out[..2]);
        }
        assert!(out[..count].iter().all(|z| z.norm() <= 1.0 + 1e-6));
        let tiny = CMat::identity(1).unwrap();
        assert!(root_music(&tiny, &mut roots, &mut coeffs, &mut found, &mut out).is_err());
    }
}
