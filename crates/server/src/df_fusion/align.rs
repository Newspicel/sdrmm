use std::collections::BTreeMap;

use sdrmm_wire::{DfEstimate, geo::LatLon};

use super::{
    NodeFusion,
    estimate::window_cells,
    grid::{LogGrid, look},
    nav::{CONCENTRATED_MAJOR_M, CONCENTRATED_MASS},
    observation::{Observation, rotate},
    votes::VoteKey,
};

pub(crate) const MAX_ALIGN_DEG: f32 = 15.0;
pub(crate) const ALIGN_EVERY_S: f64 = 10.0;
pub(crate) const MIN_ALIGN_KEYS: usize = 8;
pub(crate) const MIN_ALIGN_SAMPLES: u32 = 40;
pub(crate) const MAX_ALIGN_SIGMA_DEG: f32 = 0.5;
pub(crate) const ALIGN_GAIN: f32 = 0.5;
pub(crate) const MIN_ALIGN_STEP_DEG: f32 = 0.5;

const SEARCH_DEG: i32 = 10;
const MIN_REACH_M: f64 = 300.0;
const MAX_REACH_M: f64 = 1_500.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Offset {
    pub(crate) deg: f32,
    pub(crate) sigma_deg: f32,
}

pub(crate) type Aligns = BTreeMap<String, f32>;

pub(crate) fn apply(aligns: &Aligns, observation: &mut Observation) {
    let Some(&align) = aligns.get(&observation.station) else {
        return;
    };
    observation.bearing_deg = (observation.bearing_deg - align).rem_euclid(360.0);
    rotate(&mut observation.bank, -align);
}

pub(crate) fn offset(
    grid: &LogGrid,
    keys: &[VoteKey],
    station: &str,
    around: (f64, f64),
    reach_m: f64,
) -> Option<Offset> {
    let own: Vec<_> = keys
        .iter()
        .filter(|key| key.station == station)
        .filter_map(|key| key.painted.as_ref())
        .collect();
    if own.is_empty() {
        return None;
    }
    let cell_m = grid.cell_m();
    let reach = (reach_m.clamp(MIN_REACH_M, MAX_REACH_M) / cell_m).ceil() as usize;
    let values = grid.values();
    let mut best = [f32::NEG_INFINITY; (2 * SEARCH_DEG + 1) as usize];
    for index in window_cells(grid, around, reach) {
        let (east, north) = grid.index_centre(index);
        let looks: Vec<_> = own
            .iter()
            .map(|painted| {
                let wedge = &painted.wedge;
                let at = look(
                    east - wedge.east_m,
                    north - wedge.north_m,
                    wedge.accuracy_m,
                    cell_m,
                );
                (at, wedge)
            })
            .collect();
        let painted: f32 = looks
            .iter()
            .map(|(at, wedge)| at.sample(&wedge.ring, wedge.centre_value))
            .sum();
        let base = values[index] - painted;
        for (slot, turn) in best.iter_mut().zip(-SEARCH_DEG..=SEARCH_DEG) {
            let turned: f32 = looks
                .iter()
                .map(|(at, wedge)| at.turned(turn).sample(&wedge.ring, wedge.centre_value))
                .sum();
            *slot = slot.max(base + turned);
        }
    }
    vertex(&best)
}

fn vertex(profile: &[f32]) -> Option<Offset> {
    let (top, _) = profile
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))?;
    if top == 0 || top + 1 == profile.len() {
        return None;
    }
    let (left, middle, right) = (profile[top - 1], profile[top], profile[top + 1]);
    let bend = left - 2.0 * middle + right;
    if bend.is_nan() || bend >= 0.0 {
        return None;
    }
    let shift = 0.5 * (left - right) / bend;
    Some(Offset {
        deg: top as f32 - SEARCH_DEG as f32 + shift,
        sigma_deg: (-1.0 / bend).sqrt(),
    })
}

fn concentrated(estimate: &DfEstimate) -> bool {
    estimate.mass >= CONCENTRATED_MASS
        && estimate.ellipse_major_m <= CONCENTRATED_MAJOR_M
        && estimate.samples >= MIN_ALIGN_SAMPLES
}

impl NodeFusion {
    pub(super) fn align(&mut self, now_s: f64) -> bool {
        if !self.params.align
            || self
                .aligned_s
                .is_some_and(|aligned| now_s - aligned < ALIGN_EVERY_S)
        {
            return false;
        }
        let Some(estimate) = self.estimate.filter(concentrated) else {
            return false;
        };
        self.aligned_s = Some(now_s);
        let Some(station) = self.next_station() else {
            return false;
        };
        let Some(grid) = &self.grid else {
            return false;
        };
        let around = grid.enu().to_enu(LatLon {
            lat: estimate.lat,
            lon: estimate.lon,
        });
        let Some(found) = offset(
            grid,
            &self.votes.keys,
            &station,
            around,
            estimate.ellipse_major_m,
        ) else {
            return false;
        };
        if found.sigma_deg > MAX_ALIGN_SIGMA_DEG || found.deg.abs() < MIN_ALIGN_STEP_DEG {
            return false;
        }
        let was = self.aligns.get(&station).copied().unwrap_or(0.0);
        let now = ALIGN_GAIN
            .mul_add(found.deg, was)
            .clamp(-MAX_ALIGN_DEG, MAX_ALIGN_DEG);
        self.turn_station(&station, now - was);
        self.aligns.insert(station, now);
        true
    }

    pub(super) fn unalign(&mut self) {
        let aligns = std::mem::take(&mut self.aligns);
        for (station, align) in aligns {
            self.turn_station(&station, -align);
        }
    }

    fn next_station(&mut self) -> Option<String> {
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for key in &self.votes.keys {
            *counts.entry(key.station.as_str()).or_default() += 1;
        }
        let ready: Vec<&str> = counts
            .into_iter()
            .filter(|(_, keys)| *keys >= MIN_ALIGN_KEYS)
            .map(|(station, _)| station)
            .collect();
        let picked = ready.get(self.align_turn % ready.len().max(1))?.to_string();
        self.align_turn = self.align_turn.wrapping_add(1);
        Some(picked)
    }

    pub(super) fn turn_station(&mut self, station: &str, step_deg: f32) {
        if step_deg == 0.0 {
            return;
        }
        let turned: Vec<usize> = self
            .votes
            .keys
            .iter()
            .enumerate()
            .filter(|(_, key)| key.station == station)
            .map(|(index, _)| index)
            .collect();
        for index in turned {
            self.votes.keys[index].rotate(-step_deg);
            self.refresh_key(index, true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::df_fusion::observation::{RING, Rings};

    #[test]
    fn the_vertex_of_a_parabola_gives_offset_and_spread() {
        let profile: Vec<f32> = (-SEARCH_DEG..=SEARCH_DEG)
            .map(|deg| {
                let off = deg as f32 - 2.3;
                -off * off / (2.0 * 0.4 * 0.4)
            })
            .collect();
        let found = vertex(&profile).expect("a vertex");
        assert!((found.deg - 2.3).abs() < 1e-3, "{found:?}");
        assert!((found.sigma_deg - 0.4).abs() < 1e-3, "{found:?}");
        let mut edge = profile.clone();
        edge[0] = 10.0;
        assert_eq!(vertex(&edge), None);
        assert_eq!(vertex(&[0.0; 21]), None);
    }

    #[test]
    fn turning_rings_there_and_back_restores_them() {
        let mut rings: Box<Rings> = Box::new([[0.0; RING]; 8]);
        for ring in rings.iter_mut() {
            ring[40] = 1.0;
        }
        rotate(&mut rings, 5.0);
        assert!((rings[0][45] - 1.0).abs() < 1e-6);
        rotate(&mut rings, -5.0);
        assert!((rings[3][40] - 1.0).abs() < 1e-6);
        assert!(rings[3][45].abs() < 1e-6);
    }
}
