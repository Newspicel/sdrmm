use serde::{Deserialize, Serialize};

use crate::default_true;

fn default_deemphasis_us() -> f32 {
    50.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WfmParams {
    #[serde(default = "default_deemphasis_us")]
    pub deemphasis_us: f32,
    #[serde(default = "default_true")]
    pub stereo: bool,
}

impl Default for WfmParams {
    fn default() -> Self {
        Self {
            deemphasis_us: default_deemphasis_us(),
            stereo: true,
        }
    }
}

pub const DEEMPHASIS_US: [f32; 2] = [50.0, 75.0];

impl sdrmm_wire::DecoderSettings for WfmParams {
    const TYPE_ID: &'static str = "wfm";
}
