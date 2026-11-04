use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::{EventContract, EventKind};

/// A file or packet a broadcast data service carried, as every broadcast decoder hands it on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct BroadcastData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protocol: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub label: Vec<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_id: Option<u32>,
    pub name: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl EventContract for BroadcastData {
    const KIND: EventKind = EventKind::BROADCAST_DATA;
}
