use std::f64::consts::TAU;

use num_complex::Complex;

use super::{Direction, LIGHT_SPEED_M_S, Vec3};

#[must_use]
pub fn wavelength_m(freq_hz: f64) -> f64 {
    LIGHT_SPEED_M_S / freq_hz
}

#[must_use]
pub fn wavenumber(freq_hz: f64) -> f64 {
    TAU * freq_hz / LIGHT_SPEED_M_S
}

pub fn steer(positions: &[Vec3], freq_hz: f64, direction: Direction, out: &mut [Complex<f32>]) {
    let k = wavenumber(freq_hz);
    let unit = direction.unit();
    for (value, position) in out.iter_mut().zip(positions) {
        let (sin, cos) = (k * position.dot(unit)).sin_cos();
        *value = Complex::new(cos as f32, sin as f32);
    }
}

pub fn phase_rates(positions: &[Vec3], freq_hz: f64, direction: Direction, out: &mut [f64]) {
    let k = wavenumber(freq_hz);
    let (sin_az, cos_az) = direction.azimuth_deg.to_radians().sin_cos();
    let cos_el = direction.elevation_deg.to_radians().cos();
    for (rate, p) in out.iter_mut().zip(positions) {
        *rate = k * cos_el * (p.x * cos_az - p.y * sin_az);
    }
}

pub fn elevation_rates(positions: &[Vec3], freq_hz: f64, direction: Direction, out: &mut [f64]) {
    let k = wavenumber(freq_hz);
    let (sin_az, cos_az) = direction.azimuth_deg.to_radians().sin_cos();
    let (sin_el, cos_el) = direction.elevation_deg.to_radians().sin_cos();
    for (rate, p) in out.iter_mut().zip(positions) {
        *rate = k * (p.z * cos_el - sin_el * (p.x * sin_az + p.y * cos_az));
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::FRAC_PI_2;

    use super::*;
    use crate::array::manifold::{Geometry, Winding};

    const ONE_METRE_WAVE_HZ: f64 = LIGHT_SPEED_M_S;

    #[test]
    fn steering_matches_the_plane_wave_convention() {
        let positions = [Vec3::new(0.25, 0.0, 0.0)];
        let k = wavenumber(ONE_METRE_WAVE_HZ);
        let exact = k * positions[0].dot(Direction::horizon(90.0).unit());
        assert!((exact - FRAC_PI_2).abs() < 1e-9, "exact {exact}");
        let mut out = [Complex::new(0.0f32, 0.0)];
        steer(
            &positions,
            ONE_METRE_WAVE_HZ,
            Direction::horizon(90.0),
            &mut out,
        );
        assert!((f64::from(out[0].arg()) - FRAC_PI_2).abs() < 1e-6);
        steer(
            &positions,
            ONE_METRE_WAVE_HZ,
            Direction::horizon(270.0),
            &mut out,
        );
        assert!((f64::from(out[0].arg()) + FRAC_PI_2).abs() < 1e-6);
    }

    fn unwrapped_phase(positions: &[Vec3], freq: f64, direction: Direction, element: usize) -> f64 {
        wavenumber(freq) * positions[element].dot(direction.unit())
    }

    fn central_difference(
        positions: &[Vec3],
        freq: f64,
        plus: Direction,
        minus: Direction,
        h_deg: f64,
        element: usize,
    ) -> f64 {
        (unwrapped_phase(positions, freq, plus, element)
            - unwrapped_phase(positions, freq, minus, element))
            / (2.0 * h_deg.to_radians())
    }

    #[test]
    fn phase_rates_match_a_numerical_derivative() {
        let geometry = Geometry::uca(0.35, 5, 10.0, Winding::Clockwise).unwrap();
        let positions = geometry.positions();
        let freq = 433.92e6;
        let h = 1e-4;
        for (azimuth, elevation) in [(0.0, 0.0), (37.0, 20.0), (211.5, 45.0), (300.0, 5.0)] {
            let direction = Direction::new(azimuth, elevation);
            let mut rates = [0.0f64; 5];
            let mut up = [0.0f64; 5];
            phase_rates(positions, freq, direction, &mut rates);
            elevation_rates(positions, freq, direction, &mut up);
            for element in 0..5 {
                let numeric = central_difference(
                    positions,
                    freq,
                    Direction::new(azimuth + h, elevation),
                    Direction::new(azimuth - h, elevation),
                    h,
                    element,
                );
                assert!((numeric - rates[element]).abs() < 1e-6, "azimuth rate");
                let numeric = central_difference(
                    positions,
                    freq,
                    Direction::new(azimuth, elevation + h),
                    Direction::new(azimuth, elevation - h),
                    h,
                    element,
                );
                assert!((numeric - up[element]).abs() < 1e-6, "elevation rate");
            }
        }
    }
}
