use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

mod broadcast;
mod ident;
mod platform;
mod registry;
mod signature;
#[cfg(test)]
pub(crate) mod test_kinds;
mod voice;

pub use broadcast::*;
pub use ident::*;
pub use platform::*;
pub use registry::*;
pub use signature::*;
pub use voice::*;

pub const NO_CHANNEL: u32 = u32::MAX;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct DecodedRecord {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<crate::EventOrigin>,
    pub device_set: u32,
    pub channel: u32,
    pub at: String,
    pub freq_hz: f64,
    pub event: DecoderEvent,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sinks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<crate::present::Presentation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub callsign: Option<crate::Callsign>,
}
