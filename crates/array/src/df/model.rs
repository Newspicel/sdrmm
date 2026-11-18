use serde::{Deserialize, Serialize};

use crate::Settings;
use crate::limits::NARROW_BAND;
use sdrmm_wire::limits::{BandLimits, Bounds};

use sdrmm_wire::DfAlgorithm;
use sdrmm_wire::processor::df::MAX_DF_PEAKS;

pub const MAX_DF_SOURCES: u32 = 15;
pub const MIN_DF_REPORT_MS: u32 = 100;
pub const MAX_DF_REPORT_MS: u32 = 10_000;
pub const MIN_DF_BANDWIDTH_HZ: f64 = 100.0;
pub const MAX_DF_BANDWIDTH_HZ: f64 = 20_000_000.0;
pub const MAX_DF_OFFSET_HZ: f64 = 100_000_000.0;
pub const MAX_STATION_ID_LEN: usize = 64;
pub const MAX_DF_SMOOTHING: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRule {
    #[default]
    Dominance,
    Mdl,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UlaSide {
    #[default]
    Both,
    Front,
    Back,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DfParams {
    pub algorithm: DfAlgorithm,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sources: Option<u32>,
    pub source_rule: SourceRule,
    pub max_peaks: u8,
    pub smoothing: u8,
    pub forward_backward: bool,
    pub elevation: bool,
    pub ula_side: UlaSide,
    pub offset_hz: f64,
    pub bandwidth_hz: f64,
    pub report_ms: u32,
    pub carry_over: f32,
    pub squelch_db: f32,
    pub loading: f32,
    pub azimuth_step_deg: f64,
    pub yaw_gate_dps: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station_id: Option<String>,
}

impl Default for DfParams {
    fn default() -> Self {
        Self {
            algorithm: DfAlgorithm::Music,
            sources: None,
            source_rule: SourceRule::Dominance,
            max_peaks: 2,
            smoothing: 0,
            forward_backward: false,
            elevation: false,
            ula_side: UlaSide::Both,
            offset_hz: 0.0,
            bandwidth_hz: 20_000.0,
            report_ms: 500,
            carry_over: 0.5,
            squelch_db: 6.0,
            loading: 0.001,
            azimuth_step_deg: 1.0,
            yaw_gate_dps: 20.0,
            station_id: None,
        }
    }
}

impl Settings for DfParams {
    const TYPE_ID: &'static str = "df";

    fn problem(&self) -> Option<&'static str> {
        let checks = [
            (
                self.sources.is_none_or(|n| LIMITS.sources.contains(n)),
                "Sources out of range",
            ),
            (LIMITS.peaks.contains(self.max_peaks), "Peaks out of range"),
            (
                LIMITS.smoothing.contains(self.smoothing),
                "Smoothing out of range",
            ),
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
                LIMITS.carry_over.contains(self.carry_over),
                "Carry over out of range",
            ),
            (
                LIMITS.squelch_db.contains(self.squelch_db),
                "Squelch out of range",
            ),
            (
                LIMITS.loading.contains(self.loading),
                "Loading out of range",
            ),
            (
                LIMITS.azimuth_step_deg.contains(self.azimuth_step_deg),
                "Step out of range",
            ),
            (
                LIMITS.yaw_gate_dps.contains(self.yaw_gate_dps),
                "Yaw gate out of range",
            ),
            (
                self.station_id
                    .as_deref()
                    .is_none_or(|id| !id.is_empty() && id.len() <= LIMITS.station_len),
                "Station out of range",
            ),
        ];
        checks.iter().find(|(ok, _)| !ok).map(|(_, text)| *text)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct DfLimits {
    pub sources: Bounds<u32>,
    pub peaks: Bounds<u8>,
    pub smoothing: Bounds<u8>,
    pub band: BandLimits,
    pub report_ms: Bounds<u32>,
    pub carry_over: Bounds<f32>,
    pub squelch_db: Bounds<f32>,
    pub loading: Bounds<f32>,
    pub azimuth_step_deg: Bounds<f64>,
    pub yaw_gate_dps: Bounds<f32>,
    pub station_len: usize,
}

pub const LIMITS: DfLimits = DfLimits {
    sources: Bounds::new(1, MAX_DF_SOURCES),
    peaks: Bounds::new(1, MAX_DF_PEAKS),
    smoothing: Bounds::new(0, MAX_DF_SMOOTHING),
    band: NARROW_BAND,
    report_ms: Bounds::new(MIN_DF_REPORT_MS, MAX_DF_REPORT_MS),
    carry_over: Bounds::new(0.0, 0.99),
    squelch_db: Bounds::new(0.0, 40.0),
    loading: Bounds::new(0.0, 1.0),
    azimuth_step_deg: Bounds::new(0.25, 10.0),
    yaw_gate_dps: Bounds::new(1.0, 360.0),
    station_len: MAX_STATION_ID_LEN,
};
