use std::f32::consts::FRAC_1_SQRT_2;

use num_complex::Complex;

use super::clutter::ClutterField;
use super::master::{Master, Symbols};
use super::ray::Ray;
use super::rays::rays;
use super::rotor::Rotor;
use super::{CHUNK, LaneSetup, amplitude, cis};
use crate::math::complex::{narrow, widen};
use crate::math::rng::Rng;
use crate::scene::world::{Ripple, Scene};

const ANTI_ALIAS: f64 = 0.9;
const PER_PPM: f64 = 1e-6;

type C32 = Complex<f32>;
type C64 = Complex<f64>;

struct NoiseFeed {
    master: Master,
    level: f64,
}

impl NoiseFeed {
    fn add(&mut self, arrival_s: f64, step_s: f64, out: &mut [C32]) {
        for (n, slot) in out.iter_mut().enumerate() {
            *slot += narrow(self.master.at(arrival_s + n as f64 * step_s) * self.level);
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Tone {
    offset_hz: f64,
    level: f64,
}

/// Renders what one antenna element and its radio see of a `Scene`.
pub struct LaneRenderer {
    rays: Vec<Ray>,
    clutter: Option<ClutterField>,
    noise: Option<NoiseFeed>,
    isolated: bool,
    pilot: Option<Tone>,
    ripple: Option<Ripple>,
    response: C64,
    hardware_delay_s: f64,
    carrier_error_hz: f64,
    thermal_sigma: f32,
    dc: C32,
    rng: Rng,
    antenna: Vec<C32>,
    echo: Vec<C32>,
}

impl LaneRenderer {
    #[must_use]
    pub fn new(thermal_seed: u64) -> Self {
        Self {
            rays: Vec::new(),
            clutter: None,
            noise: None,
            isolated: false,
            pilot: None,
            ripple: None,
            response: C64::new(1.0, 0.0),
            hardware_delay_s: 0.0,
            carrier_error_hz: 0.0,
            thermal_sigma: 0.0,
            dc: C32::new(0.0, 0.0),
            rng: Rng::mixed(thermal_seed),
            antenna: vec![C32::new(0.0, 0.0); CHUNK],
            echo: vec![C32::new(0.0, 0.0); CHUNK],
        }
    }

    pub fn configure(&mut self, scene: &Scene, setup: &LaneSetup<'_>) {
        let lane = setup.impairments;
        self.rays = rays(scene, setup);
        self.clutter = ClutterField::of(scene, setup);
        let noise_rate = scene.noise_bandwidth.min(ANTI_ALIAS * setup.sample_rate);
        self.noise = setup.noise_seed.map(|seed| NoiseFeed {
            master: Master::new(seed, noise_rate, Symbols::Gaussian),
            level: amplitude(scene.noise_source_dbfs),
        });
        self.isolated = setup.isolated;
        self.pilot = setup
            .pilot
            .filter(|pilot| setup.in_band(pilot.offset_hz, 0.0))
            .map(|pilot| Tone {
                offset_hz: pilot.offset_hz,
                level: amplitude(pilot.power_dbfs),
            });
        let turn = (lane.phase_deg + lane.phase_per_db * setup.gain_setting_db).to_radians();
        self.response = amplitude(lane.gain_db) * cis(turn + setup.scramble_rad);
        self.ripple = lane
            .ripple
            .filter(|ripple| ripple.cycles > 0 && ripple.depth_db > 0.0);
        self.hardware_delay_s = lane.frac_delay / setup.sample_rate;
        self.carrier_error_hz = lane.ppm * PER_PPM * setup.center_hz;
        self.thermal_sigma = (amplitude(scene.thermal_dbfs) as f32) * FRAC_1_SQRT_2;
        self.dc = lane.dc_dbfs.map_or(C32::new(0.0, 0.0), |db| {
            narrow(amplitude(db) * cis(lane.dc_phase_deg.to_radians()))
        });
    }

    pub fn render(&mut self, h: i64, start_s: f64, step_s: f64, noise_on: bool, out: &mut [C32]) {
        for (index, chunk) in out.chunks_mut(CHUNK).enumerate() {
            let offset = index * CHUNK;
            let start = start_s + offset as f64 * step_s;
            self.render_chunk(h + offset as i64, start, step_s, noise_on, chunk);
        }
    }

    fn front(&mut self, arrival_s: f64, step_s: f64, noise_on: bool, len: usize) {
        let mut antenna = std::mem::take(&mut self.antenna);
        self.sources(arrival_s, step_s, noise_on, &mut antenna[..len]);
        if let Some(ripple) = self.ripple {
            let reach = f64::from(ripple.cycles) * step_s;
            let half = 0.5 * ripple.depth();
            let mut echo = std::mem::take(&mut self.echo);
            for shift in [-reach, reach] {
                self.sources(arrival_s + shift, step_s, noise_on, &mut echo[..len]);
                for (slot, value) in antenna.iter_mut().zip(&echo[..len]) {
                    *slot += value * half as f32;
                }
            }
            self.echo = echo;
        }
        self.antenna = antenna;
    }

    fn sources(&mut self, arrival_s: f64, step_s: f64, noise_on: bool, out: &mut [C32]) {
        out.fill(C32::new(0.0, 0.0));
        if !(noise_on && self.noise.is_some() && self.isolated) {
            for ray in &mut self.rays {
                ray.add(arrival_s, step_s, out);
            }
        }
        if noise_on && let Some(noise) = &mut self.noise {
            noise.add(arrival_s, step_s, out);
        }
        if let Some(pilot) = self.pilot {
            let mut tone = Rotor::new(pilot.offset_hz, arrival_s, step_s);
            for slot in out.iter_mut() {
                *slot += narrow(tone.next() * pilot.level);
            }
        }
    }

    fn render_chunk(&mut self, h: i64, start_s: f64, step_s: f64, noise_on: bool, out: &mut [C32]) {
        let arrival_s = start_s - self.hardware_delay_s;
        self.front(arrival_s, step_s, noise_on, out.len());
        let antenna = &mut self.antenna[..out.len()];
        if !(noise_on && self.noise.is_some() && self.isolated)
            && let Some(clutter) = &mut self.clutter
        {
            clutter.add(h, arrival_s, step_s, antenna);
        }
        let mut error = Rotor::new(self.carrier_error_hz, start_s, step_s);
        for (slot, value) in out.iter_mut().zip(antenna.iter()) {
            let shaped = narrow(self.response * error.next() * widen(*value));
            *slot = shaped + self.rng.normal_pair(self.thermal_sigma) + self.dc;
        }
    }

    #[must_use]
    pub fn clutter_taps(&self) -> Vec<(usize, C64)> {
        self.clutter
            .as_ref()
            .map_or_default(|clutter| clutter.taps.clone())
    }
}
