use num_complex::Complex;

use super::ray::{Fold, Ray, Wave};
use super::{LaneSetup, amplitude, cis};
use crate::array::manifold::{Direction, LIGHT_SPEED_M_S, Vec3, steer};
use crate::math::complex::widen;
use crate::math::rng::mix;
use crate::scene::world::{Echo, Scene};

type C32 = Complex<f32>;
type C64 = Complex<f64>;

/// Steering phase and propagation lead of one plane wave at one element.
pub(super) struct Arrival {
    pub(super) steer: C64,
    pub(super) lead_s: f64,
}

pub(super) fn arrival(element: Vec3, center_hz: f64, direction: Direction) -> Arrival {
    let mut response = [C32::new(0.0, 0.0)];
    steer(&[element], center_hz, direction, &mut response);
    Arrival {
        steer: widen(response[0]),
        lead_s: element.dot(direction.unit()) / LIGHT_SPEED_M_S,
    }
}

pub(super) fn emitter_seed(scene_seed: u64, emitter: usize) -> u64 {
    mix(scene_seed ^ mix(emitter as u64 + 1))
}

fn echo_ray(wave: Wave, setup: &LaneSetup<'_>, echo: &Echo, level: f64) -> Ray {
    let reach = arrival(
        setup.element(),
        setup.center_hz,
        Direction::horizon(echo.azimuth_deg),
    );
    let mut ray = wave.ray(
        reach.steer * level * amplitude(echo.gain_db),
        reach.lead_s - echo.delay_s,
    );
    let rate = echo.doppler_hz / setup.center_hz.max(1.0);
    ray.scale = 1.0 + rate;
    ray.doppler_hz = echo.doppler_hz;
    ray.fold = (rate != 0.0 && echo.delay_s > 0.0).then(|| Fold {
        rate,
        period_s: echo.delay_s / (2.0 * rate.abs()),
    });
    ray
}

pub(super) fn rays(scene: &Scene, setup: &LaneSetup<'_>) -> Vec<Ray> {
    let element = setup.element();
    let shift_hz = setup.radio_center_hz - setup.center_hz;
    let mut rays = Vec::new();
    for (index, emitter) in scene.emitters.iter().enumerate() {
        let offset = emitter.offset_hz + shift_hz;
        if !setup.in_band(offset, emitter.waveform.half_bandwidth_hz()) {
            continue;
        }
        let wave = Wave::of(emitter, offset, emitter_seed(scene.seed, index));
        let level = amplitude(emitter.power_dbfs);
        let direct = arrival(
            element,
            setup.center_hz,
            Direction::new(emitter.azimuth_deg, emitter.elevation_deg),
        );
        rays.push(wave.ray(direct.steer * level, direct.lead_s));
        for path in &emitter.paths {
            let reach = arrival(
                element,
                setup.center_hz,
                Direction::new(path.azimuth_deg, path.elevation_deg),
            );
            let gain = reach.steer * amplitude(path.gain_db) * cis(path.phase_deg.to_radians());
            rays.push(wave.ray(gain * level, reach.lead_s - path.delay_s));
        }
        for echo in scene.echoes.iter().filter(|echo| echo.emitter == index) {
            rays.push(echo_ray(wave, setup, echo, level));
        }
    }
    rays
}
