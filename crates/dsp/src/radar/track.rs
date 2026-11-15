mod kalman;
mod target;
mod tracker;

pub use tracker::Tracker;

pub const MAX_TRACKS: usize = 64;
pub const MAX_TENTATIVE: usize = 128;
pub const MAX_MEASUREMENTS: usize = 128;
pub const MAX_WINDOW: u32 = 16;
pub const TRAIL_LEN: usize = 16;
pub const AOA_HISTORY: usize = 5;

const MIN_NOISE_FACTOR: f64 = 0.05;
const MAX_NOISE_FACTOR: f64 = 0.5;
const ACCEL_GATE_SIGMAS: f64 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackerConfig {
    pub wavelength_m: f64,
    pub confirm_hits: u32,
    pub confirm_window: u32,
    pub coast_looks: u32,
    pub max_accel: f64,
    pub gate: f64,
    pub jerk: f64,
    pub range_resolution_m: f64,
    pub doppler_resolution_hz: f64,
}

impl TrackerConfig {
    #[must_use]
    pub fn valid(&self) -> bool {
        let positive = |value: f64| value.is_finite() && value > 0.0;
        positive(self.wavelength_m)
            && positive(self.max_accel)
            && positive(self.gate)
            && positive(self.jerk)
            && positive(self.range_resolution_m)
            && positive(self.doppler_resolution_hz)
            && self.confirm_hits >= 1
            && self.confirm_hits <= self.confirm_window
            && self.confirm_window <= MAX_WINDOW
    }

    fn noise(&self, snr: f64) -> (f64, f64) {
        let factor = if snr > 0.0 && snr.is_finite() {
            (1.0 / (2.0 * snr).sqrt()).clamp(MIN_NOISE_FACTOR, MAX_NOISE_FACTOR)
        } else {
            MAX_NOISE_FACTOR
        };
        (
            factor * self.range_resolution_m,
            factor * self.doppler_resolution_hz,
        )
    }

    const fn window_mask(&self) -> u32 {
        (1u32 << self.confirm_window) - 1
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TrackAoa {
    pub azimuth_deg: f32,
    pub quality: f32,
    pub sigma_deg: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Measurement {
    pub range_m: f64,
    pub doppler_hz: f64,
    pub snr: f64,
    pub aoa: Option<TrackAoa>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TrackView {
    pub id: u32,
    pub coasting: bool,
    pub range_m: f64,
    pub range_rate_mps: f64,
    pub accel_mps2: f64,
    pub range_sigma_m: f64,
    pub rate_sigma_mps: f64,
    pub doppler_hz: f64,
    pub snr: f64,
    pub looks: u32,
    pub misses: u32,
    pub aoa: Option<TrackAoa>,
    pub trail: [(f32, f32); TRAIL_LEN],
    pub trail_len: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrackerStats {
    pub dropped: u32,
    pub ignored: u32,
}

#[cfg(test)]
mod tests;
