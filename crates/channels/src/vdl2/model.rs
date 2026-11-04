use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vdl2Params {}

impl sdrmm_wire::DecoderSettings for Vdl2Params {
    const TYPE_ID: &'static str = "vdl2";
}
