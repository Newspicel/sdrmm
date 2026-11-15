use super::{
    C32, CURVATURE_STEP_DEG, Doa, DoaError, DoaReport, LIKELIHOOD_COARSE_DEG, LIKELIHOOD_FLOOR,
    LIKELIHOOD_POINTS, LIKELIHOOD_WINDOW_DEG, UlaSide, on_side, refine,
};
use crate::array::manifold::{Direction, MAX_ELEMENTS, Manifold, SteeringGrid};
use crate::math::linalg::{CMat, norm_sqr};

/// Byte value of the most likely bearing.
const TOP_LEVEL: f32 = 255.0;

impl Doa {
    pub fn likelihood(
        &self,
        manifold: &Manifold,
        r: &CMat,
        report: &DoaReport,
        out: &mut [f32; LIKELIHOOD_POINTS],
    ) -> Result<bool, DoaError> {
        let Some(primary) = report.peaks().first().filter(|_| report.squelch_open) else {
            return Ok(false);
        };
        self.check_input(manifold, r)?;
        let beam = Beam {
            manifold,
            r,
            freq_hz: self.freq_hz,
            elevation_deg: primary.elevation_deg,
            ring: &self.ring,
        };
        let peak = beam.peak(primary.azimuth_deg);
        let top = beam.power(peak);
        let scale =
            report.snapshots.max(1.0) / f64::from(report.noise_power).max(f64::MIN_POSITIVE);
        let side = self.excluded_side();
        let mut raw = [0.0f32; LIKELIHOOD_POINTS];
        for (degree, value) in raw.iter_mut().enumerate() {
            *value = if side.is_some_and(|centre| !on_side(degree as f64, centre)) {
                f32::NEG_INFINITY
            } else {
                (scale * (beam.at_degree(degree) - top)) as f32
            };
        }
        let h = CURVATURE_STEP_DEG.to_radians();
        let bend = beam.power(peak + CURVATURE_STEP_DEG) - 2.0 * top
            + beam.power(peak - CURVATURE_STEP_DEG);
        let curvature = -scale * bend / (h * h);
        temper_likelihood(
            &mut raw,
            curvature,
            f64::from(primary.sigma_deg).to_radians(),
        );
        *out = raw;
        Ok(true)
    }

    pub(super) fn excluded_side(&self) -> Option<f64> {
        match self.config.ula_side {
            UlaSide::Both => None,
            side => self.layout.side_centre_deg(side),
        }
    }
}

pub fn temper_likelihood(raw: &mut [f32; LIKELIHOOD_POINTS], curvature: f64, sigma_rad: f64) {
    let sharp = curvature * sigma_rad * sigma_rad;
    let tau = if sharp.is_finite() && sharp > 0.0 {
        sharp.recip().min(1.0)
    } else {
        1.0
    };
    let floor = LIKELIHOOD_FLOOR.ln();
    for value in raw.iter_mut() {
        let tempered = (f64::from(*value) * tau) as f32;
        *value = if tempered.is_nan() {
            floor
        } else {
            tempered.max(floor)
        };
    }
}

pub fn quantize_likelihood(ln: &[f32], out: &mut [u8]) {
    let floor = LIKELIHOOD_FLOOR.ln();
    for (byte, &value) in out.iter_mut().zip(ln) {
        let level = (TOP_LEVEL * (value - floor) / -floor).round();
        *byte = if level.is_nan() {
            0
        } else {
            level.clamp(0.0, TOP_LEVEL) as u8
        };
    }
}

/// Bartlett beam power of the primary peak's elevation row, for the likelihood curve.
struct Beam<'a> {
    manifold: &'a Manifold,
    r: &'a CMat,
    freq_hz: f64,
    elevation_deg: f64,
    ring: &'a SteeringGrid,
}

impl Beam<'_> {
    fn power(&self, azimuth_deg: f64) -> f64 {
        let n = self.manifold.len();
        let mut a = [C32::new(0.0, 0.0); MAX_ELEMENTS];
        self.manifold.steer(
            self.freq_hz,
            Direction::new(azimuth_deg, self.elevation_deg),
            &mut a[..n],
        );
        self.of(&a[..n])
    }

    fn at_degree(&self, degree: usize) -> f64 {
        if self.elevation_deg == 0.0 && self.ring.points() == LIKELIHOOD_POINTS {
            self.of(self.ring.vector(degree))
        } else {
            self.power(degree as f64)
        }
    }

    fn of(&self, a: &[C32]) -> f64 {
        let norm = f64::from(norm_sqr(a));
        if norm > 0.0 {
            f64::from(self.r.quad(a)) / norm
        } else {
            0.0
        }
    }

    fn peak(&self, around_deg: f64) -> f64 {
        let steps = (2.0 * LIKELIHOOD_WINDOW_DEG / LIKELIHOOD_COARSE_DEG).round() as i32;
        let mut best = (around_deg, f64::NEG_INFINITY);
        for step in 0..=steps {
            let azimuth =
                around_deg - LIKELIHOOD_WINDOW_DEG + f64::from(step) * LIKELIHOOD_COARSE_DEG;
            let power = self.power(azimuth);
            if power > best.1 {
                best = (azimuth, power);
            }
        }
        let (refined, power) = refine(|x| self.power(x), best.0, LIKELIHOOD_COARSE_DEG);
        if power >= best.1 { refined } else { best.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tempering_reaches_the_target_curvature_and_keeps_the_floor() {
        let sigma = 3f64.to_radians();
        let curvature = 1e4;
        let mut raw = [0.0f32; LIKELIHOOD_POINTS];
        for (i, value) in raw.iter_mut().enumerate() {
            let offset = (i as f64 - 180.0).to_radians();
            *value = (-0.5 * curvature * offset * offset) as f32;
        }
        temper_likelihood(&mut raw, curvature, sigma);
        let second = f64::from(raw[181] - 2.0 * raw[180] + raw[179]) / 1f64.to_radians().powi(2);
        assert!((-1.0 / second - sigma * sigma).abs() < 0.01 * sigma * sigma);
        assert!(raw.iter().all(|&v| v >= LIKELIHOOD_FLOOR.ln()));
        let mut flat = [-0.01f32; LIKELIHOOD_POINTS];
        temper_likelihood(&mut flat, 0.1, sigma);
        assert!(flat.iter().all(|&v| (v + 0.01).abs() < 1e-7));
    }

    #[test]
    fn quantized_likelihood_spans_the_byte() {
        let floor = LIKELIHOOD_FLOOR.ln();
        let mut out = [7u8; 5];
        quantize_likelihood(&[0.0, floor, floor / 2.0, 3.0, f32::NAN], &mut out);
        assert_eq!(out, [255, 0, 128, 255, 0]);
    }
}
