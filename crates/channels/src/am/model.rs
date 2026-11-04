use sdrmm_wire::DecoderSettings;
use serde::{Deserialize, Serialize};

fn default_am_bandwidth_hz() -> f64 {
    10_000.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AmParams {
    #[serde(default = "default_am_bandwidth_hz")]
    pub bandwidth_hz: f64,
    #[serde(default)]
    pub sync: bool,
}

impl Default for AmParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_am_bandwidth_hz(),
            sync: false,
        }
    }
}

impl DecoderSettings for AmParams {
    const TYPE_ID: &'static str = "am";
}
