use serde::{Deserialize, Serialize};

use sdrmm_wire::labelled;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AeroChannel {
    #[default]
    P,
    Burst,
    C,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InmarsatAeroParams {
    #[serde(default)]
    pub channel: AeroChannel,
}

labelled! {
    AeroChannel {
        P => "P": "Forward channel to aircraft",
        Burst => "R/T": "Bursts from aircraft",
        C => "C": "Voice circuit",
    }
}

impl sdrmm_wire::DecoderSettings for InmarsatAeroParams {
    const TYPE_ID: &'static str = "inmarsat_aero";
}
