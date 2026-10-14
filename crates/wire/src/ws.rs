use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    position::PositionFix,
    presence::{Peer, Pointer},
    workspace::PatchApplyReport,
};

pub const WS_SUBPROTOCOL: &str = "sdrmm";
pub const WS_BEARER_PROTOCOL_PREFIX: &str = "sdrmm.bearer.";
pub const WS_CLOSE_REVOKED: u16 = 4003;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "scope", content = "id", rename_all = "snake_case")]
pub enum StateScope {
    All,
    Devices,
    DeviceSet(u32),
    Presets,
    Bookmarks,
    SavedRadios,
    Recordings,
    Clients,
    DecoderLog,
    Images,
    Workspaces,
    Workspace(i64),
    Arrays,
    Phones,
    Missions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StreamKind {
    Spectrum,
    Audio,
    Video,
    Iq,
    Symbols,
    RangeDoppler,
    SpatialSpectrum,
    Visibility,
    FusionGrid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct SurfaceFit {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRefusal {
    NoSurface,
    FitNotPositive,
    NoStreamIds,
}

impl SurfaceRefusal {
    pub const ALL: [Self; 3] = [Self::NoSurface, Self::FitNotPositive, Self::NoStreamIds];

    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoSurface => "No surface",
            Self::FitNotPositive => "Fit must be positive",
            Self::NoStreamIds => "Too many streams",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct StreamParams {
    #[serde(default = "default_fps")]
    pub fps: u16,
    #[serde(default = "default_bins")]
    pub bins: u16,
}

const fn default_fps() -> u16 {
    25
}

const fn default_bins() -> u16 {
    1024
}

const fn is_first(index: &u32) -> bool {
    *index == 0
}

impl Default for StreamParams {
    fn default() -> Self {
        Self {
            fps: default_fps(),
            bins: default_bins(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct PortActivity {
    pub node: String,
    pub port: String,
    pub emitted: u64,
    pub delivered: u64,
    pub unrouted: u64,
    pub ignored: u64,
    pub lost: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data")]
pub enum ServerEvent {
    BeastExportStatus(crate::event_output::BeastExportStatus),
    EventOutputStatus(crate::event_output::EventOutputStatus),
    PipelineHealth {
        queues: Vec<crate::PipelineQueue>,
        websocket: crate::QueueHealth,
    },
    DeviceReadouts {
        device_set: u32,
        readouts: Vec<crate::Readout>,
    },
    Hello {
        revision: u64,
        protocol: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        phone: Option<String>,
    },
    StateChanged {
        scope: StateScope,
    },
    SurfaceStreamStarted {
        stream_id: u16,
        node: String,
        kind: StreamKind,
    },
    SurfaceRefused {
        node: String,
        reason: SurfaceRefusal,
    },
    PortStream {
        stream_id: u16,
        node: String,
        port: String,
        source: String,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        via: Vec<String>,
        #[serde(default, skip_serializing_if = "is_first")]
        index: u32,
        kind: StreamKind,
    },
    PortRefused {
        node: String,
        port: String,
        reason: String,
    },
    StreamStopped {
        stream_id: u16,
        kind: StreamKind,
    },
    DecodedLost {
        count: u64,
    },
    ChannelLevels {
        device_set: u32,
        levels: Vec<crate::state::ChannelLevel>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        lanes: Vec<crate::state::LaneLevel>,
    },
    ScannerUpdate {
        device_set: u32,
        status: Box<crate::scan::ScannerStatus>,
    },
    HuntUpdate {
        device_set: u32,
        status: Box<crate::hunt::HuntStatus>,
    },
    ChannelBroadcast {
        node: String,
        status: Box<crate::decode::BroadcastStatus>,
    },
    SatelliteUpdate {
        status: Box<crate::satellite::SatelliteStatus>,
    },
    RotatorUpdate {
        status: Box<crate::rotator::RotatorStatus>,
    },
    PositionChanged {
        node: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fix: Option<PositionFix>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    ArrayUpdate {
        status: Box<crate::array::ArrayStatus>,
    },
    ProcessorUpdate {
        node: String,
        reading: Box<crate::processor::ProcessorReading>,
    },
    DfFusionUpdate {
        node: String,
        state: Box<crate::fusion::DfFusionState>,
    },
    SurveyUpdate {
        node: String,
        update: Box<crate::survey::SurveyUpdate>,
    },
    Peers {
        you: u32,
        peers: Vec<Peer>,
    },
    PeerPointer {
        peer: u32,
        pointer: Pointer,
    },
    WorkspaceSwitched {
        id: i64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        by: Option<String>,
        report: PatchApplyReport,
    },
    PatchResolved(Box<crate::patch::ResolvedPatch>),
    NodeProblems {
        problems: Vec<crate::patch::NodeProblem>,
    },
    NodeActivity {
        ports: Vec<PortActivity>,
    },
    SinkSnapshot {
        node: String,
        feed: Box<crate::sink::SinkFeed>,
    },
    SinkUpdate {
        node: String,
        update: Box<crate::sink::SinkUpdate>,
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "type", content = "data")]
pub enum ClientCommand {
    SubscribeDiagnostics {
        enabled: bool,
    },
    PublishPose {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fix: Option<PositionFix>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    SubscribeSurface {
        node: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fit: Option<SurfaceFit>,
    },
    UnsubscribeSurface {
        node: String,
    },
    SubscribePort {
        node: String,
        port: String,
        #[serde(default)]
        params: StreamParams,
    },
    UnsubscribePort {
        node: String,
        port: String,
    },
    SubscribeSink {
        node: String,
    },
    UnsubscribeSink {
        node: String,
    },
    Present {
        author: String,
        #[serde(default)]
        name: String,
    },
    Point(Pointer),
}

#[cfg(test)]
mod tests;
