use crate::manifold::{Geometry, Winding};

pub const MAX_BENCH_LANES: usize = 16;
pub const MAX_CLUTTER_ECHOES: u32 = 64;
pub const MAX_CLUTTER_DELAY_SAMPLES: u32 = 65_536;
pub const MAX_PPM: f64 = 1_000.0;
pub const MAX_RIPPLE_DB: f64 = 6.0;
pub const MAX_RIPPLE_CYCLES: u32 = 64;

pub const KRAKEN_RADIUS_M: f64 = 0.35;
pub const DONGLE_SPACING_M: f64 = 0.5;

const DEFAULT_SEED: u64 = 0x5EED_BE7C_0000_0137;

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
        10f64.powf(self.depth_db / 20.0) - 1.0
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

    fn problem(self) -> Option<&'static str> {
        let sound = match self {
            Self::Tone => true,
            Self::Fm {
                deviation_hz,
                rate_hz,
            } => deviation_hz.is_finite() && deviation_hz >= 0.0 && positive(rate_hz),
            Self::Am { depth, rate_hz } => (0.0..=1.0).contains(&depth) && positive(rate_hz),
            Self::Noise { bandwidth_hz } => positive(bandwidth_hz),
            Self::Bpsk { symbol_rate } => positive(symbol_rate),
        };
        (!sound).then_some("an emitter waveform needs finite, positive rates")
    }
}

fn positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

#[must_use]
pub fn all_finite(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite())
}

impl Scene {
    #[must_use]
    pub fn problem(&self) -> Option<String> {
        if !self.positions.iter().all(|p| all_finite(p)) {
            return Some("element positions must be finite".to_owned());
        }
        if !all_finite(&[self.thermal_dbfs, self.noise_source_dbfs])
            || !positive(self.noise_bandwidth)
        {
            return Some("noise levels and the noise bandwidth must be finite".to_owned());
        }
        for (index, emitter) in self.emitters.iter().enumerate() {
            if let Some(problem) = emitter.problem() {
                return Some(format!("emitter {index}: {problem}"));
            }
        }
        for (index, echo) in self.echoes.iter().enumerate() {
            if echo.emitter >= self.emitters.len() {
                return Some(format!("echo {index} names a missing emitter"));
            }
            if !all_finite(&[
                echo.azimuth_deg,
                echo.delay_s,
                echo.doppler_hz,
                echo.gain_db,
            ]) {
                return Some(format!("echo {index} must be finite"));
            }
        }
        self.clutter_problem()
    }

    fn clutter_problem(&self) -> Option<String> {
        let clutter = self.clutter;
        if clutter.echoes == 0 {
            return None;
        }
        if self.emitters.is_empty() {
            return Some("clutter needs an emitter to reflect".to_owned());
        }
        let sound = clutter.echoes <= MAX_CLUTTER_ECHOES
            && (1..=MAX_CLUTTER_DELAY_SAMPLES).contains(&clutter.max_delay_samples)
            && clutter.gain_db.is_finite();
        (!sound).then(|| {
            format!(
                "clutter takes at most {MAX_CLUTTER_ECHOES} echoes within 1 to \
                 {MAX_CLUTTER_DELAY_SAMPLES} samples"
            )
        })
    }
}

impl Emitter {
    fn problem(&self) -> Option<&'static str> {
        if !all_finite(&[
            self.azimuth_deg,
            self.elevation_deg,
            self.offset_hz,
            self.power_dbfs,
        ]) {
            return Some("direction, offset and power must be finite");
        }
        let paths_sound = self.paths.iter().all(|path| {
            all_finite(&[
                path.azimuth_deg,
                path.elevation_deg,
                path.delay_s,
                path.gain_db,
                path.phase_deg,
            ])
        });
        if !paths_sound {
            return Some("every path must be finite");
        }
        self.waveform.problem()
    }
}

impl LaneImpairments {
    #[must_use]
    pub fn problem(&self) -> Option<&'static str> {
        let sound = all_finite(&[
            self.ppm,
            self.gain_db,
            self.phase_deg,
            self.phase_per_db,
            self.frac_delay,
            self.dc_phase_deg,
        ]) && self.dc_dbfs.is_none_or(f64::is_finite);
        if !sound {
            return Some("lane impairments must be finite");
        }
        if self.ripple.is_some_and(|ripple| {
            !(0.0..MAX_RIPPLE_DB).contains(&ripple.depth_db) || ripple.cycles > MAX_RIPPLE_CYCLES
        }) {
            return Some("a lane ripple is 0 to 6 dB deep with at most 64 cycles");
        }
        (self.ppm.abs() > MAX_PPM).then_some("a lane clock is off by at most 1000 ppm")
    }
}

fn points(geometry: &Geometry) -> Vec<[f64; 3]> {
    geometry
        .positions()
        .iter()
        .map(|p| [p.x, p.y, p.z])
        .collect()
}

pub fn uca_positions(radius_m: f64, count: usize) -> Result<Vec<[f64; 3]>, String> {
    Geometry::uca(radius_m, count, 0.0, Winding::Clockwise)
        .map(|geometry| points(&geometry))
        .map_err(|err| err.to_string())
}

pub fn circle(radius_m: f64, count: usize) -> Vec<[f64; 3]> {
    uca_positions(radius_m, count).unwrap_or_default()
}

pub fn line(spacing_m: f64, count: usize) -> Vec<[f64; 3]> {
    Geometry::ula(spacing_m, count, 90.0)
        .map(|geometry| points(&geometry))
        .unwrap_or_default()
}

#[must_use]
pub fn default_scene() -> Scene {
    let mut positions = circle(KRAKEN_RADIUS_M, 5);
    positions.extend(circle(KRAKEN_RADIUS_M, 4));
    positions.extend(line(DONGLE_SPACING_M, 2));
    Scene {
        positions,
        emitters: vec![Emitter {
            azimuth_deg: 137.0,
            elevation_deg: 0.0,
            offset_hz: 0.0,
            power_dbfs: -30.0,
            waveform: Waveform::Fm {
                deviation_hz: 75_000.0,
                rate_hz: 1_000.0,
            },
            paths: Vec::new(),
        }],
        echoes: vec![Echo {
            emitter: 0,
            azimuth_deg: 250.0,
            delay_s: 40e-6,
            doppler_hz: 60.0,
            gain_db: -25.0,
        }],
        clutter: Clutter::default(),
        thermal_dbfs: -60.0,
        noise_source_dbfs: -20.0,
        noise_bandwidth: 2_000_000.0,
        seed: DEFAULT_SEED,
    }
}
