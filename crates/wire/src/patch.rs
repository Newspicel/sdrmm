use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use crate::{
    EventOutputNode, GpsNode, MAX_NMEA_BAUD, MAX_NMEA_UPDATE_INTERVAL_MS,
    MAX_POSITION_ENDPOINT_LEN, MIN_NMEA_BAUD, MIN_NMEA_UPDATE_INTERVAL_MS, PositionSource,
    channel::{ChannelDescriptor, ChannelParams},
    device::{DeviceInfo, RECORDING_DRIVER_ID, SIGGEN_DRIVER_ID, recording_stem_valid},
    fusion::TriangulationParams,
    hunt::HuntSweepParams,
    workspace::MAX_NAME_LEN,
};

mod contract;
mod ports;
mod registry;
pub mod resolve;

pub use contract::*;
pub use ports::*;
pub use registry::*;
pub use resolve::{
    ChannelAddr, Feed, NodeProblem, ProblemSource, Reach, RefusedEdge, ResolveCtx, ResolvedEdge,
    ResolvedNode, ResolvedPatch, ResolvedPort, ResolvedState, Runs, Tap,
};

pub const MAX_NODES: usize = 128;
pub const MAX_EDGES: usize = 256;
pub const MAX_NODE_ID_LEN: usize = 64;
pub const MAX_COORD: f32 = 100_000.0;
pub const MAX_NODE_SIZE: f32 = 10_000.0;
pub const RACK_COLS: u16 = 12;
pub const RACK_ROWS: u16 = 8;
pub const MAX_STREAMS: u32 = 16;

#[must_use]
pub fn stream_port(base: &str, index: u32) -> String {
    if index == 0 {
        base.to_owned()
    } else {
        format!("{base}{}", index + 1)
    }
}

#[must_use]
pub fn port_stream(base: &str, name: &str) -> Option<u32> {
    if name == base {
        return Some(0);
    }
    let suffix = name.strip_prefix(base)?;
    let n: u32 = suffix.parse().ok()?;
    if !(2..=MAX_STREAMS).contains(&n) || suffix != n.to_string() {
        return None;
    }
    Some(n - 1)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum NodeCategory {
    Source,
    Channel,
    Tool,
    Output,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeviceRef {
    pub backend: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

impl DeviceRef {
    #[must_use]
    pub fn from_info(info: &DeviceInfo) -> Self {
        let variant = info.serial.as_ref().is_some_and(|serial| {
            info.key
                .strip_prefix(serial)
                .is_some_and(|suffix| suffix.starts_with('@'))
        });
        Self {
            backend: info.driver.clone(),
            serial: info.serial.clone(),
            key: (info.serial.is_none() || variant).then(|| info.key.clone()),
        }
    }

    #[must_use]
    pub fn matches(&self, info: &DeviceInfo) -> bool {
        if self.backend != info.driver {
            return false;
        }
        match (&self.serial, &info.serial) {
            (Some(want), Some(have)) => {
                want == have && self.key.as_ref().is_none_or(|key| *key == info.key)
            }
            (Some(_), None) => false,
            (None, _) => match &self.key {
                Some(key) => *key == info.key,
                None => true,
            },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DeviceNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<DeviceRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locked_streams: Vec<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub split_tuning: bool,
}

impl DeviceNode {
    #[must_use]
    pub fn tuning_locked(&self, stream: u32) -> bool {
        self.locked_streams.contains(&stream)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RecordingNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<String>,
}

impl RecordingNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.recording.as_deref().is_none_or(recording_stem_valid)
    }

    #[must_use]
    pub fn device_ref(&self) -> Option<DeviceRef> {
        let stem = self
            .recording
            .as_deref()
            .filter(|stem| recording_stem_valid(stem))?;
        Some(DeviceRef {
            backend: RECORDING_DRIVER_ID.to_owned(),
            serial: None,
            key: Some(stem.to_owned()),
        })
    }
}

#[must_use]
pub fn siggen_key(node_id: &str) -> String {
    node_id.replace(':', "-")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct SignalGenNode {
    pub running: bool,
}

impl Default for SignalGenNode {
    fn default() -> Self {
        Self { running: true }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct ChannelNode {
    pub channel_type: String,
    #[serde(default)]
    pub tuning_locked: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DmrTrunkProtocol {
    #[default]
    Auto,
    CapacityPlus,
    HyteraXpt,
    TierThree,
}

pub const MAX_DMR_SEARCH_RANGES: usize = 8;
pub const MAX_DMR_SEARCH_CANDIDATES: usize = 512;
pub const MIN_DMR_SEARCH_STEP_HZ: u64 = 1_250;
pub const MAX_DMR_CHANNEL_MAP: usize = 512;
pub const MAX_DMR_PROBES: u8 = 8;
pub const DEFAULT_DMR_PROBES: u8 = 4;
pub const MAX_DMR_LOGICAL_CHANNEL: u16 = 4095;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DmrSearchRange {
    pub start_hz: u64,
    pub end_hz: u64,
    pub step_hz: u64,
}

impl DmrSearchRange {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.step_hz >= MIN_DMR_SEARCH_STEP_HZ
            && self.start_hz > 0
            && self.end_hz >= self.start_hz
            && self.candidates() <= MAX_DMR_SEARCH_CANDIDATES
    }

    #[must_use]
    pub fn candidates(&self) -> usize {
        if self.step_hz == 0 || self.end_hz < self.start_hz {
            return 0;
        }
        ((self.end_hz - self.start_hz) / self.step_hz) as usize + 1
    }

    pub fn frequencies(&self) -> impl Iterator<Item = u64> + '_ {
        (0..self.candidates() as u64).map(|step| self.start_hz + step * self.step_hz)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DmrChannelEntry {
    pub lcn: u16,
    pub freq_hz: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DmrDiscovery {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ranges: Vec<DmrSearchRange>,
    #[serde(default)]
    pub max_probes: u8,
}

impl DmrDiscovery {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.ranges.len() <= MAX_DMR_SEARCH_RANGES
            && self.ranges.iter().all(DmrSearchRange::valid)
            && self.candidates() <= MAX_DMR_SEARCH_CANDIDATES
            && self.max_probes <= MAX_DMR_PROBES
    }

    #[must_use]
    pub fn candidates(&self) -> usize {
        self.ranges.iter().map(DmrSearchRange::candidates).sum()
    }

    #[must_use]
    pub fn probes(&self) -> u8 {
        if self.max_probes == 0 {
            DEFAULT_DMR_PROBES
        } else {
            self.max_probes.min(MAX_DMR_PROBES)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DmrTrunkNode {
    #[serde(default)]
    pub protocol: DmrTrunkProtocol,
    #[serde(default)]
    pub discovery: DmrDiscovery,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channel_map: Vec<DmrChannelEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control_hz: Option<u64>,
    #[serde(default)]
    pub ignore_crc: bool,
}

impl DmrTrunkNode {
    #[must_use]
    pub fn valid(&self) -> bool {
        self.discovery.valid()
            && self.channel_map.len() <= MAX_DMR_CHANNEL_MAP
            && self
                .channel_map
                .iter()
                .all(|entry| entry.lcn <= MAX_DMR_LOGICAL_CHANNEL && entry.freq_hz > 0)
            && self.control_hz.is_none_or(|hz| hz > 0)
    }
}

pub const DEFAULT_SIGNAL_MAP_OFFSET_HZ: i64 = 0;
pub const DEFAULT_SIGNAL_MAP_BANDWIDTH_HZ: u64 = 12_500;
pub const MAX_SIGNAL_MAP_OFFSET_HZ: i64 = 1_000_000_000_000;
pub const MAX_SIGNAL_MAP_BANDWIDTH_HZ: u64 = 100_000_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct RecorderNode {
    pub recording: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct AudioRecorderNode {
    pub recording: bool,
    pub skip_silence: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct SignalMapNode {
    pub offset_hz: i64,
    pub bandwidth_hz: u64,
}

impl Default for SignalMapNode {
    fn default() -> Self {
        Self {
            offset_hz: DEFAULT_SIGNAL_MAP_OFFSET_HZ,
            bandwidth_hz: DEFAULT_SIGNAL_MAP_BANDWIDTH_HZ,
        }
    }
}

impl Default for DmrTrunkNode {
    fn default() -> Self {
        Self {
            protocol: DmrTrunkProtocol::Auto,
            discovery: DmrDiscovery::default(),
            channel_map: Vec::new(),
            control_hz: None,
            ignore_crc: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct HuntNode {
    #[serde(default = "default_clicks")]
    pub clicks: bool,
    #[serde(default)]
    pub sweep: HuntSweepParams,
}

const fn default_clicks() -> bool {
    true
}

impl Default for HuntNode {
    fn default() -> Self {
        Self {
            clicks: default_clicks(),
            sweep: HuntSweepParams::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct DecoderLogNode {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<crate::LogGroupKey>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ScannerNode {
    #[serde(default)]
    pub settings: crate::ScanSettings,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct TriangulationNode {
    #[serde(default)]
    pub settings: TriangulationParams,
}

impl NodeBody {
    #[must_use]
    pub fn device_ref(&self, node_id: &str) -> Option<DeviceRef> {
        match self {
            Self::Device(device) => device.device.clone(),
            Self::Recording(recording) => recording.device_ref(),
            Self::SignalGen(generator) => generator.running.then(|| DeviceRef {
                backend: SIGGEN_DRIVER_ID.to_owned(),
                serial: None,
                key: Some(siggen_key(node_id)),
            }),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Position {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PatchNode {
    pub id: String,
    #[serde(flatten)]
    pub body: NodeBody,
    pub position: Position,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<Size>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct PortRef {
    pub node: String,
    pub port: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ToSchema)]
pub struct PatchEdge {
    pub from: PortRef,
    pub to: PortRef,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct PatchGraph {
    pub nodes: Vec<PatchNode>,
    #[serde(default)]
    pub edges: Vec<PatchEdge>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RackCell {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RackSlot {
    pub node: String,
    #[serde(flatten)]
    pub cell: RackCell,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct RackLayout {
    #[serde(default)]
    pub slots: Vec<RackSlot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PatchError {
    TooManyNodes,
    TooManyEdges,
    NodeId(String),
    DuplicateNode(String),
    Label(String),
    Geometry(String),
    Backend(String),
    Gps(String),
    ChannelType(String),
    NodeSettings(String),
    UnknownNode(String),
    UnknownPort(PortRef),
    Direction(PortRef),
    TypeMismatch {
        from: PortType,
        to: PortType,
    },
    DuplicateEdge(PortRef),
    SelfEdge(String),
    Cycle(String),
    RackCell(String),
    DuplicateRackSlot(String),
    RackOverlap(String),
    Refused {
        edge: Box<PatchEdge>,
        refusal: Refusal,
    },
}

impl std::fmt::Display for PatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooManyNodes => write!(f, "too many nodes (max {MAX_NODES})"),
            Self::TooManyEdges => write!(f, "too many edges (max {MAX_EDGES})"),
            Self::NodeId(id) => write!(f, "invalid node id {id:?}"),
            Self::DuplicateNode(id) => write!(f, "duplicate node id {id}"),
            Self::Label(id) => write!(f, "label of node {id} is longer than {MAX_NAME_LEN}"),
            Self::Geometry(id) => write!(f, "node {id} sits outside the canvas bounds"),
            Self::Backend(id) => write!(f, "node {id} names no backend"),
            Self::Gps(reason) => write!(f, "invalid GPS source: {reason}"),
            Self::ChannelType(ty) => write!(f, "unknown channel type {ty:?}"),
            Self::NodeSettings(id) => write!(f, "invalid settings for node {id}"),
            Self::UnknownNode(id) => write!(f, "edge names unknown node {id}"),
            Self::UnknownPort(port) => {
                write!(f, "node {} has no port {}", port.node, port.port)
            }
            Self::Direction(port) => write!(
                f,
                "port {} of node {} is on the wrong side of that wire",
                port.port, port.node
            ),
            Self::TypeMismatch { from, to } => write!(
                f,
                "a {} output cannot feed a {} input",
                from.as_str(),
                to.as_str()
            ),
            Self::DuplicateEdge(port) => {
                write!(f, "duplicate wire into {}.{}", port.node, port.port)
            }
            Self::SelfEdge(id) => write!(f, "node {id} cannot wire to itself"),
            Self::Cycle(id) => write!(f, "node {id} sits on a loop of wires"),
            Self::RackCell(node) => write!(
                f,
                "rack slot for {node} is outside the {RACK_COLS}×{RACK_ROWS} grid"
            ),
            Self::DuplicateRackSlot(node) => write!(f, "node {node} is pinned twice"),
            Self::RackOverlap(node) => write!(f, "rack slot for {node} overlaps another"),
            Self::Refused { refusal, .. } => f.write_str(&refusal.text()),
        }
    }
}

impl std::error::Error for PatchError {}

impl PatchGraph {
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&PatchNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn device_nodes(&self) -> impl Iterator<Item = &PatchNode> {
        self.nodes.iter().filter(|node| node.body.opens_device())
    }

    #[must_use]
    pub fn wired_lanes(&self, array: &str) -> usize {
        self.edges
            .iter()
            .filter(|edge| edge.to.node == array)
            .filter_map(|edge| port_stream(ARRAY_LANE_PORT, &edge.to.port))
            .filter_map(|lane| usize::try_from(lane).ok())
            .map(|lane| lane + 1)
            .max()
            .unwrap_or(0)
    }

    pub fn settle_arrays(&mut self) -> bool {
        let wired: Vec<usize> = self
            .nodes
            .iter()
            .map(|node| self.wired_lanes(&node.id))
            .collect();
        let mut settled = false;
        for (node, lanes) in self.nodes.iter_mut().zip(wired) {
            if let NodeBody::Array(crate::array::ArrayNode {
                geometry: crate::array::ArrayGeometry::Explicit { positions },
                ..
            }) = &mut node.body
                && positions.len() < lanes
            {
                positions.resize(lanes, crate::array::ArrayElement::default());
                settled = true;
            }
        }
        settled
    }

    #[must_use]
    pub fn released_radios(&self, before: &Self) -> Vec<String> {
        before
            .nodes
            .iter()
            .filter_map(|old| {
                let NodeBody::Device(DeviceNode {
                    device: Some(held), ..
                }) = &old.body
                else {
                    return None;
                };
                let NodeBody::Device(now) = &self.node(&old.id)?.body else {
                    return None;
                };
                (now.device.as_ref() != Some(held)).then(|| old.id.clone())
            })
            .collect()
    }

    #[must_use]
    pub fn same_topology(&self, other: &Self) -> bool {
        self.edges == other.edges
            && self.nodes.len() == other.nodes.len()
            && self
                .nodes
                .iter()
                .zip(&other.nodes)
                .all(|(a, b)| a.id == b.id && a.body == b.body)
    }

    pub fn validate(&self) -> Result<(), PatchError> {
        resolve::compile(self, &ResolveCtx::default()).map(drop)
    }

    pub fn validate_against(&self, channels: &[ChannelDescriptor]) -> Result<(), PatchError> {
        let ctx = ResolveCtx {
            descriptors: Some(channels),
            live: None,
        };
        resolve::compile(self, &ctx).map(drop)
    }
}

pub(super) fn validate_gps_source(source: &PositionSource) -> Result<(), PatchError> {
    match source {
        PositionSource::Phone { phone } => {
            if crate::phone::valid_phone_id(phone) {
                Ok(())
            } else {
                Err(PatchError::Gps("phone id is not valid".to_owned()))
            }
        }
        PositionSource::Fixed { lat, lon, .. } => {
            if (-90.0..=90.0).contains(lat) && (-180.0..=180.0).contains(lon) {
                Ok(())
            } else {
                Err(PatchError::Gps(
                    "a fixed position has to be on the globe".to_owned(),
                ))
            }
        }
        PositionSource::Gpsd { address } => {
            if address.is_empty() || address.len() > MAX_POSITION_ENDPOINT_LEN {
                Err(PatchError::Gps(
                    "gpsd address is empty or too long".to_owned(),
                ))
            } else if !valid_host_port(address) {
                Err(PatchError::Gps(
                    "gpsd address must be a host and non-zero port".to_owned(),
                ))
            } else {
                Ok(())
            }
        }
        PositionSource::Nmea {
            device,
            baud,
            update_interval_ms,
        } => {
            if device.is_empty() || device.len() > MAX_POSITION_ENDPOINT_LEN {
                return Err(PatchError::Gps(
                    "NMEA device is empty or too long".to_owned(),
                ));
            }
            if !(MIN_NMEA_BAUD..=MAX_NMEA_BAUD).contains(baud) {
                return Err(PatchError::Gps(format!(
                    "NMEA baud rate is outside {MIN_NMEA_BAUD}..={MAX_NMEA_BAUD}"
                )));
            }
            if !(MIN_NMEA_UPDATE_INTERVAL_MS..=MAX_NMEA_UPDATE_INTERVAL_MS)
                .contains(update_interval_ms)
            {
                return Err(PatchError::Gps(format!(
                    "NMEA update interval is outside {MIN_NMEA_UPDATE_INTERVAL_MS}..={MAX_NMEA_UPDATE_INTERVAL_MS} ms"
                )));
            }
            Ok(())
        }
    }
}

pub(crate) fn valid_host_port(address: &str) -> bool {
    let Some((host, port)) = address.rsplit_once(':') else {
        return false;
    };
    if port.parse::<u16>().ok().is_none_or(|port| port == 0) {
        return false;
    }
    if let Some(ipv6) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        return ipv6.parse::<std::net::Ipv6Addr>().is_ok();
    }
    !host.is_empty()
        && !host.contains(':')
        && host
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

pub(super) fn check_geometry(node: &PatchNode) -> Result<(), PatchError> {
    let bad_position = !node.position.x.is_finite()
        || !node.position.y.is_finite()
        || node.position.x.abs() > MAX_COORD
        || node.position.y.abs() > MAX_COORD;
    let bad_size = node.size.is_some_and(|size| {
        !size.w.is_finite()
            || !size.h.is_finite()
            || size.w <= 0.0
            || size.h <= 0.0
            || size.w > MAX_NODE_SIZE
            || size.h > MAX_NODE_SIZE
    });
    if bad_position || bad_size {
        return Err(PatchError::Geometry(node.id.clone()));
    }
    Ok(())
}

impl RackLayout {
    pub fn validate(&self, graph: &PatchGraph) -> Result<(), PatchError> {
        let mut seen: Vec<&str> = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            if graph.node(&slot.node).is_none() {
                return Err(PatchError::UnknownNode(slot.node.clone()));
            }
            if seen.contains(&slot.node.as_str()) {
                return Err(PatchError::DuplicateRackSlot(slot.node.clone()));
            }
            seen.push(&slot.node);
            let cell = slot.cell;
            if cell.w == 0
                || cell.h == 0
                || u32::from(cell.x) + u32::from(cell.w) > u32::from(RACK_COLS)
                || u32::from(cell.y) + u32::from(cell.h) > u32::from(RACK_ROWS)
            {
                return Err(PatchError::RackCell(slot.node.clone()));
            }
        }
        for (i, slot) in self.slots.iter().enumerate() {
            if self.slots[..i]
                .iter()
                .any(|other| other.cell.overlaps(slot.cell))
            {
                return Err(PatchError::RackOverlap(slot.node.clone()));
            }
        }
        Ok(())
    }
}

impl RackCell {
    #[must_use]
    pub fn overlaps(self, other: Self) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

impl ChannelParams {
    #[must_use]
    pub fn default_for(type_id: &str) -> Option<Self> {
        serde_json::from_value(serde_json::json!({ "type": type_id, "settings": {} })).ok()
    }
}

#[cfg(test)]
mod tests;
