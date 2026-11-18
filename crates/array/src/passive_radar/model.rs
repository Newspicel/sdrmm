use serde::{Deserialize, Serialize};

use crate::Settings;
use sdrmm_wire::limits::Bounds;

use crate::GpuUse;
use sdrmm_wire::array::MAX_ARRAY_LANES;

pub const RADAR_MIN_CPI_MS: u32 = 50;
pub const RADAR_MAX_CPI_MS: u32 = 2_000;
pub const RADAR_MAX_OVERLAP: f32 = 0.75;
pub const RADAR_MAX_RANGE_KM: f32 = 400.0;
pub const RADAR_MIN_SPEED_MPS: f32 = 10.0;
pub const RADAR_MAX_SPEED_MPS: f32 = 2_000.0;
pub const RADAR_MAX_GATES: u32 = 2_048;
pub const RADAR_MIN_BATCHES: u32 = 16;
pub const RADAR_MAX_BATCHES: u32 = 4_096;
pub const RADAR_MAX_OFFSET_HZ: f64 = 50_000_000.0;
pub const RADAR_MIN_BANDWIDTH_HZ: f64 = 10_000.0;
pub const RADAR_MAX_BANDWIDTH_HZ: f64 = 20_000_000.0;
pub const RADAR_MAX_WORKING_BYTES: u64 = 512 * 1024 * 1024;
pub const FM_BANDWIDTH_HZ: f64 = 200_000.0;
pub const DAB_BANDWIDTH_HZ: f64 = 1_536_000.0;
pub const DAB_SAMPLE_RATE_HZ: f64 = 2_048_000.0;
pub const DVBT_BANDWIDTH_HZ: f64 = 7_610_000.0;
pub const MAX_ECA_ORDER: u32 = 512;
pub const MAX_ECA_LEAD: u32 = 16;
pub const MAX_ECA_DOPPLER_TAPS: u32 = 2;
pub const MAX_CMA_TAPS: u32 = 64;
pub const MAX_CFAR_GUARD: u32 = 16;
pub const MAX_CFAR_TRAIN_RANGE: u32 = 64;
pub const MAX_CFAR_TRAIN_DOPPLER: u32 = 16;
pub const MAX_TRACK_WINDOW: u32 = 16;
pub const MAX_RADAR_ALTITUDE_M: f32 = 15_000.0;
pub const DEFAULT_CUSTOM_BANDWIDTH_HZ: f64 = FM_BANDWIDTH_HZ;
pub const DEFAULT_CMA_TAPS: u32 = 16;
pub const DEFAULT_CMA_STEP: f32 = 1e-3;
pub const DEFAULT_OS_RANK: f32 = 0.75;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Illuminator {
    Fm,
    Dab,
    DvbtPartial { bandwidth_hz: f64 },
    Custom { bandwidth_hz: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SurveillanceSet {
    AllOthers,
    Mask { mask: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReferenceCleaning {
    Off,
    Cma { taps: u32, step: f32 },
    DabRemod,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClutterMethod {
    #[default]
    EcaBatch,
    EcaSliding,
    Nlms,
    BlockNlms,
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClutterParams {
    pub method: ClutterMethod,
    pub reach_km: f32,
    pub lead: u32,
    pub doppler_taps: u32,
    pub batch_ms: f32,
    pub taper: bool,
    pub extension_ms: f32,
    pub step: f32,
    pub loading: f32,
}

impl Default for ClutterParams {
    fn default() -> Self {
        Self {
            method: ClutterMethod::EcaBatch,
            reach_km: 15.0,
            lead: 2,
            doppler_taps: 0,
            batch_ms: 50.0,
            taper: true,
            extension_ms: 25.0,
            step: 0.05,
            loading: 1e-4,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DopplerWindow {
    #[default]
    Hann,
    BlackmanHarris,
    Rectangular,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CfarKind {
    Ca,
    Os { rank: f32 },
    Go,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CfarWindow {
    #[default]
    Range,
    Plane,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CfarParams {
    pub kind: CfarKind,
    pub window: CfarWindow,
    pub pfa: f64,
    pub guard_range: u32,
    pub train_range: u32,
    pub guard_doppler: u32,
    pub train_doppler: u32,
    pub min_doppler_hz: f32,
    pub min_range_km: f32,
    pub min_snr_db: f32,
}

impl Default for CfarParams {
    fn default() -> Self {
        Self {
            kind: CfarKind::Ca,
            window: CfarWindow::Range,
            pfa: 1e-5,
            guard_range: 2,
            train_range: 8,
            guard_doppler: 1,
            train_doppler: 4,
            min_doppler_hz: 5.0,
            min_range_km: 1.0,
            min_snr_db: 8.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackerParams {
    pub confirm_hits: u32,
    pub confirm_window: u32,
    pub coast_looks: u32,
    pub max_accel_mps2: f32,
    pub gate: f32,
    pub jerk: f32,
}

impl Default for TrackerParams {
    fn default() -> Self {
        Self {
            confirm_hits: 3,
            confirm_window: 5,
            coast_looks: 10,
            max_accel_mps2: 30.0,
            gate: 11.8,
            jerk: 5.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PassiveRadarParams {
    pub reference_element: u32,
    pub surveillance: SurveillanceSet,
    pub illuminator: Illuminator,
    pub offset_hz: f64,
    pub cpi_ms: u32,
    pub overlap: f32,
    pub max_range_km: f32,
    pub max_speed_mps: f32,
    pub window: DopplerWindow,
    pub reference: ReferenceCleaning,
    pub clutter: ClutterParams,
    pub cfar: CfarParams,
    pub tracker: TrackerParams,
    pub aoa: bool,
    pub assumed_altitude_m: f32,
    pub gpu: GpuUse,
}

impl Default for PassiveRadarParams {
    fn default() -> Self {
        Self {
            reference_element: 0,
            surveillance: SurveillanceSet::AllOthers,
            illuminator: Illuminator::Fm,
            offset_hz: 0.0,
            cpi_ms: 500,
            overlap: 0.0,
            max_range_km: 80.0,
            max_speed_mps: 400.0,
            window: DopplerWindow::Hann,
            reference: ReferenceCleaning::Cma {
                taps: DEFAULT_CMA_TAPS,
                step: DEFAULT_CMA_STEP,
            },
            clutter: ClutterParams::default(),
            cfar: CfarParams::default(),
            tracker: TrackerParams::default(),
            aoa: true,
            assumed_altitude_m: 8_000.0,
            gpu: GpuUse::Auto,
        }
    }
}

impl Illuminator {
    #[must_use]
    pub const fn bandwidth_hz(&self) -> f64 {
        match self {
            Self::Fm => FM_BANDWIDTH_HZ,
            Self::Dab => DAB_BANDWIDTH_HZ,
            Self::DvbtPartial { bandwidth_hz } | Self::Custom { bandwidth_hz } => *bandwidth_hz,
        }
    }

    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Fm => "FM",
            Self::Dab => "DAB",
            Self::DvbtPartial { .. } => "DVB-T",
            Self::Custom { .. } => "Custom",
        }
    }
}

impl SurveillanceSet {
    const ELEMENT_BITS: u32 = MAX_ARRAY_LANES;

    pub fn elements(&self, reference: u32, elements: u32) -> impl Iterator<Item = u32> {
        let mask = match *self {
            Self::AllOthers => u32::MAX,
            Self::Mask { mask } => mask,
        };
        (0..elements.min(Self::ELEMENT_BITS))
            .filter(move |element| *element != reference && mask & (1 << element) != 0)
    }

    fn problem(self, reference: u32) -> Option<&'static str> {
        let Self::Mask { mask } = self else {
            return None;
        };
        let usable = mask & ((1 << Self::ELEMENT_BITS) - 1);
        if usable == 0 || usable != mask {
            Some("No surveillance element")
        } else if reference < Self::ELEMENT_BITS && mask & (1 << reference) != 0 {
            Some("Surveillance holds the reference")
        } else {
            None
        }
    }
}

impl ReferenceCleaning {
    fn problem(&self, illuminator: Illuminator) -> Option<&'static str> {
        match *self {
            Self::Off => None,
            Self::DabRemod => {
                (!matches!(illuminator, Illuminator::Dab)).then_some("DAB remod needs DAB")
            }
            Self::Cma { taps, step } => {
                if !matches!(illuminator, Illuminator::Fm | Illuminator::Custom { .. }) {
                    Some("CMA needs FM")
                } else if !LIMITS.cma_taps.contains(taps) {
                    Some("CMA taps out of range")
                } else if !LIMITS.cma_step.contains(step) {
                    Some("CMA step out of range")
                } else {
                    None
                }
            }
        }
    }
}

fn first_problem(checks: &[(bool, &'static str)]) -> Option<&'static str> {
    checks.iter().find(|(ok, _)| !ok).map(|(_, text)| *text)
}

impl ClutterParams {
    fn problem(&self) -> Option<&'static str> {
        first_problem(&[
            (
                LIMITS.reach_km.contains(self.reach_km),
                "Clutter reach out of range",
            ),
            (LIMITS.lead.contains(self.lead), "Lead out of range"),
            (
                LIMITS.doppler_taps.contains(self.doppler_taps),
                "Doppler taps out of range",
            ),
            (
                LIMITS.batch_ms.contains(self.batch_ms),
                "Batch out of range",
            ),
            (
                LIMITS.extension_ms.contains(self.extension_ms),
                "Extension out of range",
            ),
            (LIMITS.clutter_step.contains(self.step), "Step out of range"),
            (
                LIMITS.loading.contains(self.loading),
                "Loading out of range",
            ),
        ])
    }
}

impl CfarParams {
    fn problem(&self) -> Option<&'static str> {
        let rank_ok = match self.kind {
            CfarKind::Os { rank } => LIMITS.os_rank.contains(rank),
            CfarKind::Ca | CfarKind::Go => true,
        };
        first_problem(&[
            (rank_ok, "Rank out of range"),
            (LIMITS.pfa.contains(self.pfa), "Pfa out of range"),
            (
                LIMITS.guard.contains(self.guard_range)
                    && LIMITS.guard.contains(self.guard_doppler),
                "Guard out of range",
            ),
            (
                LIMITS.train_range.contains(self.train_range)
                    && LIMITS.train_doppler.contains(self.train_doppler),
                "Train out of range",
            ),
            (
                LIMITS.min_doppler_hz.contains(self.min_doppler_hz),
                "Min Doppler out of range",
            ),
            (
                LIMITS.min_range_km.contains(self.min_range_km),
                "Min range out of range",
            ),
            (
                LIMITS.min_snr_db.contains(self.min_snr_db),
                "Min SNR out of range",
            ),
        ])
    }
}

impl TrackerParams {
    fn problem(&self) -> Option<&'static str> {
        first_problem(&[
            (
                LIMITS.track_window.contains(self.confirm_hits)
                    && LIMITS.track_window.contains(self.confirm_window)
                    && self.confirm_hits <= self.confirm_window,
                "M of N out of range",
            ),
            (
                LIMITS.coast_looks.contains(self.coast_looks),
                "Coast out of range",
            ),
            (
                LIMITS.max_accel_mps2.contains(self.max_accel_mps2),
                "Accel out of range",
            ),
            (LIMITS.gate.contains(self.gate), "Gate out of range"),
            (LIMITS.jerk.contains(self.jerk), "Jerk out of range"),
        ])
    }
}

impl PassiveRadarParams {
    fn frame_problem(&self) -> Option<&'static str> {
        first_problem(&[
            (LIMITS.cpi_ms.contains(self.cpi_ms), "CPI out of range"),
            (
                LIMITS.overlap.contains(self.overlap),
                "Overlap out of range",
            ),
            (
                LIMITS.max_range_km.contains(self.max_range_km),
                "Range out of range",
            ),
            (
                LIMITS.max_speed_mps.contains(self.max_speed_mps),
                "Speed out of range",
            ),
            (
                LIMITS.offset_hz.contains(self.offset_hz),
                "Offset out of range",
            ),
            (
                LIMITS
                    .bandwidth_hz
                    .contains(self.illuminator.bandwidth_hz()),
                "Bandwidth out of range",
            ),
            (
                self.reference_element < MAX_ARRAY_LANES,
                "Reference out of range",
            ),
        ])
    }
}

impl Settings for PassiveRadarParams {
    const TYPE_ID: &'static str = "passive_radar";

    fn problem(&self) -> Option<&'static str> {
        self.frame_problem()
            .or_else(|| self.surveillance.problem(self.reference_element))
            .or_else(|| self.reference.problem(self.illuminator))
            .or_else(|| self.clutter.problem())
            .or_else(|| self.cfar.problem())
            .or_else(|| self.tracker.problem())
            .or_else(|| {
                (!LIMITS.altitude_m.contains(self.assumed_altitude_m))
                    .then_some("Altitude out of range")
            })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RadarLimits {
    pub offset_hz: Bounds<f64>,
    pub bandwidth_hz: Bounds<f64>,
    pub max_range_km: Bounds<f32>,
    pub max_speed_mps: Bounds<f32>,
    pub cpi_ms: Bounds<u32>,
    pub overlap: Bounds<f32>,
    pub reach_km: Bounds<f32>,
    pub lead: Bounds<u32>,
    pub doppler_taps: Bounds<u32>,
    pub batch_ms: Bounds<f32>,
    pub extension_ms: Bounds<f32>,
    pub clutter_step: Bounds<f32>,
    pub loading: Bounds<f32>,
    pub cma_taps: Bounds<u32>,
    pub cma_step: Bounds<f32>,
    pub os_rank: Bounds<f32>,
    pub pfa: Bounds<f64>,
    pub guard: Bounds<u32>,
    pub train_range: Bounds<u32>,
    pub train_doppler: Bounds<u32>,
    pub min_doppler_hz: Bounds<f32>,
    pub min_range_km: Bounds<f32>,
    pub min_snr_db: Bounds<f32>,
    pub track_window: Bounds<u32>,
    pub coast_looks: Bounds<u32>,
    pub max_accel_mps2: Bounds<f32>,
    pub gate: Bounds<f32>,
    pub jerk: Bounds<f32>,
    pub altitude_m: Bounds<f32>,
    pub seed: RadarSeeds,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct RadarSeeds {
    pub dvbt_bandwidth_hz: f64,
    pub custom_bandwidth_hz: f64,
    pub cma_taps: u32,
    pub cma_step: f32,
    pub os_rank: f32,
}

pub const LIMITS: RadarLimits = RadarLimits {
    offset_hz: Bounds::new(-RADAR_MAX_OFFSET_HZ, RADAR_MAX_OFFSET_HZ),
    bandwidth_hz: Bounds::new(RADAR_MIN_BANDWIDTH_HZ, RADAR_MAX_BANDWIDTH_HZ),
    max_range_km: Bounds::above(0.0, RADAR_MAX_RANGE_KM),
    max_speed_mps: Bounds::new(RADAR_MIN_SPEED_MPS, RADAR_MAX_SPEED_MPS),
    cpi_ms: Bounds::new(RADAR_MIN_CPI_MS, RADAR_MAX_CPI_MS),
    overlap: Bounds::new(0.0, RADAR_MAX_OVERLAP),
    reach_km: Bounds::new(0.1, 100.0),
    lead: Bounds::new(0, MAX_ECA_LEAD),
    doppler_taps: Bounds::new(0, MAX_ECA_DOPPLER_TAPS),
    batch_ms: Bounds::new(1.0, 1_000.0),
    extension_ms: Bounds::new(0.0, 500.0),
    clutter_step: Bounds::above(0.0, 1.0),
    loading: Bounds::new(0.0, 1.0),
    cma_taps: Bounds::new(1, MAX_CMA_TAPS),
    cma_step: Bounds::above(0.0, 0.1),
    os_rank: Bounds::new(0.5, 0.95),
    pfa: Bounds::new(1e-9, 1e-2),
    guard: Bounds::new(0, MAX_CFAR_GUARD),
    train_range: Bounds::new(1, MAX_CFAR_TRAIN_RANGE),
    train_doppler: Bounds::new(1, MAX_CFAR_TRAIN_DOPPLER),
    min_doppler_hz: Bounds::new(0.0, 100.0),
    min_range_km: Bounds::new(0.0, 50.0),
    min_snr_db: Bounds::new(0.0, 40.0),
    track_window: Bounds::new(1, MAX_TRACK_WINDOW),
    coast_looks: Bounds::new(0, 100),
    max_accel_mps2: Bounds::new(1.0, 200.0),
    gate: Bounds::new(4.0, 30.0),
    jerk: Bounds::above(0.0, 1_000.0),
    altitude_m: Bounds::new(0.0, MAX_RADAR_ALTITUDE_M),
    seed: RadarSeeds {
        dvbt_bandwidth_hz: DVBT_BANDWIDTH_HZ,
        custom_bandwidth_hz: DEFAULT_CUSTOM_BANDWIDTH_HZ,
        cma_taps: DEFAULT_CMA_TAPS,
        cma_step: DEFAULT_CMA_STEP,
        os_rank: DEFAULT_OS_RANK,
    },
};

#[cfg(test)]
mod tests;
