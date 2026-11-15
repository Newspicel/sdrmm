use std::ops::Range;

use super::MAX_DOPPLER_TAPS;
use crate::radar::RadarDspError;
use crate::radar::batch::BatchShape;

/// How the batches of one CPI split into groups that each get their own clutter weights.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupPlan {
    pub bounds: Vec<usize>,
    pub extension: usize,
    pub doppler_taps: usize,
    pub taper: bool,
    pub sliding: bool,
}

impl GroupPlan {
    pub fn split(
        batches: usize,
        group_batches: usize,
        extension: usize,
        doppler_taps: usize,
        taper: bool,
        sliding: bool,
    ) -> Result<Self, RadarDspError> {
        if batches == 0 || doppler_taps > MAX_DOPPLER_TAPS {
            return Err(RadarDspError::Setting);
        }
        let per_group = group_batches.max(1);
        let groups = ((batches as f64 / per_group as f64).round() as usize).clamp(1, batches);
        let bounds = (0..=groups).map(|g| g * batches / groups).collect();
        Ok(Self {
            bounds,
            extension,
            doppler_taps,
            taper,
            sliding,
        })
    }

    #[must_use]
    pub fn groups(&self) -> usize {
        self.bounds.len().saturating_sub(1)
    }

    #[must_use]
    pub const fn taps(&self) -> usize {
        2 * self.doppler_taps + 1
    }

    #[must_use]
    pub const fn shifts(&self) -> usize {
        4 * self.doppler_taps + 1
    }

    fn batches(&self) -> usize {
        self.bounds.last().copied().unwrap_or(0)
    }

    fn members(&self, group: usize) -> Range<usize> {
        self.bounds[group]..self.bounds[group + 1]
    }

    pub(super) fn estimation(&self, group: usize) -> Range<usize> {
        let members = self.members(group);
        if self.sliding {
            members.start.saturating_sub(self.extension)
                ..(members.end + self.extension).min(self.batches())
        } else {
            members
        }
    }

    pub(super) fn cycles(&self, group: usize, batch: usize) -> f64 {
        (batch as f64 + 0.5) / self.members(group).len() as f64
    }

    fn centre(&self, group: usize) -> f64 {
        let members = self.members(group);
        0.5 * (members.start + members.end) as f64
    }

    fn owner(&self, batch: usize) -> usize {
        self.bounds[1..]
            .iter()
            .position(|&end| batch < end)
            .unwrap_or(self.groups() - 1)
    }

    pub(super) fn blend(&self, batch: usize) -> (usize, usize, f32) {
        let own = self.owner(batch);
        if self.sliding || !self.taper {
            return (own, own, 0.0);
        }
        let last = self.groups() - 1;
        let x = batch as f64 + 0.5;
        if x <= self.centre(0) {
            return (0, 0, 0.0);
        }
        if x >= self.centre(last) {
            return (last, last, 0.0);
        }
        let group = (0..last)
            .find(|&g| x < self.centre(g + 1))
            .unwrap_or(last - 1);
        let (low, high) = (self.centre(group), self.centre(group + 1));
        (group, group + 1, ((x - low) / (high - low)) as f32)
    }

    pub(super) fn valid_for(&self, shape: &BatchShape) -> bool {
        self.groups() >= 1
            && self.bounds.first() == Some(&0)
            && self.batches() == shape.batches
            && self.bounds.windows(2).all(|pair| pair[0] < pair[1])
            && self.doppler_taps <= MAX_DOPPLER_TAPS
    }
}
