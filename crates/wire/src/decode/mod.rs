use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

mod aviation;
mod broadcast;
mod datalink;
mod dect;
#[cfg(test)]
pub(crate) mod fixtures;
mod ident;
mod maritime;
mod navaid;
mod pager;
mod picture;
mod registry;
mod text;
mod voice;
mod wsjt;

pub use aviation::*;
pub use broadcast::*;
pub use datalink::*;
pub use dect::*;
pub use ident::*;
pub use maritime::*;
pub use navaid::*;
pub use pager::*;
pub use picture::*;
pub use registry::*;
pub use text::*;
pub use voice::*;
pub use wsjt::*;

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

#[cfg(test)]
mod tests;
