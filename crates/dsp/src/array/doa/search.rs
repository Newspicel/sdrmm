use super::peaks::{REFINE_ITERATIONS, REFINE_TOL_DEG};
use super::probe::Probe;
use super::space::Space;
use super::{
    Candidate, Doa, Found, GridShape, MAX_CANDIDATES, MAX_PEAKS, MIRROR_APART_DEG, Stats, UlaSide,
    apart_deg, local_maxima, mirror_deg, on_side,
};
use crate::array::manifold::{Direction, Manifold};
use crate::math::special::{brent_max, norm_deg};

/// Grid peaks closer than this many grid steps are one source.
const SAME_PEAK_STEPS: f64 = 1.5;

impl Doa {
    pub(super) fn grid_peaks(
        &self,
        manifold: &Manifold,
        space: Space,
        stats: Stats,
        out: &mut [Found; MAX_PEAKS],
    ) -> usize {
        let shape = GridShape {
            azimuths: self.grid.azimuths(),
            elevations: self.grid.elevations(),
            wraps: self.grid.wraps(),
        };
        let mut candidates = [Candidate::default(); MAX_CANDIDATES];
        let count = local_maxima(
            &self.spectrum,
            shape,
            self.config.peak_range_db,
            &mut candidates,
        );
        let wanted = self.wanted(stats);
        let apart = SAME_PEAK_STEPS * self.grid.azimuth_step_deg();
        let mut kept = 0;
        for candidate in &candidates[..count] {
            if kept == wanted {
                break;
            }
            let direction = self.grid.direction(candidate.point);
            let azimuth_deg = self.to_side(direction.azimuth_deg);
            let taken = out[..kept].iter().any(|found| {
                found.elevation_deg == direction.elevation_deg
                    && apart_deg(found.azimuth_deg, azimuth_deg) < apart
            });
            if !taken {
                out[kept] = Found {
                    azimuth_deg,
                    elevation_deg: direction.elevation_deg,
                    fit_scale: 1.0,
                    power: candidate.power,
                };
                kept += 1;
            }
        }
        let probe = self.probe(manifold, space, stats);
        for found in &mut out[..kept] {
            self.refine_found(&probe, found);
            found.azimuth_deg = self.to_side(found.azimuth_deg);
        }
        kept
    }

    pub(super) fn refine_found(&self, probe: &Probe<'_>, found: &mut Found) {
        let step = self.grid.azimuth_step_deg();
        let along_azimuth = |found: &mut Found| {
            let elevation = found.elevation_deg;
            let (azimuth, power) = best_of(
                |azimuth| probe.power(Direction::new(azimuth, elevation)),
                (found.azimuth_deg, found.power),
                found.azimuth_deg - step,
                found.azimuth_deg + step,
            );
            found.azimuth_deg = norm_deg(azimuth);
            found.power = power;
        };
        along_azimuth(found);
        let Some(span) = self.grid.spec().elevation else {
            return;
        };
        let rows = self.grid.elevations().saturating_sub(1).max(1) as f64;
        let elevation_step = (span.max_deg - span.min_deg) / rows;
        let azimuth = found.azimuth_deg;
        let (elevation, power) = best_of(
            |elevation| probe.power(Direction::new(azimuth, elevation)),
            (found.elevation_deg, found.power),
            (found.elevation_deg - elevation_step).max(span.min_deg),
            (found.elevation_deg + elevation_step).min(span.max_deg),
        );
        found.elevation_deg = elevation;
        found.power = power;
        along_azimuth(found);
    }

    pub(super) fn to_side(&self, azimuth_deg: f64) -> f64 {
        match (
            self.layout.axis_deg,
            self.layout.side_centre_deg(self.config.ula_side),
        ) {
            (Some(axis), Some(centre)) if !on_side(azimuth_deg, centre) => {
                mirror_deg(azimuth_deg, axis)
            }
            _ => norm_deg(azimuth_deg),
        }
    }

    pub(super) fn mirror_of(&self, azimuth_deg: f64) -> Option<f64> {
        let axis = self.layout.axis_deg?;
        if self.config.ula_side != UlaSide::Both {
            return None;
        }
        let mirror = mirror_deg(azimuth_deg, axis);
        (apart_deg(mirror, azimuth_deg) > MIRROR_APART_DEG).then_some(mirror)
    }
}

fn best_of(mut f: impl FnMut(f64) -> f32, current: (f64, f32), lo: f64, hi: f64) -> (f64, f32) {
    let (x, fx) = brent_max(
        |x| f64::from(f(x)),
        lo,
        hi,
        REFINE_TOL_DEG,
        REFINE_ITERATIONS,
    );
    let power = fx as f32;
    if power >= current.1 {
        (x, power)
    } else {
        current
    }
}
