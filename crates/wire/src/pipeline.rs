use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct QueueHealth {
    pub queued: u64,
    pub capacity: u64,
    pub oldest_ms: f64,
    pub dropped: u64,
}

/// How the network path to one client fares, where the transport can tell.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct LinkHealth {
    pub rtt_ms: f32,
    /// Share of the packets sent since the last report that were lost, from 0 to 1.
    pub loss: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PipelineQueue {
    pub device_set: u32,
    pub stream: u32,
    pub channel: Option<u32>,
    pub stage: PipelineStage,
    pub health: QueueHealth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStage {
    Capture,
    Spectrum,
    Channel,
}
