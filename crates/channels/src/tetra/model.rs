use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TetraParams {}

impl sdrmm_wire::DecoderSettings for TetraParams {
    const TYPE_ID: &'static str = "tetra";
}
