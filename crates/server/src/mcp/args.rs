use sdrmm_wire::{
    ChannelDescriptor, ChannelInfo, ChannelSettings, DeviceInfo, DeviceRef, DeviceSet,
    DeviceSettings, NodeBody, PatchApplyReport, PatchNode, PortRef, Position, ScanAction,
    ScannerStatus, WorkspaceDetail,
};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct NodeArgs {
    pub node: String,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct WorkspaceArgs {
    pub workspace: i64,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct PutNodeArgs {
    #[serde(default)]
    pub node: Option<String>,
    #[serde(default)]
    pub body: Option<NodeBody>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub position: Option<Position>,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct WireArgs {
    pub from: PortRef,
    pub to: PortRef,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct TuneArgs {
    pub node: String,
    pub settings: DeviceSettings,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct ChannelArgs {
    pub node: String,
    pub settings: ChannelSettings,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct ScanArgs {
    pub node: String,
    pub action: ScanAction,
}

#[derive(Debug, Deserialize, ToSchema)]
pub(super) struct SpectrumArgs {
    pub node: String,
    #[serde(default)]
    pub stream: u32,
}

#[derive(Debug, Serialize)]
pub(super) struct Applied {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    pub report: PatchApplyReport,
}

#[derive(Debug, Serialize)]
pub(super) struct WorkspaceView {
    #[serde(flatten)]
    pub detail: WorkspaceDetail,
    pub running: Vec<String>,
}

#[derive(Debug, Serialize)]
pub(super) struct NodeView {
    pub node: PatchNode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub radio: Option<DeviceSet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoder: Option<ChannelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved: Option<ChannelSettings>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scan: Option<ScannerStatus>,
}

#[derive(Debug, Serialize)]
pub(super) struct DeviceChoice {
    pub device: DeviceRef,
    pub info: DeviceInfo,
}

#[derive(Debug, Serialize)]
pub(super) struct Devices {
    pub devices: Vec<DeviceChoice>,
}

#[derive(Debug, Serialize)]
pub(super) struct ChannelTypes {
    pub types: Vec<ChannelDescriptor>,
}

#[derive(Debug, Serialize)]
pub(super) struct ChannelSet {
    pub live: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct Spectrum {
    pub center_hz: f64,
    pub span_hz: f32,
    pub floor_db: Option<f32>,
    pub bins_db: Vec<f32>,
}
