use serde::{Deserialize, Serialize};

use crate::Settings;
use crate::limits::WIDE_BAND;
use sdrmm_wire::limits::{BandLimits, Bounds};

use crate::limits::{band_holds, power_of_two_in};
use sdrmm_wire::processor::reserved_at;

pub const MAX_SPATIAL_PEAKS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialMethod {
    #[default]
    Bartlett,
    Capon,
    Music,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SpatialSpectrumParams {
    pub bins: u32,
    pub columns: u32,
    pub average_ms: u32,
    pub report_ms: u32,
    pub azimuth_step_deg: f64,
    pub method: SpatialMethod,
    pub span_db: f32,
    pub offset_hz: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bandwidth_hz: Option<f64>,
}

impl Default for SpatialSpectrumParams {
    fn default() -> Self {
        Self {
            bins: 1024,
            columns: 256,
            average_ms: 300,
            report_ms: 100,
            azimuth_step_deg: 2.0,
            method: SpatialMethod::Bartlett,
            span_db: 30.0,
            offset_hz: 0.0,
            bandwidth_hz: None,
        }
    }
}

fn divides_circle(step_deg: f64) -> bool {
    let count = 360.0 / step_deg;
    LIMITS.azimuth_step_deg.contains(step_deg) && (count - count.round()).abs() < 1e-9
}

impl Settings for SpatialSpectrumParams {
    const TYPE_ID: &'static str = "spatial_spectrum";

    fn problem(&self) -> Option<&'static str> {
        let checks = [
            (power_of_two_in(self.bins, LIMITS.bins), "Bins out of range"),
            (
                power_of_two_in(self.columns, LIMITS.columns) && self.columns <= self.bins,
                "Columns out of range",
            ),
            (
                LIMITS.average_ms.contains(self.average_ms),
                "Average out of range",
            ),
            (
                LIMITS.report_ms.contains(self.report_ms),
                "Report out of range",
            ),
            (divides_circle(self.azimuth_step_deg), "Step out of range"),
            (LIMITS.span_db.contains(self.span_db), "Span out of range"),
            (
                LIMITS.band.offset_hz.contains(self.offset_hz),
                "Offset out of range",
            ),
            (
                band_holds(LIMITS.band, self.bandwidth_hz),
                "Bandwidth out of range",
            ),
        ];
        checks.iter().find(|(ok, _)| !ok).map(|(_, text)| *text)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpatialPeak {
    pub freq_hz: f64,
    pub bearing_deg: f32,
    pub db: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub true_deg: Option<f32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpatialReading {
    pub at: String,
    pub peaks: Vec<SpatialPeak>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azimuth_deg: Option<f64>,
    #[serde(default)]
    pub frames: u64,
    #[serde(default)]
    pub dropped_frames: u64,
}

impl SpatialReading {
    #[must_use]
    pub fn reserved() -> Self {
        Self {
            at: reserved_at(),
            peaks: Vec::with_capacity(MAX_SPATIAL_PEAKS),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct SpatialLimits {
    pub bins: Bounds<u32>,
    pub columns: Bounds<u32>,
    pub average_ms: Bounds<u32>,
    pub report_ms: Bounds<u32>,
    pub azimuth_step_deg: Bounds<f64>,
    pub span_db: Bounds<f32>,
    pub band: BandLimits,
}

pub const LIMITS: SpatialLimits = SpatialLimits {
    bins: Bounds::new(256, 4_096),
    columns: Bounds::new(64, 1_024),
    average_ms: Bounds::new(50, 10_000),
    report_ms: Bounds::new(50, 2_000),
    azimuth_step_deg: Bounds::new(1.0, 10.0),
    span_db: Bounds::new(10.0, 80.0),
    band: WIDE_BAND,
};
