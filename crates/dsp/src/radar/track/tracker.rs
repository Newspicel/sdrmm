use super::target::Track;
use super::{
    MAX_MEASUREMENTS, MAX_TENTATIVE, MAX_TRACKS, Measurement, TrackView, TrackerConfig,
    TrackerStats,
};
use crate::radar::RadarDspError;
use crate::radar::assign::Assignment;

/// M-of-N confirmed tracker that assigns measurements to tracks by Mahalanobis cost.
pub struct Tracker {
    config: TrackerConfig,
    confirmed: Vec<Track>,
    tentative: Vec<Track>,
    assignment: Assignment,
    matches: Vec<Option<usize>>,
    free: Vec<usize>,
    used: Vec<bool>,
    next_id: u32,
}

impl Tracker {
    pub fn new(config: TrackerConfig) -> Result<Self, RadarDspError> {
        if !config.valid() {
            return Err(RadarDspError::Setting);
        }
        let rows = MAX_TENTATIVE.max(MAX_TRACKS);
        Ok(Self {
            config,
            confirmed: Vec::with_capacity(MAX_TRACKS),
            tentative: Vec::with_capacity(MAX_TENTATIVE),
            assignment: Assignment::new(rows, MAX_MEASUREMENTS),
            matches: vec![None; rows],
            free: Vec::with_capacity(MAX_MEASUREMENTS),
            used: vec![false; MAX_MEASUREMENTS],
            next_id: 1,
        })
    }

    pub fn set_config(&mut self, config: TrackerConfig) -> Result<(), RadarDspError> {
        if !config.valid() {
            return Err(RadarDspError::Setting);
        }
        self.config = config;
        let mask = config.window_mask();
        for track in self.confirmed.iter_mut().chain(self.tentative.iter_mut()) {
            track.history &= mask;
        }
        Ok(())
    }

    #[must_use]
    pub const fn config(&self) -> TrackerConfig {
        self.config
    }

    #[must_use]
    pub fn confirmed(&self) -> usize {
        self.confirmed.len()
    }

    #[must_use]
    pub fn tentative(&self) -> usize {
        self.tentative.len()
    }

    #[must_use]
    pub const fn next_id(&self) -> u32 {
        self.next_id
    }

    pub fn resume_ids(&mut self, next_id: u32) {
        self.next_id = self.next_id.max(next_id);
    }

    pub fn reset(&mut self, ended: &mut Vec<u32>) {
        ended.clear();
        ended.extend(self.confirmed.iter().map(|track| track.id));
        self.confirmed.clear();
        self.tentative.clear();
    }

    pub fn update(
        &mut self,
        dt_s: f64,
        measurements: &[Measurement],
        assigned: &mut Vec<Option<u32>>,
        ended: &mut Vec<u32>,
    ) -> TrackerStats {
        let count = measurements.len().min(MAX_MEASUREMENTS);
        let mut stats = TrackerStats {
            dropped: 0,
            ignored: (measurements.len() - count) as u32,
        };
        let measurements = &measurements[..count];
        ended.clear();
        assigned.clear();
        assigned.resize(count, None);
        self.used[..count].fill(false);
        let dt = if dt_s.is_finite() && dt_s > 0.0 {
            dt_s
        } else {
            0.0
        };
        for track in self.confirmed.iter_mut().chain(self.tentative.iter_mut()) {
            track.filter.predict(dt, self.config.jerk);
        }
        stats.ignored += self.associate_confirmed(measurements, dt);
        stats.ignored += self.associate_tentative(measurements, dt);
        stats.ignored += self.unsound(measurements);
        stats.dropped += self.start_tentatives(measurements);
        stats.dropped += self.promote(ended);
        self.retire(ended);
        for track in &self.confirmed {
            if let Some(slot) = track.hit.and_then(|index| assigned.get_mut(index)) {
                *slot = Some(track.id);
            }
        }
        stats
    }

    pub fn for_each_confirmed(&self, mut f: impl FnMut(&TrackView)) {
        for track in &self.confirmed {
            f(&track.view(self.config.wavelength_m));
        }
    }

    fn associate_confirmed(&mut self, measurements: &[Measurement], dt: f64) -> u32 {
        let rows = self.confirmed.len();
        let config = self.config;
        let tracks = &self.confirmed;
        let solved = self.assignment.solve(
            rows,
            measurements.len(),
            |row, col| tracks[row].cost(&measurements[col], &config, dt),
            &mut self.matches[..rows],
        );
        let ignored = if solved.is_err() {
            self.matches[..rows].fill(None);
            measurements.len() as u32
        } else {
            0
        };
        for (track, pick) in self.confirmed.iter_mut().zip(&self.matches) {
            if let Some(col) = *pick {
                self.used[col] = true;
            }
            track.look(pick.map(|col| (col, &measurements[col])), &config);
        }
        ignored
    }

    fn associate_tentative(&mut self, measurements: &[Measurement], dt: f64) -> u32 {
        self.free.clear();
        let used = &self.used;
        self.free
            .extend((0..measurements.len()).filter(|&index| !used[index]));
        let rows = self.tentative.len();
        let config = self.config;
        let tracks = &self.tentative;
        let free = &self.free;
        let solved = self.assignment.solve(
            rows,
            free.len(),
            |row, col| tracks[row].cost(&measurements[free[col]], &config, dt),
            &mut self.matches[..rows],
        );
        let ignored = if solved.is_err() {
            self.matches[..rows].fill(None);
            free.len() as u32
        } else {
            0
        };
        for (track, pick) in self.tentative.iter_mut().zip(&self.matches) {
            let hit = pick.map(|col| self.free[col]);
            if let Some(index) = hit {
                self.used[index] = true;
            }
            track.look(hit.map(|index| (index, &measurements[index])), &config);
        }
        ignored
    }

    fn unsound(&self, measurements: &[Measurement]) -> u32 {
        measurements
            .iter()
            .zip(&self.used)
            .filter(|(measurement, used)| !**used && !measurement_is_sound(measurement))
            .count() as u32
    }

    fn start_tentatives(&mut self, measurements: &[Measurement]) -> u32 {
        let mut dropped = 0;
        for (index, measurement) in measurements.iter().enumerate() {
            if self.used[index] || !measurement_is_sound(measurement) {
                continue;
            }
            let track = Track::start(measurement, index, &self.config);
            if self.tentative.len() < MAX_TENTATIVE {
                self.tentative.push(track);
                continue;
            }
            dropped += 1;
            if let Some(weak) = weakest(&self.tentative)
                && self.tentative[weak].snr < track.snr
            {
                self.tentative[weak] = track;
            }
        }
        dropped
    }

    fn promote(&mut self, ended: &mut Vec<u32>) -> u32 {
        let mut dropped = 0;
        let hits = self.config.confirm_hits;
        let mut index = 0;
        while index < self.tentative.len() {
            if self.tentative[index].history.count_ones() < hits {
                index += 1;
                continue;
            }
            let mut track = self.tentative.remove(index);
            if self.confirmed.len() >= MAX_TRACKS {
                dropped += 1;
                match weakest(&self.confirmed) {
                    Some(weak) if self.confirmed[weak].snr < track.snr => {
                        ended.push(self.confirmed[weak].id);
                        self.confirmed.remove(weak);
                    }
                    _ => continue,
                }
            }
            track.id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1).max(1);
            self.confirmed.push(track);
        }
        dropped
    }

    fn retire(&mut self, ended: &mut Vec<u32>) {
        let window = self.config.confirm_window;
        self.tentative.retain(|track| track.looks < window);
        let limit = self.config.coast_looks;
        self.confirmed.retain(|track| {
            let alive = track.misses <= limit;
            if !alive {
                ended.push(track.id);
            }
            alive
        });
    }
}

fn measurement_is_sound(measurement: &Measurement) -> bool {
    measurement.range_m.is_finite() && measurement.doppler_hz.is_finite()
}

fn weakest(tracks: &[Track]) -> Option<usize> {
    tracks
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.snr.total_cmp(&b.1.snr))
        .map(|(index, _)| index)
}
