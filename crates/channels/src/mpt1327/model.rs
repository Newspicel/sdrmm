use serde::{Deserialize, Serialize};

fn default_mpt1327_bandwidth_hz() -> f64 {
    12_500.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mpt1327Params {
    #[serde(default = "default_mpt1327_bandwidth_hz")]
    pub bandwidth_hz: f64,
}

impl Default for Mpt1327Params {
    fn default() -> Self {
        Self {
            bandwidth_hz: default_mpt1327_bandwidth_hz(),
        }
    }
}

impl sdrmm_wire::DecoderSettings for Mpt1327Params {
    const TYPE_ID: &'static str = "mpt1327";
}
