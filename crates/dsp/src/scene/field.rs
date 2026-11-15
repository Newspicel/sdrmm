mod clutter;
mod lane;
mod master;
mod ray;
mod rays;
mod rotor;
mod sinc_table;

use std::f64::consts::TAU;

pub use lane::LaneRenderer;
use num_complex::Complex;

use super::world::{LaneImpairments, Pilot};
use crate::array::manifold::Vec3;
use crate::math::db::Decibels;

const CHUNK: usize = 8_192;
const TAPS: usize = 32;
const TAP_LEAD: i64 = 15;

type C64 = Complex<f64>;

#[derive(Clone, Debug)]
pub struct LaneSetup<'a> {
    pub position: [f64; 3],
    pub center_hz: f64,
    pub radio_center_hz: f64,
    pub sample_rate: f64,
    pub impairments: &'a LaneImpairments,
    pub gain_setting_db: f64,
    pub scramble_rad: f64,
    pub pilot: Option<Pilot>,
    pub noise_seed: Option<u64>,
    pub isolated: bool,
}

impl LaneSetup<'_> {
    fn element(&self) -> Vec3 {
        Vec3::new(self.position[0], self.position[1], self.position[2])
    }

    fn in_band(&self, offset_hz: f64, half_bandwidth_hz: f64) -> bool {
        offset_hz.abs() + half_bandwidth_hz <= 0.5 * self.sample_rate
    }
}

#[must_use]
pub fn amplitude(db: f64) -> f64 {
    db.db_to_amplitude()
}

fn cis(radians: f64) -> C64 {
    C64::from_polar(1.0, radians)
}

fn cycles(freq_hz: f64, time_s: f64) -> C64 {
    cis(TAU * (freq_hz * time_s).rem_euclid(1.0))
}
