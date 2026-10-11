use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

use sdrmm_wire::{
    array::ArrayTuneRequest,
    channel::ChannelInfo,
    patch::{NodeBody, PatchGraph, PortRef, port_stream, stream_port},
    state::DeviceSet,
};

use super::radio::Lane;
use crate::{binding, store::Store};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VirtualPort {
    pub node: String,
    pub port: String,
    pub array: Option<String>,
}

#[must_use]
pub fn lane_for(graph: &PatchGraph, sets: &[DeviceSet], node: &str) -> Option<Lane> {
    let from = &graph
        .edges
        .iter()
        .find(|edge| edge.to.node == node && edge.to.port == "iq")?
        .from;
    let devices = binding::device_sets(graph, sets);
    if let Some(stream) = port_stream("iq", &from.port) {
        return devices.get(&from.node).map(|set| Lane {
            set: *set,
            stream,
            device: from.node.clone(),
            virtual_port: None,
        });
    }
    let source = graph.node(&from.node)?;
    if !source.body.lane_outputs().contains(&from.port.as_str()) {
        return None;
    }
    virtual_lane(graph, sets, &devices, from)
}

fn virtual_lane(
    graph: &PatchGraph,
    sets: &[DeviceSet],
    devices: &HashMap<String, u32>,
    from: &PortRef,
) -> Option<Lane> {
    let mut bound: Vec<(&String, &u32)> = devices.iter().collect();
    bound.sort();
    bound.into_iter().find_map(|(device, id)| {
        let set = sets.iter().find(|set| set.id == *id)?;
        let listed = set
            .virtual_lanes
            .iter()
            .find(|lane| lane.node == from.node && lane.port == from.port)?;
        Some(Lane {
            set: *id,
            stream: listed.stream,
            device: device.clone(),
            virtual_port: Some(VirtualPort {
                node: from.node.clone(),
                port: from.port.clone(),
                array: array_of(graph, &from.node),
            }),
        })
    })
}

#[must_use]
pub fn array_of(graph: &PatchGraph, processor: &str) -> Option<String> {
    let wire = graph
        .edges
        .iter()
        .find(|edge| edge.to.node == processor && edge.to.port == "array")?;
    graph
        .node(&wire.from.node)
        .filter(|source| matches!(source.body, NodeBody::Array(_)))
        .map(|source| source.id.clone())
}

impl Lane {
    #[must_use]
    pub fn tuned_array(&self) -> Option<&str> {
        self.virtual_port.as_ref()?.array.as_deref()
    }

    #[must_use]
    pub fn feeds(&self) -> PortRef {
        match &self.virtual_port {
            Some(port) => PortRef {
                node: port.node.clone(),
                port: port.port.clone(),
            },
            None => PortRef {
                node: self.device.clone(),
                port: stream_port("iq", self.stream),
            },
        }
    }
}

#[must_use]
pub fn virtual_faces(
    graph: &PatchGraph,
    channels: &[ChannelInfo],
    port: &VirtualPort,
) -> HashMap<u32, String> {
    let mut taken: HashSet<u32> = HashSet::new();
    let mut faces = HashMap::new();
    for node in &graph.nodes {
        let NodeBody::Channel(channel) = &node.body else {
            continue;
        };
        let wired = graph.edges.iter().any(|edge| {
            edge.to.node == node.id
                && edge.to.port == "iq"
                && edge.from.node == port.node
                && edge.from.port == port.port
        });
        if !wired {
            continue;
        }
        let found = channels.iter().find(|info| {
            !taken.contains(&info.id) && info.settings.params.type_id() == channel.channel_type
        });
        if let Some(info) = found {
            taken.insert(info.id);
            faces.insert(info.id, node.id.clone());
        }
    }
    faces
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Queue {
    waiting: Option<f64>,
    busy: bool,
}

thread_local! {
    static QUEUES: RefCell<HashMap<String, Queue>> = RefCell::new(HashMap::new());
}

fn enqueue(node: &str, hz: f64) -> bool {
    QUEUES.with(|queues| {
        let mut queues = queues.borrow_mut();
        let queue = queues.entry(node.to_owned()).or_default();
        queue.waiting = Some(hz);
        !std::mem::replace(&mut queue.busy, true)
    })
}

fn next(node: &str) -> Option<f64> {
    QUEUES.with(|queues| {
        let mut queues = queues.borrow_mut();
        let queue = queues.entry(node.to_owned()).or_default();
        let hz = queue.waiting.take();
        queue.busy = hz.is_some();
        hz
    })
}

pub fn tune_array(store: Store, node: String, hz: f64) {
    if !enqueue(&node, hz) {
        return;
    }
    zgui::task::spawn_local(async move {
        let path = format!("/api/arrays/{node}/tune");
        while let Some(hz) = next(&node) {
            let body = ArrayTuneRequest {
                center_hz: Some(hz),
                gain: None,
            };
            if let Err(error) = store
                .api()
                .patch::<_, serde::de::IgnoredAny>(&path, &body)
                .await
            {
                store.say(format!("Cannot tune the array: {error:#}"));
            }
        }
    });
}

#[must_use]
pub fn heard_hz(frequency_hz: f64, shift_hz: Option<f64>) -> f64 {
    frequency_hz + shift_hz.unwrap_or(0.0).round()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{
        array::VirtualLane,
        patch::{ChannelNode, DeviceNode, DeviceRef, PatchEdge, PatchNode, Position},
    };

    use super::*;

    fn node(id: &str, kind: &str) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body: NodeBody::default_for(kind).unwrap_or(NodeBody::Scope),
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn channel_node(id: &str, type_id: &str) -> PatchNode {
        PatchNode {
            body: NodeBody::Channel(ChannelNode {
                channel_type: type_id.to_owned(),
                tuning_locked: false,
            }),
            ..node(id, "scope")
        }
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

    fn set(id: u32, key: &str, lanes: Vec<VirtualLane>) -> DeviceSet {
        let mut set: DeviceSet = serde_json::from_value(serde_json::json!({
            "id": id,
            "device": { "driver": "virtual", "key": key, "label": key },
            "settings": {},
            "capabilities": {
                "freq_ranges": [], "sample_rates": [], "gains": [], "antennas": [], "bandwidths": []
            },
            "status": "running",
            "channels": []
        }))
        .expect("a device set");
        set.virtual_lanes = lanes;
        set
    }

    fn device(id: &str, key: &str) -> PatchNode {
        PatchNode {
            body: NodeBody::Device(DeviceNode {
                device: Some(DeviceRef {
                    backend: "virtual".to_owned(),
                    serial: None,
                    key: Some(key.to_owned()),
                }),
                locked_streams: Vec::new(),
                split_tuning: false,
            }),
            ..node(id, "scope")
        }
    }

    fn channel(id: u32, type_id: &str) -> ChannelInfo {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "stream": 3,
            "settings": { "frequency_hz": 100e6, "params": { "type": type_id, "settings": {} } },
            "out_of_band": null
        }))
        .expect("a channel")
    }

    fn beam_graph() -> PatchGraph {
        PatchGraph {
            nodes: vec![
                device("radio", "kraken"),
                node("array", "array"),
                node("beam", "beamformer"),
                node("scope", "scope"),
            ],
            edges: vec![
                wire(("array", "array"), ("beam", "array")),
                wire(("beam", "beam"), ("scope", "iq")),
            ],
        }
    }

    #[test]
    fn a_device_lane_reads_its_stream_from_the_port() {
        let graph = PatchGraph {
            nodes: vec![device("radio", "kraken"), node("scope", "scope")],
            edges: vec![wire(("radio", "iq3"), ("scope", "iq"))],
        };
        let sets = [set(7, "kraken", Vec::new())];
        let lane = lane_for(&graph, &sets, "scope").expect("a lane");
        assert_eq!(
            (lane.set, lane.stream, lane.device.as_str()),
            (7, 2, "radio")
        );
        assert_eq!(lane.virtual_port, None);
        assert_eq!(lane.feeds().port, "iq3");
        assert_eq!(lane_for(&graph, &sets, "radio"), None);
    }

    #[test]
    fn a_virtual_lane_reads_the_stream_its_device_set_lists_and_tunes_the_array() {
        let graph = beam_graph();
        let listed = VirtualLane {
            stream: 5,
            node: String::from("beam"),
            port: String::from("beam"),
            center_hz: 433.9e6,
            sample_rate: 48_000.0,
        };
        let sets = [set(7, "kraken", vec![listed])];
        let lane = lane_for(&graph, &sets, "scope").expect("a lane");
        assert_eq!((lane.set, lane.stream), (7, 5));
        assert_eq!(lane.tuned_array(), Some("array"));
        assert_eq!(lane.feeds().node, "beam");
        assert_eq!(lane.feeds().port, "beam");
        assert_eq!(
            lane_for(&graph, &[set(7, "kraken", Vec::new())], "scope"),
            None
        );
    }

    #[test]
    fn a_channel_on_a_virtual_port_binds_to_the_matching_decoder() {
        let mut graph = beam_graph();
        graph.nodes.push(channel_node("nfm", "nfm"));
        graph.edges.push(wire(("beam", "beam"), ("nfm", "iq")));
        let port = VirtualPort {
            node: String::from("beam"),
            port: String::from("beam"),
            array: None,
        };
        let faces = virtual_faces(&graph, &[channel(1, "am"), channel(2, "nfm")], &port);
        assert_eq!(faces.get(&2).map(String::as_str), Some("nfm"));
        assert_eq!(faces.len(), 1);
    }

    #[test]
    fn an_array_tune_queue_keeps_only_the_newest_frequency() {
        assert!(enqueue("q", 1.0));
        assert!(!enqueue("q", 2.0));
        assert_eq!(next("q"), Some(2.0));
        assert_eq!(next("q"), None);
        assert!(enqueue("q", 3.0));
    }

    #[test]
    fn a_marker_sits_where_the_channel_hears() {
        assert_eq!(heard_hz(100e6, Some(12.4)), 100e6 + 12.0);
        assert_eq!(heard_hz(100e6, None), 100e6);
    }
}
