use std::collections::HashMap;

use sdrmm_wire::{
    channel::ChannelInfo,
    patch::{NodeBody, PatchGraph},
    state::DeviceSet,
};

use crate::binding;

#[derive(Clone, Debug, PartialEq)]
pub enum TuneTarget {
    Device {
        node: String,
        set: DeviceSet,
        locked: bool,
    },
    Channel {
        node: String,
        set: Option<DeviceSet>,
        locked: bool,
    },
}

impl TuneTarget {
    #[must_use]
    pub fn locked(&self) -> bool {
        match self {
            Self::Device { locked, .. } | Self::Channel { locked, .. } => *locked,
        }
    }
}

#[must_use]
pub fn tuning_locked(graph: &PatchGraph, id: &str) -> bool {
    match graph.node(id).map(|node| &node.body) {
        Some(NodeBody::Device(device)) => device.locked_streams.contains(&0),
        Some(NodeBody::Channel(channel)) => channel.tuning_locked,
        _ => false,
    }
}

#[must_use]
pub fn bound_sets(graph: &PatchGraph, sets: &[DeviceSet]) -> Vec<(String, DeviceSet)> {
    let bound: HashMap<String, u32> = binding::device_sets(graph, sets);
    graph
        .nodes
        .iter()
        .filter_map(|node| {
            let id = bound.get(&node.id)?;
            let set = sets.iter().find(|set| set.id == *id)?;
            Some((node.id.clone(), set.clone()))
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaneKind {
    Radio,
    Virtual { array: Option<String> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IqLane {
    pub source: String,
    pub stream: u32,
    pub kind: LaneKind,
}

fn iq_port_stream(port: &str) -> Option<u32> {
    let suffix = port.strip_prefix("iq")?;
    if suffix.is_empty() {
        return Some(0);
    }
    suffix.parse::<u32>().ok().map(|n| n.saturating_sub(1))
}

#[must_use]
pub fn array_of(graph: &PatchGraph, processor: &str) -> Option<String> {
    let wire = graph
        .edges
        .iter()
        .find(|edge| edge.to.node == processor && edge.to.port == "array")?;
    graph
        .node(&wire.from.node)
        .filter(|found| matches!(found.body, NodeBody::Array(_)))
        .map(|found| found.id.clone())
}

fn virtual_lane(
    graph: &PatchGraph,
    node: &str,
    port: &str,
    devices: &[(String, DeviceSet)],
) -> Option<IqLane> {
    devices.iter().find_map(|(source, set)| {
        let listed = set
            .virtual_lanes
            .iter()
            .find(|lane| lane.node == node && lane.port == port)?;
        Some(IqLane {
            source: source.clone(),
            stream: listed.stream,
            kind: LaneKind::Virtual {
                array: array_of(graph, node),
            },
        })
    })
}

#[must_use]
pub fn iq_lanes(graph: &PatchGraph, node: &str, devices: &[(String, DeviceSet)]) -> Vec<IqLane> {
    graph
        .edges
        .iter()
        .filter(|edge| edge.to.node == node && edge.to.port == "iq")
        .filter_map(|edge| match iq_port_stream(&edge.from.port) {
            Some(stream) => Some(IqLane {
                source: edge.from.node.clone(),
                stream,
                kind: LaneKind::Radio,
            }),
            None => virtual_lane(graph, &edge.from.node, &edge.from.port, devices),
        })
        .collect()
}

#[must_use]
pub fn carrier_of<'a>(
    graph: &PatchGraph,
    node: &str,
    devices: &'a [(String, DeviceSet)],
) -> Option<(&'a str, &'a ChannelInfo)> {
    let Some(NodeBody::Channel(wanted)) = graph.node(node).map(|found| &found.body) else {
        return None;
    };
    let lanes = iq_lanes(graph, node, devices);
    devices.iter().find_map(|(owner, set)| {
        set.channels
            .iter()
            .find(|channel| {
                channel.node.as_deref() == Some(node)
                    && channel.settings.params.type_id() == wanted.channel_type
                    && lanes
                        .iter()
                        .any(|lane| lane.source == *owner && lane.stream == channel.stream)
            })
            .map(|channel| (owner.as_str(), channel))
    })
}

#[must_use]
pub fn owner_of(graph: &PatchGraph, node: &str, devices: &[(String, DeviceSet)]) -> Option<String> {
    let opens = |id: &str| {
        graph
            .node(id)
            .is_some_and(|found| found.body.opens_device())
    };
    if opens(node) {
        return Some(node.to_owned());
    }
    let carrying = |channel: &str| {
        if let Some((owner, _)) = carrier_of(graph, channel, devices)
            && opens(owner)
        {
            return Some(owner.to_owned());
        }
        iq_lanes(graph, channel, devices)
            .into_iter()
            .next()
            .map(|lane| lane.source)
            .filter(|source| opens(source))
    };
    carrying(node)
        .or_else(|| binding::controlled_node_of(graph, node).and_then(|driven| carrying(&driven)))
}

#[must_use]
pub fn lane_of(graph: &PatchGraph, node: &str, devices: &[(String, DeviceSet)]) -> Option<IqLane> {
    let wired = iq_lanes(graph, node, devices).into_iter().next();
    let Some((owner, channel)) = carrier_of(graph, node, devices) else {
        return wired;
    };
    match wired {
        Some(lane) if lane.source == owner && lane.stream == channel.stream => Some(lane),
        _ => Some(IqLane {
            source: owner.to_owned(),
            stream: channel.stream,
            kind: LaneKind::Radio,
        }),
    }
}

#[must_use]
pub fn library_target(
    graph: &PatchGraph,
    devices: &[(String, DeviceSet)],
    selected: Option<&str>,
) -> Option<TuneTarget> {
    let set_of = |node: &str| {
        devices
            .iter()
            .find(|(id, _)| id == node)
            .map(|(_, set)| set.clone())
    };
    if let Some(selected) = selected {
        if let Some(set) = set_of(selected) {
            return Some(TuneTarget::Device {
                node: selected.to_owned(),
                set,
                locked: tuning_locked(graph, selected),
            });
        }
        if matches!(
            graph.node(selected).map(|node| &node.body),
            Some(NodeBody::Channel(_))
        ) {
            return Some(TuneTarget::Channel {
                node: selected.to_owned(),
                set: owner_of(graph, selected, devices).and_then(|owner| set_of(&owner)),
                locked: tuning_locked(graph, selected),
            });
        }
    }
    match devices {
        [(node, set)] => Some(TuneTarget::Device {
            node: node.clone(),
            set: set.clone(),
            locked: tuning_locked(graph, node),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::patch::{ChannelNode, DeviceNode, PatchEdge, PatchNode, PortRef, Position};

    use super::*;

    fn set(id: u32) -> DeviceSet {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "device": { "driver": "rtlsdr", "key": id.to_string(), "label": "RTL-SDR" },
            "capabilities": {
                "freq_ranges": [], "sample_rates": [], "gains": [],
                "antennas": [], "bandwidths": [], "duplex": "rx_only"
            },
            "settings": {},
            "status": "running",
            "channels": [],
        }))
        .expect("a device set")
    }

    fn node(id: &str, body: NodeBody) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn channel(locked: bool) -> NodeBody {
        NodeBody::Channel(ChannelNode {
            channel_type: "nfm".to_owned(),
            tuning_locked: locked,
        })
    }

    fn graph() -> PatchGraph {
        PatchGraph {
            nodes: vec![
                node("device:1", NodeBody::Device(DeviceNode::default())),
                node("device:2", NodeBody::Device(DeviceNode::default())),
                node("channel:1", channel(false)),
                node("channel:2", channel(true)),
                node("speaker:1", NodeBody::Speaker),
            ],
            edges: vec![PatchEdge {
                from: PortRef {
                    node: "device:1".to_owned(),
                    port: "iq".to_owned(),
                },
                to: PortRef {
                    node: "channel:1".to_owned(),
                    port: "iq".to_owned(),
                },
            }],
        }
    }

    fn one() -> Vec<(String, DeviceSet)> {
        vec![("device:1".to_owned(), set(1))]
    }

    fn two() -> Vec<(String, DeviceSet)> {
        vec![
            ("device:1".to_owned(), set(1)),
            ("device:2".to_owned(), set(2)),
        ]
    }

    #[test]
    fn takes_the_selected_device() {
        assert_eq!(
            library_target(&graph(), &two(), Some("device:2")),
            Some(TuneTarget::Device {
                node: "device:2".to_owned(),
                set: set(2),
                locked: false,
            })
        );
    }

    #[test]
    fn takes_a_selected_channel_with_the_radio_it_is_wired_to() {
        assert_eq!(
            library_target(&graph(), &two(), Some("channel:1")),
            Some(TuneTarget::Channel {
                node: "channel:1".to_owned(),
                set: Some(set(1)),
                locked: false,
            })
        );
    }

    #[test]
    fn keeps_an_unwired_channel_as_a_target_without_a_radio() {
        assert_eq!(
            library_target(&graph(), &two(), Some("channel:2")),
            Some(TuneTarget::Channel {
                node: "channel:2".to_owned(),
                set: None,
                locked: true,
            })
        );
    }

    #[test]
    fn falls_back_to_the_only_device_when_nothing_is_selected() {
        let only = Some(TuneTarget::Device {
            node: "device:1".to_owned(),
            set: set(1),
            locked: false,
        });
        assert_eq!(library_target(&graph(), &one(), None), only);
        assert_eq!(library_target(&graph(), &one(), Some("speaker:1")), only);
    }

    #[test]
    fn has_no_target_when_several_devices_are_drawn_and_none_is_selected() {
        assert_eq!(library_target(&graph(), &two(), None), None);
        assert_eq!(library_target(&graph(), &[], None), None);
    }

    fn wire(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
        PatchEdge {
            from: PortRef {
                node: from.0.to_owned(),
                port: from.1.to_owned(),
            },
            to: PortRef {
                node: to.0.to_owned(),
                port: to.1.to_owned(),
            },
        }
    }

    fn carrying(id: u32, node: &str, stream: u32) -> DeviceSet {
        let mut carried = set(id);
        carried.channels = vec![
            serde_json::from_value(serde_json::json!({
                "id": 7,
                "stream": stream,
                "node": node,
                "settings": { "frequency_hz": 1e8, "params": { "type": "nfm", "settings": {} } }
            }))
            .expect("a channel"),
        ];
        carried
    }

    fn arrayed() -> (PatchGraph, Vec<(String, DeviceSet)>) {
        let mut graph = graph();
        graph.nodes.push(node(
            "array:1",
            NodeBody::Array(sdrmm_wire::array::ArrayNode::default()),
        ));
        graph.nodes.push(node("beam:1", NodeBody::Speaker));
        graph
            .edges
            .push(wire(("array:1", "array"), ("beam:1", "array")));
        graph
            .edges
            .push(wire(("beam:1", "beam"), ("channel:2", "iq")));
        let mut lanes = set(1);
        lanes.virtual_lanes = vec![sdrmm_wire::array::VirtualLane {
            stream: 5,
            node: "beam:1".to_owned(),
            port: "beam".to_owned(),
            center_hz: 100e6,
            sample_rate: 2e6,
        }];
        (graph, vec![("device:1".to_owned(), lanes)])
    }

    #[test]
    fn a_channel_belongs_to_the_radio_that_carries_it() {
        let mut graph = graph();
        graph
            .edges
            .push(wire(("device:2", "iq"), ("channel:1", "iq")));
        let devices = vec![
            ("device:1".to_owned(), set(1)),
            ("device:2".to_owned(), carrying(2, "channel:1", 0)),
        ];
        let Some(TuneTarget::Channel {
            set: Some(owner), ..
        }) = library_target(&graph, &devices, Some("channel:1"))
        else {
            panic!("a channel target with a radio");
        };
        assert_eq!(owner.id, 2);
        assert_eq!(
            owner_of(&graph, "channel:1", &two()).as_deref(),
            Some("device:1")
        );
    }

    #[test]
    fn the_carrier_names_the_lane_a_channel_runs_on() {
        let mut two_lanes = graph();
        two_lanes
            .edges
            .push(wire(("device:1", "iq2"), ("channel:1", "iq")));
        let devices = vec![("device:1".to_owned(), carrying(1, "channel:1", 1))];
        assert_eq!(
            lane_of(&two_lanes, "channel:1", &devices),
            Some(IqLane {
                source: "device:1".to_owned(),
                stream: 1,
                kind: LaneKind::Radio,
            })
        );
        assert_eq!(
            lane_of(&graph(), "channel:1", &one()),
            Some(IqLane {
                source: "device:1".to_owned(),
                stream: 0,
                kind: LaneKind::Radio,
            })
        );
        assert_eq!(lane_of(&graph(), "channel:2", &one()), None);
        assert!(carrier_of(&graph(), "channel:1", &devices).is_none());
        let mut retyped = two_lanes.clone();
        if let Some(found) = retyped
            .nodes
            .iter_mut()
            .find(|found| found.id == "channel:1")
        {
            found.body = NodeBody::Channel(ChannelNode {
                channel_type: "am".to_owned(),
                tuning_locked: false,
            });
        }
        assert!(carrier_of(&retyped, "channel:1", &devices).is_none());
        assert!(carrier_of(&two_lanes, "channel:1", &devices).is_some());
    }

    #[test]
    fn a_beam_output_is_a_virtual_lane_of_its_array() {
        let (graph, devices) = arrayed();
        assert_eq!(
            iq_lanes(&graph, "channel:2", &devices),
            vec![IqLane {
                source: "device:1".to_owned(),
                stream: 5,
                kind: LaneKind::Virtual {
                    array: Some("array:1".to_owned()),
                },
            }]
        );
        assert_eq!(array_of(&graph, "beam:1").as_deref(), Some("array:1"));
        assert_eq!(
            owner_of(&graph, "channel:2", &devices).as_deref(),
            Some("device:1")
        );
        assert!(iq_lanes(&graph, "channel:2", &[]).is_empty());
    }

    #[test]
    fn numbered_iq_ports_name_their_stream() {
        assert_eq!(iq_port_stream("iq"), Some(0));
        assert_eq!(iq_port_stream("iq3"), Some(2));
        assert_eq!(iq_port_stream("beam"), None);
    }
}
