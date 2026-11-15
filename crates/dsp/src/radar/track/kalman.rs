use super::{Measurement, TrackerConfig};

type Mat3 = [[f64; 3]; 3];

/// Range, range rate and acceleration filter fed range and Doppler measurements, with
/// white-jerk process noise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Kalman {
    pub(super) x: [f64; 3],
    pub(super) p: Mat3,
}

pub(super) struct Innovation {
    pub(super) residual: [f64; 2],
    inverse: [[f64; 2]; 2],
    pub(super) distance: f64,
    pub(super) log_det: f64,
}

impl Kalman {
    pub(super) fn start(measurement: &Measurement, config: &TrackerConfig) -> Self {
        let (sigma_range, sigma_doppler) = config.noise(measurement.snr);
        let lambda = config.wavelength_m;
        let rate_sigma = lambda * sigma_doppler;
        let accel_sigma = config.max_accel / 2.0;
        Self {
            x: [measurement.range_m, -lambda * measurement.doppler_hz, 0.0],
            p: [
                [sigma_range * sigma_range, 0.0, 0.0],
                [0.0, rate_sigma * rate_sigma, 0.0],
                [0.0, 0.0, accel_sigma * accel_sigma],
            ],
        }
    }

    pub(super) fn predict(&mut self, dt: f64, jerk: f64) {
        let f = [[1.0, dt, 0.5 * dt * dt], [0.0, 1.0, dt], [0.0, 0.0, 1.0]];
        let x = self.x;
        for (row, out) in f.iter().zip(self.x.iter_mut()) {
            *out = row[0] * x[0] + row[1] * x[1] + row[2] * x[2];
        }
        let mut p = multiply_transposed(&multiply(&f, &self.p), &f);
        let (t2, t3) = (dt * dt, dt * dt * dt);
        let (t4, t5) = (t3 * dt, t3 * t2);
        let q = [
            [t5 / 20.0, t4 / 8.0, t3 / 6.0],
            [t4 / 8.0, t3 / 3.0, t2 / 2.0],
            [t3 / 6.0, t2 / 2.0, dt],
        ];
        for (row, q_row) in p.iter_mut().zip(&q) {
            for (value, q_value) in row.iter_mut().zip(q_row) {
                *value += jerk * q_value;
            }
        }
        self.p = p;
    }

    pub(super) fn doppler_hz(&self, lambda: f64) -> f64 {
        -self.x[1] / lambda
    }

    pub(super) fn innovation(
        &self,
        measurement: &Measurement,
        config: &TrackerConfig,
    ) -> Option<Innovation> {
        let lambda = config.wavelength_m;
        let (sigma_range, sigma_doppler) = config.noise(measurement.snr);
        let p = &self.p;
        let s = [
            [p[0][0] + sigma_range * sigma_range, -p[0][1] / lambda],
            [
                -p[1][0] / lambda,
                p[1][1] / (lambda * lambda) + sigma_doppler * sigma_doppler,
            ],
        ];
        let det = s[0][0] * s[1][1] - s[0][1] * s[1][0];
        if !(det > 0.0 && det.is_finite()) {
            return None;
        }
        let inverse = [
            [s[1][1] / det, -s[0][1] / det],
            [-s[1][0] / det, s[0][0] / det],
        ];
        let residual = [
            measurement.range_m - self.x[0],
            measurement.doppler_hz - self.doppler_hz(lambda),
        ];
        let distance = residual[0] * (inverse[0][0] * residual[0] + inverse[0][1] * residual[1])
            + residual[1] * (inverse[1][0] * residual[0] + inverse[1][1] * residual[1]);
        Some(Innovation {
            residual,
            inverse,
            distance,
            log_det: det.ln(),
        })
    }

    pub(super) fn correct(&mut self, measurement: &Measurement, config: &TrackerConfig) -> bool {
        let Some(innovation) = self.innovation(measurement, config) else {
            return false;
        };
        let lambda = config.wavelength_m;
        let (sigma_range, sigma_doppler) = config.noise(measurement.snr);
        let p = self.p;
        let pht = [
            [p[0][0], -p[0][1] / lambda],
            [p[1][0], -p[1][1] / lambda],
            [p[2][0], -p[2][1] / lambda],
        ];
        let inverse = innovation.inverse;
        let mut gain = [[0.0; 2]; 3];
        for (row, source) in gain.iter_mut().zip(&pht) {
            for (col, value) in row.iter_mut().enumerate() {
                *value = source[0] * inverse[0][col] + source[1] * inverse[1][col];
            }
        }
        let mut x = self.x;
        for (value, row) in x.iter_mut().zip(&gain) {
            *value += row[0] * innovation.residual[0] + row[1] * innovation.residual[1];
        }
        let mut reduce = [[0.0; 3]; 3];
        for (i, row) in reduce.iter_mut().enumerate() {
            row[i] = 1.0;
            row[0] -= gain[i][0];
            row[1] += gain[i][1] / lambda;
        }
        let mut updated = multiply_transposed(&multiply(&reduce, &p), &reduce);
        let noise = [sigma_range * sigma_range, sigma_doppler * sigma_doppler];
        for (i, row) in updated.iter_mut().enumerate() {
            for (j, value) in row.iter_mut().enumerate() {
                *value += gain[i][0] * noise[0] * gain[j][0] + gain[i][1] * noise[1] * gain[j][1];
            }
        }
        let total: f64 = x.iter().chain(updated.iter().flatten()).sum();
        if !total.is_finite() {
            return false;
        }
        self.x = x;
        self.p = updated;
        true
    }
}

fn multiply(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn multiply_transposed(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [[0.0; 3]; 3];
    for (i, row) in out.iter_mut().enumerate() {
        for (j, value) in row.iter_mut().enumerate() {
            *value = (0..3).map(|k| a[i][k] * b[j][k]).sum();
        }
    }
    out
}
