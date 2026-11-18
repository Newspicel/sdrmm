use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::array::MAX_ARRAY_LANES;
use crate::geo::LatLon;
use crate::processor::reserved_at;

pub const DF_POINTS: usize = 360;
pub const MAX_DF_PEAKS: u8 = 4;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DfAlgorithm {
    Bartlett,
    Capon,
    #[default]
    Music,
    RootMusic,
    Esprit,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DfPeak {
    pub relative_deg: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub true_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elevation_deg: Option<f32>,
    pub power_db: f32,
    pub confidence: f32,
    pub sigma_deg: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror_true_deg: Option<f32>,
    #[serde(default)]
    pub fit: f32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DfReading {
    pub at: String,
    pub peaks: Vec<DfPeak>,
    pub pseudospectrum: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub azimuth_deg: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<LatLon>,
    pub sources: u32,
    pub sources_auto: bool,
    pub squelched: bool,
    pub aliasing: bool,
    #[serde(default)]
    pub algorithm: DfAlgorithm,
    #[serde(default)]
    pub freq_hz: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub heading_sigma_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub likelihood: Vec<u8>,
    #[serde(default)]
    pub likelihood_true: bool,
    #[serde(default)]
    pub span_db: f32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub eigenvalues_db: Vec<f32>,
    #[serde(default)]
    pub eig_ratio_db: f32,
    #[serde(default)]
    pub lambda12_db: f32,
    #[serde(default)]
    pub snr_db: f32,
    #[serde(default)]
    pub snapshots: f32,
    #[serde(default)]
    pub fit: f32,
    #[serde(default)]
    pub spacing_ratio: f32,
    #[serde(default)]
    pub aperture_wavelengths: f32,
    #[serde(default)]
    pub mode_aliasing: bool,
    #[serde(default)]
    pub mirror: bool,
    #[serde(default)]
    pub rotating: bool,
    #[serde(default)]
    pub singular: bool,
    #[serde(default)]
    pub table_out_of_range: bool,
    #[serde(default)]
    pub gated_blocks: u32,
}

impl DfReading {
    #[must_use]
    pub fn reserved() -> Self {
        Self {
            at: reserved_at(),
            peaks: Vec::with_capacity(usize::from(MAX_DF_PEAKS)),
            pseudospectrum: Vec::with_capacity(DF_POINTS),
            likelihood: Vec::with_capacity(DF_POINTS),
            eigenvalues_db: Vec::with_capacity(MAX_ARRAY_LANES as usize),
            ..Self::default()
        }
    }
}
