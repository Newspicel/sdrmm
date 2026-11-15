use super::MAX_PEAKS;
use crate::math::linalg::MAX_ORDER;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Peak {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub power: f32,
    pub sigma_deg: f32,
    pub sigma_el_deg: f32,
    pub confidence: f32,
    pub fit: f32,
    pub ambiguity: f32,
    pub mirror_deg: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DoaReport {
    pub peaks: [Peak; MAX_PEAKS],
    pub peak_count: usize,
    pub sources: usize,
    pub sources_raw: usize,
    pub squelch_open: bool,
    pub eigenvalues: [f32; MAX_ORDER],
    pub order: usize,
    pub eig_ratio_db: f32,
    pub lambda12_db: f32,
    pub snr_db: f32,
    pub noise_power: f32,
    pub snapshots: f64,
    pub fit: f32,
    pub loading_used: f32,
}

impl DoaReport {
    #[must_use]
    pub fn peaks(&self) -> &[Peak] {
        &self.peaks[..self.peak_count.min(MAX_PEAKS)]
    }
}
