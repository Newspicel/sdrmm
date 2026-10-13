use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PocsagPayload {
    Tone,
    Numeric,
    Alpha,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PocsagMessage {
    pub address: u32,
    pub function: u8,
    pub baud: u16,
    pub payload: PocsagPayload,
    pub text: String,
    pub errors_corrected: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PagerPayload {
    Tone,
    Numeric,
    Alpha,
    Binary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct FlexMessage {
    pub address: u64,
    pub payload: PagerPayload,
    pub text: String,
    pub baud: u16,
    pub levels: u8,
    pub cycle: u8,
    pub frame: u8,
    pub phase: char,
    pub errors_corrected: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ErmesMessage {
    pub local_address: u32,
    pub message_number: u8,
    pub payload: PagerPayload,
    pub text: String,
    pub urgent: bool,
    pub alert: u8,
    pub errors_corrected: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SelcallSequence {
    pub system: crate::channel::SelcallSystem,
    pub code: String,
    pub tone_ms: u32,
}
