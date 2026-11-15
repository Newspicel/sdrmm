use std::f64::consts::SQRT_2;

use super::{LIKELIHOOD_POINTS, Rotation, SweepBearing, SweepConfig, SweepEstimator, SweepReject};
use crate::{
    math::db::Decibels,
    math::special::{erf, norm_deg, parabolic_peak},
};

pub(super) const TEMPLATE_POINTS: usize = 360;
pub(super) const LIKELIHOOD_FLOOR: f64 = 0.02;
const LEVEL_PERIOD_S: f64 = 0.05;
const CONFIDENCE_WINDOW_DEG: f64 = 5.0;
const CURVATURE_STEP_DEG: f64 = 0.5;
const MIN_FIT_SAMPLES: usize = 4;
const WIDEST_SIGMA_DEG: f64 = 180.0;
const ONSET_DEG: f64 = 1.0;
const CONTRAST_LOW_QUANTILE: f64 = 0.05;
const CONTRAST_HIGH_QUANTILE: f64 = 0.95;
/// Level offset, beam scale and bearing: the degrees of freedom the fit spends.
const FIT_PARAMETERS: f64 = 3.0;
/// Keeps the noise estimate of a near perfect fit from collapsing to zero.
const MIN_NOISE_FRACTION: f64 = 1e-12;

struct Fit {
    bearing_deg: f64,
    fit: f64,
    sigma_deg: f64,
    bank_sigma_deg: f64,
    curvature: f64,
    s_ll: f64,
    rss_best: f64,
    noise_var: f64,
}

/// The antenna's gain in dB at each degree off boresight: a cardioid power raised to give
/// `beamwidth_deg` at half power, never deeper than the front-to-back ratio.
pub(super) fn beam_template(config: &SweepConfig, template: &mut [f32; TEMPLATE_POINTS]) {
    let half = (config.beamwidth_deg / 2.0).to_radians().cos();
    let power = 0.5f64.ln() / ((1.0 + half) / 2.0).ln();
    for (degree, value) in template.iter_mut().enumerate() {
        let cosine = (degree as f64).to_radians().cos();
        let gain = power * ((1.0 + cosine) / 2.0).power_to_db();
        *value = gain.max(-config.front_back_db) as f32;
    }
}

impl SweepEstimator {
    pub(super) fn evaluate(
        &mut self,
        end: usize,
        sign: f64,
        out: &mut SweepBearing,
    ) -> Result<(), SweepReject> {
        let count = end + 1;
        let headings = &self.track.heading[..count];
        let lowest = headings.iter().copied().fold(f64::INFINITY, f64::min);
        let highest = headings.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let span = highest - lowest;
        if span < self.config.min_span_deg {
            return Err(SweepReject::ShortSpan);
        }
        let contrast = self.contrast(count);
        if contrast < self.config.min_contrast_db {
            return Err(SweepReject::LowContrast);
        }
        let mean_sigma = self.track.sigma[..count]
            .iter()
            .map(|&s| f64::from(s))
            .sum::<f64>()
            / count as f64;
        if mean_sigma > f64::from(self.config.max_heading_sigma_deg) {
            return Err(SweepReject::HeadingPoor);
        }
        let onset = self.track.onset(ONSET_DEG);
        let duration = self.track.t[end] - self.track.t[onset.min(end)];
        let rate = if duration > 0.0 { span / duration } else { 0.0 };
        let fit = self.fit(count, rate, mean_sigma)?;
        self.fill_likelihood(&fit, out);
        out.bearing_deg = norm_deg(fit.bearing_deg);
        out.sigma_deg = fit.sigma_deg as f32;
        out.confidence = (erf(CONFIDENCE_WINDOW_DEG / (fit.sigma_deg * SQRT_2)) * fit.fit) as f32;
        out.fit = fit.fit as f32;
        out.contrast_db = contrast;
        out.span_deg = span as f32;
        out.rate_dps = rate as f32;
        out.rotation = if sign > 0.0 {
            Rotation::Clockwise
        } else {
            Rotation::Counterclockwise
        };
        out.samples = count.min(usize::from(u16::MAX)) as u16;
        out.lag_s = self.lag_s as f32;
        self.learn_lag(out.bearing_deg, rate, out.rotation, self.track.t[end]);
        Ok(())
    }

    pub(super) fn template_at(&self, offset_deg: f64) -> f64 {
        let position = norm_deg(offset_deg);
        let low = (position as usize).min(TEMPLATE_POINTS - 1);
        let high = (low + 1) % TEMPLATE_POINTS;
        let weight = position - low as f64;
        f64::from(self.template[low]) * (1.0 - weight) + f64::from(self.template[high]) * weight
    }

    fn contrast(&mut self, count: usize) -> f32 {
        let levels = &mut self.scratch[..count];
        levels.copy_from_slice(&self.track.level[..count]);
        levels.sort_unstable_by(f32::total_cmp);
        let at = |fraction: f64| levels[((count - 1) as f64 * fraction).round() as usize];
        at(CONTRAST_HIGH_QUANTILE) - at(CONTRAST_LOW_QUANTILE)
    }

    fn correlation_at(&self, count: usize, bearing_deg: f64, mean_level: f64, s_ll: f64) -> f64 {
        let (mut sum_g, mut sum_gg, mut sum_lg) = (0.0, 0.0, 0.0);
        for (&heading, &level) in self.track.heading[..count]
            .iter()
            .zip(&self.track.level[..count])
        {
            let g = self.template_at(heading - bearing_deg);
            sum_g += g;
            sum_gg += g * g;
            sum_lg += (f64::from(level) - mean_level) * g;
        }
        let s_gg = sum_gg - sum_g * sum_g / count as f64;
        if s_gg <= 0.0 || s_ll <= 0.0 {
            return 0.0;
        }
        sum_lg / (s_ll * s_gg).sqrt()
    }

    fn level_moments(&self, count: usize) -> (f64, f64) {
        let levels = &self.track.level[..count];
        let mean = levels.iter().map(|&l| f64::from(l)).sum::<f64>() / count as f64;
        let s_ll = levels
            .iter()
            .map(|&l| (f64::from(l) - mean).powi(2))
            .sum::<f64>();
        (mean, s_ll)
    }

    /// Correlates the levels with the beam at every whole degree, refines the best one, and
    /// reads the bearing's spread off the curvature of the fit around it.
    fn fit(&mut self, count: usize, rate: f64, mean_sigma: f64) -> Result<Fit, SweepReject> {
        if count < MIN_FIT_SAMPLES {
            return Err(SweepReject::PoorFit);
        }
        let (mean, s_ll) = self.level_moments(count);
        for degree in 0..LIKELIHOOD_POINTS {
            self.correlation[degree] = self.correlation_at(count, degree as f64, mean, s_ll);
        }
        let best = (0..LIKELIHOOD_POINTS)
            .max_by(|&a, &b| self.correlation[a].total_cmp(&self.correlation[b]))
            .unwrap_or(0);
        if self.correlation[best] <= 0.0 {
            return Err(SweepReject::PoorFit);
        }
        let before = self.correlation[(best + LIKELIHOOD_POINTS - 1) % LIKELIHOOD_POINTS];
        let after = self.correlation[(best + 1) % LIKELIHOOD_POINTS];
        let bearing = best as f64 + parabolic_peak(before, self.correlation[best], after);
        let fit = self
            .correlation_at(count, bearing, mean, s_ll)
            .max(self.correlation[best]);
        if fit < f64::from(self.config.min_fit) {
            return Err(SweepReject::PoorFit);
        }
        let rss = |rho: f64| s_ll * (1.0 - rho.max(0.0).powi(2));
        let rss_best = rss(fit);
        let noise_var =
            (rss_best / (count as f64 - FIT_PARAMETERS).max(1.0)).max(MIN_NOISE_FRACTION * s_ll);
        let scale = 2.0 * noise_var * self.config.sample_correlation;
        let raw = |b: f64| -(rss(self.correlation_at(count, b, mean, s_ll)) - rss_best) / scale;
        let h = CURVATURE_STEP_DEG;
        let curvature = -(raw(bearing + h) + raw(bearing - h)) / (h * h);
        let raw_var = if curvature > 0.0 {
            1.0 / curvature
        } else {
            WIDEST_SIGMA_DEG * WIDEST_SIGMA_DEG
        };
        let smear = rate * LEVEL_PERIOD_S;
        let bank = raw_var + smear * smear;
        let sigma = (bank + mean_sigma * mean_sigma).sqrt();
        Ok(Fit {
            bearing_deg: bearing,
            fit,
            sigma_deg: sigma.min(WIDEST_SIGMA_DEG),
            bank_sigma_deg: bank.sqrt().min(WIDEST_SIGMA_DEG),
            curvature,
            s_ll,
            rss_best,
            noise_var,
        })
    }

    fn fill_likelihood(&self, fit: &Fit, out: &mut SweepBearing) {
        let sigma_sq = fit.bank_sigma_deg * fit.bank_sigma_deg;
        let temper = if fit.curvature > 0.0 {
            (1.0 / (fit.curvature * sigma_sq)).min(1.0)
        } else {
            1.0
        };
        let floor = LIKELIHOOD_FLOOR.ln();
        let scale = 2.0 * fit.noise_var * self.config.sample_correlation;
        for (value, &rho) in out.likelihood.iter_mut().zip(&self.correlation) {
            let rss = fit.s_ll * (1.0 - rho.max(0.0).powi(2));
            let raw = -(rss - fit.rss_best).max(0.0) / scale;
            *value = (temper * raw).max(floor) as f32;
        }
    }
}
