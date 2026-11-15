use super::setup::usable;
use super::space::Space;
use super::{
    C32, C64, Doa, DoaError, Estimator, Found, MAX_ESPRIT_SOURCES, MAX_PEAKS, ROOT_RADIUS_SLACK,
    Stats, esprit, line_azimuths, root_music,
};
use crate::array::manifold::{Direction, Manifold, wavenumber};
use crate::math::linalg::MAX_ORDER;
use crate::math::special::norm_deg;

impl Doa {
    pub(super) fn subspace_peaks(
        &mut self,
        manifold: &Manifold,
        space: Space,
        stats: Stats,
        out: &mut [Found; MAX_PEAKS],
    ) -> Result<usize, DoaError> {
        let mut values = [C64::new(0.0, 0.0); MAX_ORDER];
        let (count, radial) = match self.config.estimator {
            Estimator::Esprit => (self.esprit_values(space, stats, &mut values)?, false),
            _ => (self.root_values(space, stats, &mut values)?, true),
        };
        let wanted = self.wanted(stats);
        let limit = stats.sources.max(1);
        let mut chosen = [Found::default(); MAX_ORDER];
        let mut kept = 0;
        for value in &values[..count] {
            if kept == limit {
                break;
            }
            let Some(azimuth_deg) = self.value_azimuth(space, *value) else {
                continue;
            };
            let radius = value.norm();
            let fit_scale = if radial && 1.0 - radius > ROOT_RADIUS_SLACK {
                radius.clamp(0.0, 1.0) as f32
            } else {
                1.0
            };
            chosen[kept] = Found {
                azimuth_deg,
                elevation_deg: 0.0,
                fit_scale,
                power: self.capon_at(manifold, Direction::horizon(azimuth_deg)),
            };
            kept += 1;
        }
        let chosen = &mut chosen[..kept];
        chosen.sort_unstable_by(|a, b| b.power.total_cmp(&a.power));
        let taken = kept.min(wanted);
        out[..taken].copy_from_slice(&chosen[..taken]);
        Ok(taken)
    }

    pub(super) fn root_values(
        &mut self,
        space: Space,
        stats: Stats,
        out: &mut [C64; MAX_ORDER],
    ) -> Result<usize, DoaError> {
        let m = stats.order;
        let signals = stats.sources.max(1).min(m - 1);
        let solvers = &mut self.solvers;
        solvers.projector.resize(m)?;
        for k in 0..m - signals {
            solvers
                .projector
                .add_outer(solvers.decomposition.vector(k), 1.0);
        }
        if let Some(weights) = self
            .workspace
            .weights(space, usable(self.phase.as_ref(), self.phase_fault))
        {
            for i in 0..m {
                for j in 0..m {
                    let value = solvers.projector.get(i, j) * (weights[i] * weights[j]);
                    solvers.projector.set(i, j, value);
                }
            }
        }
        root_music(
            &solvers.projector,
            &mut solvers.roots,
            &mut solvers.coeffs,
            &mut solvers.found,
            out,
        )
    }

    pub(super) fn esprit_values(
        &mut self,
        space: Space,
        stats: Stats,
        out: &mut [C64; MAX_ORDER],
    ) -> Result<usize, DoaError> {
        let m = stats.order;
        let d = stats.sources.max(1).min(m - 1).min(MAX_ESPRIT_SOURCES);
        let mut signal = [C32::new(0.0, 0.0); MAX_ORDER * MAX_ESPRIT_SOURCES];
        let mut rows = [0.0f32; MAX_ORDER];
        let whitening = self.workspace.whitening();
        let weighted = matches!(space, Space::Vandermonde { .. });
        for slot in 0..d {
            let vector = self.solvers.decomposition.vector(m - 1 - slot);
            let column = &mut signal[slot * m..(slot + 1) * m];
            for ((value, &e), &weight) in column.iter_mut().zip(vector).zip(whitening) {
                *value = if weighted && weight > 0.0 {
                    e / weight
                } else {
                    e
                };
            }
        }
        if weighted {
            for (row, value) in rows.iter_mut().enumerate().take(m - 1) {
                *value = pair_weight(whitening[row], whitening[row + 1]);
            }
        }
        let row_weights = if weighted { &rows[..m - 1] } else { &[][..] };
        esprit(
            &signal[..m * d],
            m,
            d,
            row_weights,
            &mut self.solvers.esprit,
            out,
        )
    }

    pub(super) fn value_azimuth(&self, space: Space, value: C64) -> Option<f64> {
        match (space, self.layout.line) {
            (Space::Line { .. }, Some(line)) => {
                let kd = wavenumber(self.freq_hz) * line.spacing_m;
                let (first, _) = line_azimuths(value.arg(), kd, line.sort_axis_deg)?;
                Some(self.to_side(first))
            }
            (Space::Line { .. }, None) => None,
            _ => Some(norm_deg(value.arg().to_degrees())),
        }
    }
}

fn pair_weight(first: f32, second: f32) -> f32 {
    let spread = first * first + second * second;
    if spread > 0.0 {
        first * second / spread.sqrt()
    } else {
        0.0
    }
}
