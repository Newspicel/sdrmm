use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::PskBaud;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct RttyText {
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct MorseText {
    pub text: String,
    pub wpm: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct CwSkimmerSpot {
    pub offset_hz: f32,
    pub text: String,
    pub wpm: f32,
    pub snr_db: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PskText {
    pub baud: PskBaud,
    pub text: String,
}
