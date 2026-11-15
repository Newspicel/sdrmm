use super::kalman::Kalman;
use super::{
    ACCEL_GATE_SIGMAS, AOA_HISTORY, Measurement, TRAIL_LEN, TrackAoa, TrackView, TrackerConfig,
};
use crate::math::special::circular_mean_deg;

const M_PER_KM: f64 = 1000.0;

/// One tentative or confirmed target: its filter, hit history, bearings and trail.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Track {
    pub(super) id: u32,
    pub(super) filter: Kalman,
    pub(super) history: u32,
    pub(super) looks: u32,
    pub(super) misses: u32,
    pub(super) snr: f64,
    pub(super) hit: Option<usize>,
    aoa: [TrackAoa; AOA_HISTORY],
    aoa_len: u8,
    aoa_next: u8,
    trail: [(f32, f32); TRAIL_LEN],
    trail_len: u8,
    trail_next: u8,
}

impl Track {
    pub(super) fn start(measurement: &Measurement, index: usize, config: &TrackerConfig) -> Self {
        let mut track = Self {
            id: 0,
            filter: Kalman::start(measurement, config),
            history: 1,
            looks: 1,
            misses: 0,
            snr: measurement.snr,
            hit: Some(index),
            aoa: [TrackAoa::default(); AOA_HISTORY],
            aoa_len: 0,
            aoa_next: 0,
            trail: [(0.0, 0.0); TRAIL_LEN],
            trail_len: 0,
            trail_next: 0,
        };
        track.remember(measurement.aoa);
        track.mark(config.wavelength_m);
        track
    }

    pub(super) fn cost(
        &self,
        measurement: &Measurement,
        config: &TrackerConfig,
        dt: f64,
    ) -> Option<f64> {
        let innovation = self.filter.innovation(measurement, config)?;
        if innovation.distance.is_nan() || innovation.distance > config.gate {
            return None;
        }
        if self.id == 0 {
            let (_, sigma_doppler) = config.noise(measurement.snr);
            let reach =
                config.max_accel * dt / config.wavelength_m + ACCEL_GATE_SIGMAS * sigma_doppler;
            if innovation.residual[1].abs() > reach {
                return None;
            }
        }
        Some(innovation.distance + innovation.log_det)
    }

    pub(super) fn look(
        &mut self,
        measurement: Option<(usize, &Measurement)>,
        config: &TrackerConfig,
    ) {
        self.looks = self.looks.saturating_add(1);
        self.history <<= 1;
        self.hit = None;
        match measurement {
            Some((index, measurement)) if self.filter.correct(measurement, config) => {
                self.history |= 1;
                self.misses = 0;
                self.snr = measurement.snr;
                self.hit = Some(index);
                self.remember(measurement.aoa);
            }
            _ => self.misses += 1,
        }
        self.history &= config.window_mask();
        self.mark(config.wavelength_m);
    }

    pub(super) fn view(&self, lambda: f64) -> TrackView {
        let mut trail = [(0.0f32, 0.0f32); TRAIL_LEN];
        let len = usize::from(self.trail_len);
        let start = (usize::from(self.trail_next) + TRAIL_LEN - len) % TRAIL_LEN;
        for (offset, point) in trail.iter_mut().take(len).enumerate() {
            *point = self.trail[(start + offset) % TRAIL_LEN];
        }
        let p = &self.filter.p;
        TrackView {
            id: self.id,
            coasting: self.misses > 0,
            range_m: self.filter.x[0],
            range_rate_mps: self.filter.x[1],
            accel_mps2: self.filter.x[2],
            range_sigma_m: p[0][0].max(0.0).sqrt(),
            rate_sigma_mps: p[1][1].max(0.0).sqrt(),
            doppler_hz: self.filter.doppler_hz(lambda),
            snr: self.snr,
            looks: self.looks,
            misses: self.misses,
            aoa: self.mean_aoa(),
            trail,
            trail_len: self.trail_len,
        }
    }

    fn remember(&mut self, aoa: Option<TrackAoa>) {
        if let Some(aoa) = aoa.filter(|aoa| aoa.azimuth_deg.is_finite() && aoa.quality > 0.0) {
            self.aoa[usize::from(self.aoa_next)] = aoa;
            self.aoa_next = (self.aoa_next + 1) % AOA_HISTORY as u8;
            self.aoa_len = (self.aoa_len + 1).min(AOA_HISTORY as u8);
        }
    }

    fn mark(&mut self, lambda: f64) {
        let point = (
            (self.filter.x[0] / M_PER_KM) as f32,
            self.filter.doppler_hz(lambda) as f32,
        );
        self.trail[usize::from(self.trail_next)] = point;
        self.trail_next = (self.trail_next + 1) % TRAIL_LEN as u8;
        self.trail_len = (self.trail_len + 1).min(TRAIL_LEN as u8);
    }

    fn mean_aoa(&self) -> Option<TrackAoa> {
        let held = &self.aoa[..usize::from(self.aoa_len)];
        if held.is_empty() {
            return None;
        }
        let (mut sin, mut cos, mut quality) = (0.0f64, 0.0f64, 0.0f64);
        let mut sharpest = f32::INFINITY;
        for aoa in held {
            let (s, c) = f64::from(aoa.azimuth_deg).to_radians().sin_cos();
            let weight = f64::from(aoa.quality);
            sin += weight * s;
            cos += weight * c;
            quality += weight;
            sharpest = sharpest.min(aoa.sigma_deg);
        }
        let count = held.len() as f64;
        Some(TrackAoa {
            azimuth_deg: circular_mean_deg(sin, cos) as f32,
            quality: (quality / count) as f32,
            sigma_deg: (f64::from(sharpest) / count.sqrt()) as f32,
        })
    }
}
