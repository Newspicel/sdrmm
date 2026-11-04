use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HfdlParams {}

impl sdrmm_wire::DecoderSettings for HfdlParams {
    const TYPE_ID: &'static str = "hfdl";
}
