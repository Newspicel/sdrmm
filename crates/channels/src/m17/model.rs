use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct M17Params {}

impl sdrmm_wire::DecoderSettings for M17Params {
    const TYPE_ID: &'static str = "m17";
}
