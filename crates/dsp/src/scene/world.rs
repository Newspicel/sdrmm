mod layout;
mod preset;
mod validate;

pub use layout::{circle, line, uca_positions};
pub use preset::default_scene;
pub use validate::all_finite;

use crate::math::db::Decibels;

pub const MAX_BENCH_LANES: usize = 16;
pub const MAX_CLUTTER_ECHOES: u32 = 64;
pub const MAX_CLUTTER_DELAY_SAMPLES: u32 = 65_536;
pub const MAX_PPM: f64 = 1_000.0;
pub const MAX_RIPPLE_DB: f64 = 6.0;
pub const MAX_RIPPLE_CYCLES: u32 = 64;

pub const KRAKEN_RADIUS_M: f64 = 0.35;
pub const DONGLE_SPACING_M: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    pub positions: Vec<[f64; 3]>,
    pub emitters: Vec<Emitter>,
    pub echoes: Vec<Echo>,
    pub clutter: Clutter,
    pub thermal_dbfs: f64,
    pub noise_source_dbfs: f64,
    pub noise_bandwidth: f64,
    pub seed: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Emitter {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub offset_hz: f64,
    pub power_dbfs: f64,
    pub waveform: Waveform,
    pub paths: Vec<Path>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Waveform {
    Tone,
    Fm { deviation_hz: f64, rate_hz: f64 },
    Am { depth: f64, rate_hz: f64 },
    Noise { bandwidth_hz: f64 },
    Bpsk { symbol_rate: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Path {
    pub azimuth_deg: f64,
    pub elevation_deg: f64,
    pub delay_s: f64,
    pub gain_db: f64,
    pub phase_deg: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Echo {
    pub emitter: usize,
    pub azimuth_deg: f64,
    pub delay_s: f64,
    pub doppler_hz: f64,
    pub gain_db: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Clutter {
    pub echoes: u32,
    pub max_delay_samples: u32,
    pub gain_db: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneImpairments {
    pub start_offset: i64,
    pub ppm: f64,
    pub gain_db: f64,
    pub phase_deg: f64,
    pub phase_per_db: f64,
    pub frac_delay: f64,
    pub dc_dbfs: Option<f64>,
    pub dc_phase_deg: f64,
    pub scramble_on_retune: bool,
    pub ripple: Option<Ripple>,
    pub slips: Vec<Slip>,
    pub gaps: Vec<ReportedGap>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ripple {
    pub depth_db: f64,
    pub cycles: u32,
}

impl Ripple {
    #[must_use]
    pub fn depth(&self) -> f64 {
        self.depth_db.db_to_amplitude() - 1.0
    }

    #[must_use]
    pub fn gain_at(&self, cycles_per_sample: f64) -> f64 {
        1.0 + self.depth()
            * (std::f64::consts::TAU * f64::from(self.cycles) * cycles_per_sample).cos()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slip {
    pub at: u64,
    pub samples: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReportedGap {
    pub at: u64,
    pub missing: u64,
    pub reported: u64,
    pub error: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pilot {
    pub offset_hz: f64,
    pub power_dbfs: f64,
}

impl Waveform {
    #[must_use]
    pub fn half_bandwidth_hz(self) -> f64 {
        match self {
            Self::Tone => 0.0,
            Self::Fm {
                deviation_hz,
                rate_hz,
            } => deviation_hz + rate_hz,
            Self::Am { rate_hz, .. } => rate_hz,
            Self::Noise { bandwidth_hz } => bandwidth_hz / 2.0,
            Self::Bpsk { symbol_rate } => symbol_rate / 2.0,
        }
    }
}
