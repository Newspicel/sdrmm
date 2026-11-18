use serde::{Deserialize, Serialize};

use crate::Settings;
use crate::limits::NARROW_BAND;
use sdrmm_wire::limits::{BandLimits, Bounds};

use sdrmm_wire::array::MAX_ARRAY_LANES;
use sdrmm_wire::processor::reserved_at;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hand {
    Right,
    Left,
    #[default]
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PolarimeterParams {
    pub h_lane: u32,
    pub v_lane: u32,
    pub offset_hz: f64,
    pub bandwidth_hz: f64,
    pub matched: bool,
    pub report_ms: u32,
    pub average_ms: u32,
    pub crossfade_ms: u32,
    pub flip_hand: bool,
}

impl Default for PolarimeterParams {
    fn default() -> Self {
        Self {
            h_lane: 0,
            v_lane: 1,
            offset_hz: 0.0,
            bandwidth_hz: 20_000.0,
            matched: true,
            report_ms: 250,
            average_ms: 500,
            crossfade_ms: 20,
            flip_hand: false,
        }
    }
}

impl Settings for PolarimeterParams {
    const TYPE_ID: &'static str = "polarimeter";

    fn problem(&self) -> Option<&'static str> {
        let checks = [
            (self.h_lane < MAX_ARRAY_LANES, "H out of range"),
            (self.v_lane < MAX_ARRAY_LANES, "V out of range"),
            (self.h_lane != self.v_lane, "Lanes must differ"),
            (
                LIMITS.band.offset_hz.contains(self.offset_hz),
                "Offset out of range",
            ),
            (
                LIMITS.band.bandwidth_hz.contains(self.bandwidth_hz),
                "Bandwidth out of range",
            ),
            (
                LIMITS.report_ms.contains(self.report_ms),
                "Report out of range",
            ),
            (
                LIMITS.average_ms.contains(self.average_ms),
                "Average out of range",
            ),
            (
                LIMITS.crossfade_ms.contains(self.crossfade_ms),
                "Crossfade out of range",
            ),
        ];
        checks.iter().find(|(ok, _)| !ok).map(|(_, text)| *text)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PolarimeterReading {
    pub at: String,
    pub i_db: f32,
    pub q: f32,
    pub u: f32,
    pub v: f32,
    pub degree: f32,
    pub angle_deg: f32,
    pub ellipticity_deg: f32,
    #[serde(default)]
    pub hand: Hand,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snr_db: Option<f32>,
    #[serde(default)]
    pub out_center_hz: f64,
    #[serde(default)]
    pub out_rate: f64,
}

impl PolarimeterReading {
    #[must_use]
    pub fn reserved() -> Self {
        Self {
            at: reserved_at(),
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct PolarimeterLimits {
    pub band: BandLimits,
    pub report_ms: Bounds<u32>,
    pub average_ms: Bounds<u32>,
    pub crossfade_ms: Bounds<u32>,
}

pub const LIMITS: PolarimeterLimits = PolarimeterLimits {
    band: NARROW_BAND,
    report_ms: Bounds::new(50, 10_000),
    average_ms: Bounds::new(50, 10_000),
    crossfade_ms: Bounds::new(0, 500),
};
