use sdrmm_wire::{
    device::Coherence,
    patch::{
        ARRAY_LANE_PORT, MAX_STREAMS, NodeBody, PatchEdge, PatchGraph, PatchNode, PortRef,
        Position, REFUSAL_UNNAMED_ARRAY, stream_port,
    },
    state::DeviceSet,
};
use zgui::{prelude::*, vocab::SharedString};

use crate::{
    store::Store,
    ui::{
        faces::device::lanes::rx_stream_count,
        kit_sources::{Tone, button},
        node::width_of,
        patch::graph::{new_node_id, node_ids},
    },
};

pub const OPEN_FIRST: &str = "Open the radio first";
pub const ONE_LANE: &str = "One lane only";
pub const NO_SHARED_CLOCK: &str = "Lanes share no clock";
pub const MAKE_ARRAY_TITLE: &str = "New Array wired to every lane";
const ARRAY_GAP_PX: f32 = 80.0;

fn array_label(graph: &PatchGraph, array: &str) -> String {
    graph
        .node(array)
        .and_then(|found| found.label.clone())
        .unwrap_or_else(|| REFUSAL_UNNAMED_ARRAY.to_owned())
}

fn holds_lane(graph: &PatchGraph, device: &str) -> bool {
    (0..MAX_STREAMS).any(|stream| graph.array_of_lane(device, stream).is_some())
}

#[must_use]
pub fn offers_make_array(graph: &PatchGraph, device: &str, set: &DeviceSet) -> bool {
    rx_stream_count(&set.capabilities) >= 2 && !holds_lane(graph, device)
}

pub fn can_make_array(
    graph: &PatchGraph,
    device: &str,
    set: Option<&DeviceSet>,
) -> Result<u32, String> {
    let set = set.ok_or_else(|| OPEN_FIRST.to_owned())?;
    let lanes = rx_stream_count(&set.capabilities);
    if lanes < 2 {
        return Err(ONE_LANE.to_owned());
    }
    if set.capabilities.coherence == Coherence::None {
        return Err(NO_SHARED_CLOCK.to_owned());
    }
    match (0..lanes).find_map(|stream| graph.array_of_lane(device, stream)) {
        Some(holder) => Err(format!("In {}", array_label(graph, holder))),
        None => Ok(lanes),
    }
}

#[must_use]
pub fn array_placement(graph: &PatchGraph, device: &str) -> Position {
    graph
        .node(device)
        .map_or(Position { x: 0.0, y: 0.0 }, |source| Position {
            x: source.position.x + width_of(source) + ARRAY_GAP_PX,
            y: source.position.y,
        })
}

#[must_use]
pub fn make_array(
    graph: &PatchGraph,
    device: &str,
    lanes: u32,
    body: NodeBody,
    id: &str,
) -> PatchGraph {
    let mut next = graph.clone();
    next.nodes.push(PatchNode {
        id: id.to_owned(),
        body,
        position: array_placement(graph, device),
        size: None,
        label: None,
    });
    next.edges.extend((0..lanes).map(|stream| PatchEdge {
        from: PortRef {
            node: device.to_owned(),
            port: stream_port("iq", stream),
        },
        to: PortRef {
            node: id.to_owned(),
            port: stream_port(ARRAY_LANE_PORT, stream),
        },
    }));
    next
}

fn make(store: Store, device: &str) {
    let Some(body) = NodeBody::default_for("array") else {
        store.say("Unknown node: array");
        return;
    };
    let set = store.device_set_of(device).and_then(|id| store.set_of(id));
    let checked = store
        .graph
        .with_untracked(|graph| can_make_array(graph, device, set.as_ref()));
    let Ok(lanes) = checked else {
        return;
    };
    let id = store
        .graph
        .with_untracked(|graph| new_node_id("array", &node_ids(graph)));
    let device = device.to_owned();
    let made = id.clone();
    store.edit_graph(move |graph| *graph = make_array(graph, &device, lanes, body, &made));
    store.selected.set(Some(id));
}

pub fn make_array_button(
    store: Store,
    node: String,
    set: Signal<Option<DeviceSet>>,
) -> impl IntoView {
    let checked = {
        let node = node.clone();
        Memo::new(move |_| {
            let set = set.get()?;
            store.graph.with(|graph| {
                offers_make_array(graph, &node, &set)
                    .then(|| can_make_array(graph, &node, Some(&set)))
            })
        })
    };
    move || {
        let checked = checked.get()?;
        let title = checked
            .as_ref()
            .err()
            .map_or_else(|| MAKE_ARRAY_TITLE.to_owned(), Clone::clone);
        let node = node.clone();
        Some(view! {
            box(a11y:description = SharedString::from(title)) {
                {button(|| "Make array".to_owned(), Tone::Quiet, Signal::stored(checked.is_err()), move || make(store, &node))}
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::{
        faces::array::fixtures::{placed, radio},
        node::DEFAULT_WIDTH,
    };

    fn kraken() -> PatchGraph {
        PatchGraph {
            nodes: vec![placed(
                "kraken",
                NodeBody::default_for("device").expect("device"),
                None,
            )],
            edges: Vec::new(),
        }
    }

    fn body() -> NodeBody {
        NodeBody::default_for("array").expect("array")
    }

    #[test]
    fn every_lane_is_wired_in_order() {
        let graph = make_array(&kraken(), "kraken", 5, body(), "array:1");
        assert_eq!(graph.nodes[1].body, body());
        let ports: Vec<(&str, &str)> = graph
            .edges
            .iter()
            .map(|edge| (edge.from.port.as_str(), edge.to.port.as_str()))
            .collect();
        assert_eq!(
            ports,
            vec![
                ("iq", "lane"),
                ("iq2", "lane2"),
                ("iq3", "lane3"),
                ("iq4", "lane4"),
                ("iq5", "lane5")
            ]
        );
        assert!(graph.edges.iter().all(|edge| edge.to.node == "array:1"));
        assert_eq!(graph.array_wired_lanes("array:1"), 5);
    }

    #[test]
    fn a_multi_lane_radio_with_a_shared_clock_can_make_an_array() {
        let set = radio(5, Coherence::TimeSync);
        assert_eq!(can_make_array(&kraken(), "kraken", Some(&set)), Ok(5));
        assert!(offers_make_array(&kraken(), "kraken", &set));
    }

    #[test]
    fn a_closed_single_unclocked_or_taken_radio_cannot() {
        let graph = kraken();
        assert_eq!(
            can_make_array(&graph, "kraken", None),
            Err(OPEN_FIRST.to_owned())
        );
        let single = radio(1, Coherence::TimeSync);
        assert_eq!(
            can_make_array(&graph, "kraken", Some(&single)),
            Err(ONE_LANE.to_owned())
        );
        assert!(!offers_make_array(&graph, "kraken", &single));
        let loose = radio(2, Coherence::None);
        assert_eq!(
            can_make_array(&graph, "kraken", Some(&loose)),
            Err(NO_SHARED_CLOCK.to_owned())
        );
        let mut made = make_array(&graph, "kraken", 5, body(), "array:1");
        let set = radio(5, Coherence::TimeSync);
        assert!(!offers_make_array(&made, "kraken", &set));
        assert_eq!(
            can_make_array(&made, "kraken", Some(&set)),
            Err(format!("In {REFUSAL_UNNAMED_ARRAY}"))
        );
        made.nodes[1].label = Some("North".to_owned());
        assert_eq!(
            can_make_array(&made, "kraken", Some(&set)),
            Err("In North".to_owned())
        );
    }

    #[test]
    fn the_array_lands_right_of_the_radio() {
        assert_eq!(
            array_placement(&kraken(), "kraken"),
            Position {
                x: 100.0 + DEFAULT_WIDTH + 80.0,
                y: 40.0
            }
        );
    }
}
