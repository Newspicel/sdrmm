//! What an array processor declares to the catalog, built in or from a plugin, and the patch
//! node that runs one.

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use utoipa::ToSchema;

use crate::{
    array::{MAX_ARRAY_LANES, MIN_ARRAY_LANES},
    channel::SettingField,
    decode::{Capability, EventKind, KindSet},
    patch::{
        ARRAY_PORT, Accept, BEARING_PORT, EVENTS_PORT, Face, IqOrigin, Payload, PortSpec,
        STEER_PORT, input, output,
    },
    present::Unit,
};

use super::ProcessorParams;

const DEFAULT_FACE_WIDTH: u16 = 400;
const DERIVED_IQ: &[IqOrigin] = &[IqOrigin::Derived];
const TRACKS: &str = "tracks";

/// What a processor needs of the array before its results mean anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(default)]
pub struct ArrayNeeds {
    pub time: bool,
    pub phase: bool,
    pub gain: bool,
    pub geometry: bool,
}

/// One number a processor reports, in the order its values arrive.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ReadoutSpec {
    pub key: String,
    pub label: String,
    #[serde(default = "no_unit")]
    pub unit: Unit,
    #[serde(default)]
    pub digits: u8,
}

const fn no_unit() -> Unit {
    Unit::None
}

impl ReadoutSpec {
    #[must_use]
    pub fn new(key: &str, label: &str, unit: Unit, digits: u8) -> Self {
        Self {
            key: key.to_owned(),
            label: label.to_owned(),
            unit,
            digits,
        }
    }
}

/// An array processor a plugin declares, as the patch catalog lists it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProcessorType {
    pub type_id: String,
    pub name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default = "min_lanes")]
    pub min_lanes: u32,
    #[serde(default = "max_lanes")]
    pub max_lanes: u32,
    #[serde(default)]
    pub needs: ArrayNeeds,
    /// The form a face renders for its settings.
    #[serde(default)]
    pub settings: Vec<SettingField>,
    #[serde(default)]
    pub readouts: Vec<ReadoutSpec>,
    /// Kinds of the events it emits, each one the plugin declares.
    #[serde(default)]
    pub emits: Vec<EventKind>,
    /// What a fresh node starts on.
    #[serde(default)]
    pub defaults: Value,
    #[serde(default)]
    pub facets: super::view::ProcessorFacets,
    #[serde(default = "default_face")]
    pub face: Face,
    /// Ports beside the array input, in the order its face shows them.
    #[serde(default)]
    pub ports: Vec<ProcessorPort>,
    /// The settings keys that hold the band it listens to, when it narrows the array itself.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub band: Option<BandKeys>,
    /// Every port of a node of this type, resolved when the catalog is built.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub resolved_ports: Vec<PortSpec>,
}

/// The settings keys a processor keeps its band in: an offset from the array centre and a
/// width, both in hertz.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
pub struct BandKeys {
    pub offset: String,
    pub width: String,
}

const fn default_face() -> Face {
    Face::fit(DEFAULT_FACE_WIDTH)
}

/// A port a processor declares, resolved against the event catalog each time it is read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(tag = "port", rename_all = "snake_case")]
pub enum ProcessorPort {
    /// Events of the kinds the type emits.
    Events {
        #[serde(default)]
        note: Option<String>,
    },
    /// A live bearing out, to steer another processor.
    Bearing {
        #[serde(default)]
        note: Option<String>,
    },
    /// A live bearing in to steer at.
    Steer {
        #[serde(default)]
        note: Option<String>,
    },
    /// A lane of IQ the processor writes.
    Lane {
        name: String,
        #[serde(default)]
        note: Option<String>,
    },
    /// A position the processor reads, such as where a transmitter stands.
    Position {
        name: String,
        #[serde(default)]
        note: Option<String>,
    },
    /// Decoded tracks the processor reads.
    Tracks {
        name: String,
        #[serde(default)]
        note: Option<String>,
    },
}

impl ProcessorPort {
    fn spec(&self, emits: &[EventKind]) -> PortSpec {
        let (spec, note) = match self {
            Self::Events { note } => {
                let kinds: KindSet = emits.iter().cloned().collect();
                (output(EVENTS_PORT, Payload::Events { kinds }), note)
            }
            Self::Bearing { note } => (output(BEARING_PORT, Payload::Bearing), note),
            Self::Steer { note } => (input(STEER_PORT, Payload::Bearing), note),
            Self::Lane { name, note } => (output(name, Payload::iq(DERIVED_IQ)), note),
            Self::Position { name, note } => (input(name, Payload::Position), note),
            Self::Tracks { name, note } => (
                input(name, Payload::events(Accept::Caps(&[Capability::Track])))
                    .many()
                    .takes(TRACKS),
                note,
            ),
        };
        match note {
            Some(note) => spec.noted(note),
            None => spec,
        }
    }

    /// The name of a lane this port writes, if it writes one.
    #[must_use]
    pub fn lane(&self) -> Option<&str> {
        match self {
            Self::Lane { name, .. } => Some(name),
            _ => None,
        }
    }
}

const fn min_lanes() -> u32 {
    MIN_ARRAY_LANES
}

const fn max_lanes() -> u32 {
    MAX_ARRAY_LANES
}

impl ProcessorType {
    /// Every port a node of this type has: the array in, then the ports it declares.
    #[must_use]
    pub fn port_specs(&self) -> Vec<PortSpec> {
        let mut ports = vec![input(ARRAY_PORT, Payload::Array)];
        ports.extend(self.ports.iter().map(|port| port.spec(&self.emits)));
        ports
    }

    /// This type as the catalog lists it, its ports resolved against the event kinds known now.
    #[must_use]
    pub fn resolved(&self) -> Self {
        Self {
            resolved_ports: self.port_specs(),
            ..self.clone()
        }
    }

    /// The lanes it writes, in port order.
    pub fn lane_ports(&self) -> impl Iterator<Item = &str> {
        self.ports.iter().filter_map(ProcessorPort::lane)
    }

    #[must_use]
    pub fn steers(&self) -> bool {
        self.ports
            .iter()
            .any(|port| matches!(port, ProcessorPort::Steer { .. }))
    }
}

/// A patch node running an array processor.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct ProcessorNode {
    #[serde(default)]
    pub processor_type: String,
    #[serde(default)]
    pub settings: Value,
}

impl ProcessorNode {
    #[must_use]
    pub fn params(&self) -> ProcessorParams {
        ProcessorParams {
            type_id: self.processor_type.clone(),
            settings: self.settings.clone(),
        }
    }
}

static TYPES: LazyLock<RwLock<Arc<Vec<ProcessorType>>>> =
    LazyLock::new(|| RwLock::new(Arc::new(Vec::new())));

/// The processors open to new nodes right now. Read on the control plane only.
#[must_use]
pub fn processor_types() -> Arc<Vec<ProcessorType>> {
    TYPES.read().unwrap_or_else(PoisonError::into_inner).clone()
}

#[must_use]
pub fn processor_type(type_id: &str) -> Option<ProcessorType> {
    processor_types()
        .iter()
        .find(|kind| kind.type_id == type_id)
        .cloned()
}

/// Replaces the processors the catalog lists.
pub fn install_processor_types(types: Vec<ProcessorType>) {
    *TYPES.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(types);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn radar() -> ProcessorType {
        let mut kind: ProcessorType =
            serde_json::from_str(r#"{"type_id":"radar","name":"Radar"}"#).expect("parses");
        kind.emits = vec![EventKind::RADAR];
        kind.ports = vec![
            ProcessorPort::Events { note: None },
            ProcessorPort::Position {
                name: "tx".into(),
                note: None,
            },
            ProcessorPort::Lane {
                name: "beam".into(),
                note: Some("The beam".into()),
            },
        ];
        kind
    }

    #[test]
    fn a_type_parses_with_only_its_name() {
        let kind: ProcessorType =
            serde_json::from_str(r#"{"type_id":"x","name":"X"}"#).expect("parses");
        assert_eq!(
            (kind.min_lanes, kind.max_lanes),
            (MIN_ARRAY_LANES, MAX_ARRAY_LANES)
        );
        assert_eq!(kind.face, default_face());
        assert_eq!(kind.port_specs().len(), 1, "only the array in");
        assert!(ProcessorNode::default().params().problem().is_some());
    }

    #[test]
    fn declared_ports_follow_the_array_and_the_events() {
        let kind = radar();
        let names: Vec<String> = kind
            .port_specs()
            .into_iter()
            .map(|port| port.name)
            .collect();
        assert_eq!(names, [ARRAY_PORT, EVENTS_PORT, "tx", "beam"]);
        assert_eq!(kind.lane_ports().collect::<Vec<_>>(), ["beam"]);
        assert!(!kind.steers());
    }

    #[test]
    fn readouts_carry_their_unit() {
        let spec = ReadoutSpec::new("power", "Power", Unit::Db, 1);
        assert_eq!(spec.unit, Unit::Db);
    }
}
