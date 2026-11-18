//! Processor types shaped like the built-in ones, so wiring tests see their ports without the
//! crate that brings them.

use std::sync::Once;

use crate::{
    decode::EventKind,
    processor::{ProcessorNode, ProcessorPort, ProcessorType, install_processor_types},
};

use super::super::NodeBody;

pub(super) const NAMES: [&str; 7] = [
    "df",
    "beamformer",
    "passive_radar",
    "stitch",
    "spatial_spectrum",
    "correlator",
    "polarimeter",
];

fn kind(type_id: &str, emits: Vec<EventKind>, ports: Vec<ProcessorPort>) -> ProcessorType {
    let name = match type_id {
        "passive_radar" => "Passive radar",
        "df" => "Direction finder",
        other => other,
    };
    let mut kind: ProcessorType =
        serde_json::from_value(serde_json::json!({"type_id": type_id, "name": name}))
            .expect("a bare type");
    kind.emits = emits;
    kind.ports = ports;
    kind
}

fn lane(name: &str) -> ProcessorPort {
    ProcessorPort::Lane {
        name: name.to_owned(),
        note: None,
    }
}

fn types() -> Vec<ProcessorType> {
    let events = ProcessorPort::Events { note: None };
    vec![
        kind(
            "df",
            vec![EventKind::DF],
            vec![events.clone(), ProcessorPort::Bearing { note: None }],
        ),
        kind(
            "beamformer",
            Vec::new(),
            vec![ProcessorPort::Steer { note: None }, lane("beam")],
        ),
        kind(
            "passive_radar",
            vec![EventKind::RADAR, EventKind::RADAR_SITES],
            vec![
                ProcessorPort::Position {
                    name: "tx".to_owned(),
                    note: None,
                },
                ProcessorPort::Tracks {
                    name: "truth".to_owned(),
                    note: None,
                },
                events,
            ],
        ),
        kind("stitch", Vec::new(), vec![lane("wide")]),
        kind("spatial_spectrum", Vec::new(), Vec::new()),
        kind("correlator", Vec::new(), Vec::new()),
        kind("polarimeter", Vec::new(), vec![lane("beam")]),
    ]
}

/// Lists the processor types every wiring test sees.
pub(crate) fn install() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| install_processor_types(types()));
}

/// A node running the processor type `type_id`, on its defaults.
pub(crate) fn body(type_id: &str) -> NodeBody {
    install();
    NodeBody::Processor(ProcessorNode {
        processor_type: type_id.to_owned(),
        settings: serde_json::Value::Null,
    })
}
