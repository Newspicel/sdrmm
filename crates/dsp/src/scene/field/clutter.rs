use std::f64::consts::TAU;

use num_complex::Complex;

use super::CHUNK;
use super::ray::{Ray, Wave};
use super::rays::{arrival, emitter_seed};
use super::{LaneSetup, amplitude, cis};
use crate::array::manifold::Direction;
use crate::math::complex::{narrow, widen};
use crate::math::rng::Rng;
use crate::scene::world::Scene;

const CLUTTER_SALT: u64 = 0xC1_0773_2000;

type C32 = Complex<f32>;
type C64 = Complex<f64>;

/// Ground clutter: the first emitter's signal through fixed random delays, bearings and
/// phases.
pub(super) struct ClutterField {
    illuminator: Ray,
    pub(super) taps: Vec<(usize, C64)>,
    reach: usize,
    history: Vec<C32>,
    next_h: Option<i64>,
    last_len: usize,
}

impl ClutterField {
    pub(super) fn of(scene: &Scene, setup: &LaneSetup<'_>) -> Option<Self> {
        let clutter = scene.clutter;
        let emitter = scene.emitters.first()?;
        let offset = emitter.offset_hz + setup.radio_center_hz - setup.center_hz;
        if clutter.echoes == 0 || !setup.in_band(offset, emitter.waveform.half_bandwidth_hz()) {
            return None;
        }
        let reach = clutter.max_delay_samples as usize;
        let level = amplitude(emitter.power_dbfs) * amplitude(clutter.gain_db);
        let mut draw = Rng::mixed(scene.seed ^ CLUTTER_SALT);
        let taps = (0..clutter.echoes)
            .map(|_| {
                let delay = 1 + (draw.bits() % u64::from(clutter.max_delay_samples)) as usize;
                let azimuth = f64::from(draw.open_unit()) * 360.0;
                let phase = f64::from(draw.open_unit()) * TAU;
                let reach = arrival(
                    setup.element(),
                    setup.center_hz,
                    Direction::horizon(azimuth),
                );
                (delay, reach.steer * level * cis(phase))
            })
            .collect();
        let wave = Wave::of(emitter, offset, emitter_seed(scene.seed, 0));
        Some(Self {
            illuminator: wave.ray(C64::new(1.0, 0.0), 0.0),
            taps,
            reach,
            history: vec![C32::new(0.0, 0.0); reach + CHUNK],
            next_h: None,
            last_len: 0,
        })
    }

    pub(super) fn add(&mut self, h: i64, arrival_s: f64, step_s: f64, out: &mut [C32]) {
        let (reach, len) = (self.reach, out.len());
        if self.next_h == Some(h) {
            self.history
                .copy_within(self.last_len..self.last_len + reach, 0);
        } else {
            let head = &mut self.history[..reach];
            head.fill(C32::new(0.0, 0.0));
            self.illuminator
                .add(arrival_s - reach as f64 * step_s, step_s, head);
        }
        let body = &mut self.history[reach..reach + len];
        body.fill(C32::new(0.0, 0.0));
        self.illuminator.add(arrival_s, step_s, body);
        for (n, slot) in out.iter_mut().enumerate() {
            let mut acc = C64::new(0.0, 0.0);
            for (delay, coefficient) in &self.taps {
                acc += coefficient * widen(self.history[reach + n - delay]);
            }
            *slot += narrow(acc);
        }
        self.next_h = Some(h + len as i64);
        self.last_len = len;
    }
}
