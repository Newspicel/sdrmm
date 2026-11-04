use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DrmMode {
    #[default]
    Auto,
    Drm30,
    DrmPlus,
}

pub(crate) fn default_drm_bandwidth_hz() -> f64 {
    100_000.0
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct DrmParams {
    #[serde(default)]
    pub mode: DrmMode,
    #[serde(default = "default_drm_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<u8>,
}

impl Default for DrmParams {
    fn default() -> Self {
        Self {
            mode: DrmMode::default(),
            bandwidth_hz: default_drm_bandwidth_hz(),
            service: None,
        }
    }
}

labelled! {
    DrmMode { Auto => "Auto", Drm30 => "DRM30", DrmPlus => "DRM+" }
}

impl sdrmm_wire::DecoderSettings for DrmParams {
    const TYPE_ID: &'static str = "drm";
}
