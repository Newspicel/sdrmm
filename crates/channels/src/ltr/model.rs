use serde::{Deserialize, Serialize};

fn default_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LtrParams {
    #[serde(default = "default_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for LtrParams {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_bandwidth_hz(),
        }
    }
}

impl sdrmm_wire::DecoderSettings for LtrParams {
    const TYPE_ID: &'static str = "ltr";
}
