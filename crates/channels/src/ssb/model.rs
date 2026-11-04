use serde::{Deserialize, Serialize};

use sdrmm_wire::Sideband;

fn default_ssb_bandwidth_hz() -> f64 {
    2_700.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SsbParams {
    #[serde(default)]
    pub sideband: Sideband,
    #[serde(default = "default_ssb_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for SsbParams {
    fn default() -> Self {
        Self {
            sideband: Sideband::default(),
            bandwidth_hz: default_ssb_bandwidth_hz(),
        }
    }
}

impl sdrmm_wire::DecoderSettings for SsbParams {
    const TYPE_ID: &'static str = "ssb";
}
