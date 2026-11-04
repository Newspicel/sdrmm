use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DscParams {}

impl sdrmm_wire::DecoderSettings for DscParams {
    const TYPE_ID: &'static str = "dsc";
}
