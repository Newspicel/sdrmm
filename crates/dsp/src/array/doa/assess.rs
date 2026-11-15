use super::{
    C32, DETECTION_SIGMAS, Doa, DoaError, DoaReport, Found, MAX_PEAKS, MIN_SHARE, MIN_SNR_DB, Peak,
    Stats, capon, confidence, crb_sigma_rad, joint_fit, mismatch_share, mismatch_sigma_deg,
    model_sigma_rad, total_sigma_deg,
};
use crate::array::manifold::{Direction, MAX_ELEMENTS, Manifold};
use crate::math::db::Decibels;
use crate::math::linalg::{CMat, norm_sqr};

/// Ambiguity of a bearing that has a mirror or an alias it cannot be told from.
const TWIN_AMBIGUITY: f32 = 0.5;

impl Doa {
    pub(super) fn capon_at(&self, manifold: &Manifold, direction: Direction) -> f32 {
        let n = self.manifold.len();
        let mut a = [C32::new(0.0, 0.0); MAX_ELEMENTS];
        let mut scratch = [C32::new(0.0, 0.0); MAX_ELEMENTS];
        manifold.steer(self.freq_hz, direction, &mut a[..n]);
        capon(&self.solvers.element_chol, &a[..n], &mut scratch)
    }

    pub(super) fn assess(
        &mut self,
        manifold: &Manifold,
        r: &CMat,
        stats: Stats,
        cal_sigma_deg: f64,
        found: &[Found],
        out: &mut DoaReport,
    ) -> Result<(), DoaError> {
        let n = self.manifold.len();
        let mut ranked = [Found::default(); MAX_PEAKS];
        let count = found.len().min(MAX_PEAKS);
        for (slot, item) in ranked.iter_mut().zip(found) {
            *slot = Found {
                power: self.capon_at(
                    manifold,
                    Direction::new(item.azimuth_deg, item.elevation_deg),
                ),
                ..*item
            };
        }
        ranked[..count].sort_unstable_by(|a, b| b.power.total_cmp(&a.power));
        let mut steering = [C32::new(0.0, 0.0); MAX_PEAKS * MAX_ELEMENTS];
        let mut signal = 0.0f64;
        for (k, item) in ranked[..count].iter().enumerate() {
            let a = &mut steering[k * n..(k + 1) * n];
            let direction = Direction::new(item.azimuth_deg, item.elevation_deg);
            manifold.steer(self.freq_hz, direction, a);
            let (peak, share) = self.peak_quality(manifold, r, stats, cal_sigma_deg, item, a);
            out.peaks[k] = peak;
            signal += share;
        }
        out.peak_count = count;
        let mut scratch = [C32::new(0.0, 0.0); MAX_PEAKS * MAX_ELEMENTS];
        out.fit = joint_fit(
            &steering[..count * n],
            count,
            n,
            r,
            &mut self.solvers.fit_chol,
            &mut scratch,
        )?;
        let share = mismatch_share(out.fit, r.trace_re(), stats.noise, n, count);
        for peak in &mut out.peaks[..count] {
            peak.sigma_deg = mismatch_sigma_deg(peak.sigma_deg, share);
            peak.confidence = confidence(f64::from(peak.sigma_deg), peak.ambiguity);
        }
        let noise = f64::from(stats.noise);
        out.snr_db = if count > 0 {
            ((signal / noise) as f32).power_to_db()
        } else {
            let top = f64::from(out.eigenvalues[0]);
            (((top / noise - 1.0) / stats.order as f64) as f32).power_to_db()
        }
        .max(MIN_SNR_DB);
        Ok(())
    }

    pub(super) fn peak_quality(
        &self,
        manifold: &Manifold,
        r: &CMat,
        stats: Stats,
        cal_sigma_deg: f64,
        item: &Found,
        a: &[C32],
    ) -> (Peak, f64) {
        let n = a.len();
        let noise = f64::from(stats.noise);
        let margin = 1.0 + DETECTION_SIGMAS / stats.snapshots.max(1.0).sqrt();
        let share = (f64::from(item.power) - margin * noise / n as f64).max(MIN_SHARE * noise);
        let snr = share / noise;
        let direction = Direction::new(item.azimuth_deg, item.elevation_deg);
        let mut rates = [0.0f64; MAX_ELEMENTS];
        manifold.phase_rates(self.freq_hz, direction, &mut rates[..n]);
        let sigma = sigma_deg(&rates[..n], snr, stats.snapshots, cal_sigma_deg);
        let sigma_el_deg = if self.grid.spec().elevation.is_some() {
            manifold.elevation_rates(self.freq_hz, direction, &mut rates[..n]);
            sigma_deg(&rates[..n], snr, stats.snapshots, cal_sigma_deg)
        } else {
            0.0
        };
        let mirror_deg = self.mirror_of(item.azimuth_deg);
        let aliased = self.alias.aliased && stats.sources == 1;
        let ambiguity = if mirror_deg.is_some() || aliased {
            TWIN_AMBIGUITY
        } else {
            0.0
        };
        let norm = norm_sqr(a);
        let trace = r.trace_re();
        let fit = if norm > 0.0 && trace > 0.0 {
            (r.quad(a) / (norm * trace)).clamp(0.0, 1.0) * item.fit_scale
        } else {
            0.0
        };
        let peak = Peak {
            azimuth_deg: item.azimuth_deg,
            elevation_deg: item.elevation_deg,
            power: item.power,
            sigma_deg: sigma as f32,
            sigma_el_deg: sigma_el_deg as f32,
            confidence: confidence(sigma, ambiguity),
            fit,
            ambiguity,
            mirror_deg,
        };
        (peak, share)
    }
}

fn sigma_deg(rates: &[f64], snr: f64, snapshots: f64, cal_sigma_deg: f64) -> f64 {
    total_sigma_deg(
        crb_sigma_rad(rates, snr, rates.len(), snapshots),
        model_sigma_rad(rates, cal_sigma_deg),
    )
}
