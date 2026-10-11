use std::time::Duration;

use sdrmm_wire::{
    channel::ChannelDescriptor,
    device::DeviceInfo,
    patch::{
        ChannelNode, DeviceNode, DeviceRef, NodeBody, NodeTypeInfo, PatchCatalog, PatchEdge,
        PatchGraph, PatchNode, PortDirection, PortRef, PortSpec, Position,
    },
};
use zgui::prelude::*;

use super::Screen;

pub const CHANNEL: &str = "channel";
const FIRST_CHANNEL: &str = "nfm";
const COLUMNS: usize = 6;
const STEP_X: f32 = 720.0;
const STEP_Y: f32 = 620.0;
const FACE_SETTLE: Duration = Duration::from_millis(900);
const MODE_SETTLE: Duration = Duration::from_millis(1500);

pub struct Built {
    pub graph: PatchGraph,
    pub refused: Vec<String>,
}

fn place(index: usize) -> Position {
    Position {
        x: (index % COLUMNS) as f32 * STEP_X,
        y: (index / COLUMNS) as f32 * STEP_Y,
    }
}

fn radio(devices: &[DeviceInfo]) -> Option<DeviceRef> {
    devices
        .iter()
        .find(|info| info.key == "band")
        .or_else(|| devices.first())
        .map(DeviceRef::from_info)
}

fn body(info: &NodeTypeInfo, devices: &[DeviceInfo]) -> NodeBody {
    match &info.default_body {
        NodeBody::Device(_) => NodeBody::Device(DeviceNode {
            device: radio(devices),
            ..DeviceNode::default()
        }),
        NodeBody::Channel(_) => NodeBody::Channel(ChannelNode {
            channel_type: FIRST_CHANNEL.to_owned(),
            tuning_locked: false,
        }),
        other => other.clone(),
    }
}

fn ports(body: &NodeBody, direction: PortDirection) -> Vec<PortSpec> {
    body.ports()
        .into_iter()
        .filter(|port| port.direction == direction)
        .collect()
}

fn edge(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
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

fn feed(graph: &mut PatchGraph, descriptors: &[ChannelDescriptor], input: &PortSpec) {
    let Some(newest) = graph.nodes.last().cloned() else {
        return;
    };
    let sources: Vec<(String, String)> = graph.nodes[..graph.nodes.len() - 1]
        .iter()
        .flat_map(|node| {
            ports(&node.body, PortDirection::Out)
                .into_iter()
                .filter(|port| port.port_type == input.port_type)
                .map(|port| (node.id.clone(), port.name))
        })
        .collect();
    for (node, port) in sources {
        graph
            .edges
            .push(edge((&node, &port), (&newest.id, &input.name)));
        if graph.validate_against(descriptors).is_ok() {
            return;
        }
        graph.edges.pop();
    }
}

pub fn build(devices: &[DeviceInfo], descriptors: &[ChannelDescriptor]) -> Built {
    let mut built = Built {
        graph: PatchGraph::default(),
        refused: Vec::new(),
    };
    for (index, info) in PatchCatalog::build().nodes.iter().enumerate() {
        let node = PatchNode {
            id: info.kind.clone(),
            body: body(info, devices),
            position: place(index),
            size: None,
            label: None,
        };
        let inputs = ports(&node.body, PortDirection::In);
        built.graph.nodes.push(node);
        if built.graph.validate_against(descriptors).is_err() {
            built.graph.nodes.pop();
            built.refused.push(info.kind.clone());
            continue;
        }
        for input in &inputs {
            feed(&mut built.graph, descriptors, input);
        }
    }
    built
}

const STILL_TRIES: u32 = 240;
const STILL_FRAMES: u32 = 5;

pub fn fit(screen: &mut Screen) {
    let at = screen.nth(".zf-controls__button", 2);
    screen.click(at);
    hold_still(screen);
}

pub fn hold_still(screen: &mut Screen) {
    let mut last = None;
    let mut still = 0;
    for _ in 0..STILL_TRIES {
        screen.run_for(Duration::from_millis(16));
        let now = screen.css_box(".zf-node");
        still = if now == last { still + 1 } else { 0 };
        if still >= STILL_FRAMES {
            return;
        }
        last = now;
    }
}

pub fn faces(screen: &mut Screen, ids: &[String]) {
    let store = screen.store();
    for id in ids {
        store.expanded.set(Some(id.clone()));
        screen.run_for(FACE_SETTLE);
        screen.shot(&format!("faces/{id}"));
        store.expanded.set(None);
        screen.frame();
    }
}

pub fn modes(screen: &mut Screen) {
    let store = screen.store();
    let types: Vec<String> = store
        .channel_types
        .get_untracked()
        .iter()
        .map(|descriptor| descriptor.type_id.clone())
        .collect();
    store.expanded.set(Some(CHANNEL.to_owned()));
    for type_id in types {
        let wanted = type_id.clone();
        store.edit_graph(move |graph| {
            if let Some(node) = graph.nodes.iter_mut().find(|node| node.id == CHANNEL)
                && let NodeBody::Channel(channel) = &mut node.body
            {
                channel.channel_type = wanted;
            }
            graph.edges.retain(|edge| edge.from.node != CHANNEL);
        });
        screen.run_for(MODE_SETTLE);
        screen.shot(&format!("faces/channel-{type_id}"));
    }
    store.expanded.set(None);
}
