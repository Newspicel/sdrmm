use std::collections::{HashMap, HashSet};

use sdrmm_wire::patch::{PatchEdge, PatchGraph, PatchNode, PortDirection, PortType};
use zgui_flow::prelude::{ChangeSet, Edge, EdgeChange, Graph, Node, NodeChange, Point, Size};

use super::graph;
use crate::ui::node::{self, Place};

const MOVED_PX: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub struct Drawn {
    pub id: String,
    pub position: Point,
    pub size: Option<Size>,
    pub class: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wire {
    pub id: String,
    pub source: String,
    pub source_port: String,
    pub target: String,
    pub target_port: String,
    pub class: &'static str,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Held {
    pub dragging: HashSet<String>,
    pub resizing: bool,
}

#[must_use]
pub fn drawn(node: &PatchNode) -> Drawn {
    let kind = node.body.kind();
    let size = graph::is_resizable(kind).then(|| {
        let natural = graph::natural_size(kind);
        let (w, h) = node
            .size
            .map_or((natural.w, natural.h.unwrap_or(0.0)), |size| {
                (size.w, size.h)
            });
        Size::new(f64::from(w), f64::from(h))
    });
    Drawn {
        id: node.id.clone(),
        position: Point::new(f64::from(node.position.x), f64::from(node.position.y)),
        size,
        class: node::category_class(node.body.category()),
    }
}

#[must_use]
pub fn wire(edge: &PatchEdge, port_type: PortType) -> Wire {
    Wire {
        id: graph::edge_key(edge),
        source: edge.from.node.clone(),
        source_port: edge.from.port.clone(),
        target: edge.to.node.clone(),
        target_port: edge.to.port.clone(),
        class: port_type.as_str(),
    }
}

#[must_use]
pub fn carried(places: &HashMap<String, Vec<Place>>, edge: &PatchEdge) -> PortType {
    places
        .get(&edge.from.node)
        .and_then(|found| {
            found
                .iter()
                .find(|place| place.name == edge.from.port && place.direction == PortDirection::Out)
        })
        .map_or(PortType::Iq, |place| place.port_type)
}

#[must_use]
pub fn wires(graph: &PatchGraph, places: &HashMap<String, Vec<Place>>) -> Vec<Wire> {
    graph
        .edges
        .iter()
        .map(|edge| wire(edge, carried(places, edge)))
        .collect()
}

fn flow_node(drawn: &Drawn) -> Node<()> {
    let mut node = Node::with_data(drawn.id.as_str(), drawn.position, ()).class(drawn.class);
    node.size = drawn.size;
    node
}

fn flow_edge(wire: &Wire) -> Edge<()> {
    Edge::with_data(
        wire.id.as_str(),
        wire.source.as_str(),
        wire.target.as_str(),
        (),
    )
    .source_handle(wire.source_port.as_str())
    .target_handle(wire.target_port.as_str())
    .class(wire.class)
}

fn moved(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() > MOVED_PX || (a.y - b.y).abs() > MOVED_PX
}

fn resized(a: Option<Size>, b: Option<Size>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => {
            (a.width - b.width).abs() > MOVED_PX || (a.height - b.height).abs() > MOVED_PX
        }
        (None, None) => false,
        _ => true,
    }
}

fn node_changes(nodes: &[Drawn], current: &Graph<(), ()>, held: &Held) -> Vec<NodeChange<()>> {
    let wanted: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let mut changes: Vec<NodeChange<()>> = current
        .nodes()
        .filter(|node| !wanted.contains(node.id.as_str()))
        .map(|node| NodeChange::Remove(node.id.clone()))
        .collect();
    for drawn in nodes {
        let Some(shown) = current.node(&drawn.id.as_str().into()) else {
            changes.push(NodeChange::Add(flow_node(drawn)));
            continue;
        };
        if held.dragging.contains(&drawn.id) {
            continue;
        }
        if moved(shown.position, drawn.position) {
            changes.push(NodeChange::Position {
                id: shown.id.clone(),
                position: drawn.position,
            });
        }
        if !held.resizing && resized(shown.size, drawn.size) {
            changes.push(NodeChange::Size {
                id: shown.id.clone(),
                size: drawn.size,
            });
        }
    }
    changes
}

fn edge_changes(nodes: &[Drawn], wires: &[Wire], current: &Graph<(), ()>) -> Vec<EdgeChange<()>> {
    let kept: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();
    let wanted: HashMap<&str, &Wire> = wires.iter().map(|wire| (wire.id.as_str(), wire)).collect();
    let mut changes = Vec::new();
    for edge in current.edges() {
        let same = wanted
            .get(edge.id.as_str())
            .is_some_and(|wire| edge.class.as_deref() == Some(wire.class));
        let ends_kept = kept.contains(edge.source.as_str()) && kept.contains(edge.target.as_str());
        if !same && ends_kept {
            changes.push(EdgeChange::Remove(edge.id.clone()));
        }
    }
    for wire in wires {
        let shown = current
            .edge(&wire.id.as_str().into())
            .is_some_and(|edge| edge.class.as_deref() == Some(wire.class));
        if !shown {
            changes.push(EdgeChange::Add(flow_edge(wire)));
        }
    }
    changes
}

#[must_use]
pub fn diff(
    nodes: &[Drawn],
    wires: &[Wire],
    current: &Graph<(), ()>,
    held: &Held,
) -> ChangeSet<(), ()> {
    ChangeSet {
        nodes: node_changes(nodes, current, held),
        edges: edge_changes(nodes, wires, current),
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::patch::{NodeBody, PortRef, Position, Size as NodeSize};

    use super::*;

    fn patch(id: &str, body: NodeBody, x: f32) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x, y: 10.0 },
            size: None,
            label: None,
        }
    }

    fn link(from: &str, to: &str) -> PatchEdge {
        PatchEdge {
            from: PortRef {
                node: from.to_owned(),
                port: "iq".to_owned(),
            },
            to: PortRef {
                node: to.to_owned(),
                port: "iq".to_owned(),
            },
        }
    }

    fn applied(changes: ChangeSet<(), ()>) -> Graph<(), ()> {
        let mut shown = Graph::new();
        shown.apply(changes);
        shown
    }

    #[test]
    fn a_resizable_node_carries_its_size_and_a_fitted_one_does_not() {
        let scope = drawn(&patch("scope", NodeBody::Scope, 0.0));
        assert_eq!(scope.size, Some(Size::new(520.0, 360.0)));
        let mut sized = patch("scope", NodeBody::Scope, 0.0);
        sized.size = Some(NodeSize { w: 600.0, h: 400.0 });
        assert_eq!(drawn(&sized).size, Some(Size::new(600.0, 400.0)));
        assert_eq!(drawn(&patch("speaker", NodeBody::Speaker, 0.0)).size, None);
    }

    #[test]
    fn an_empty_flow_receives_every_node_and_wire() {
        let nodes = [
            drawn(&patch("scope", NodeBody::Scope, 0.0)),
            drawn(&patch("speaker", NodeBody::Speaker, 400.0)),
        ];
        let wires = [wire(&link("scope", "speaker"), PortType::Audio)];
        let changes = diff(&nodes, &wires, &Graph::new(), &Held::default());
        assert_eq!(changes.nodes.len(), 2);
        assert_eq!(changes.edges.len(), 1);
        let shown = applied(changes);
        assert_eq!(shown.node_count(), 2);
        let edge = shown.edges().next().expect("one edge");
        assert_eq!(edge.class.as_deref(), Some("audio"));
        assert_eq!(
            edge.source_handle.as_ref().map(|id| id.as_str()),
            Some("iq")
        );
    }

    #[test]
    fn a_synced_flow_needs_no_change() {
        let nodes = [drawn(&patch("scope", NodeBody::Scope, 0.0))];
        let shown = applied(diff(&nodes, &[], &Graph::new(), &Held::default()));
        assert!(diff(&nodes, &[], &shown, &Held::default()).is_empty());
    }

    #[test]
    fn a_moved_node_moves_unless_it_is_being_dragged() {
        let before = [drawn(&patch("scope", NodeBody::Scope, 0.0))];
        let shown = applied(diff(&before, &[], &Graph::new(), &Held::default()));
        let after = [drawn(&patch("scope", NodeBody::Scope, 120.0))];
        let changes = diff(&after, &[], &shown, &Held::default());
        assert!(matches!(
            changes.nodes.as_slice(),
            [NodeChange::Position { position, .. }] if position.x == 120.0
        ));
        let held = Held {
            dragging: HashSet::from(["scope".to_owned()]),
            resizing: false,
        };
        assert!(diff(&after, &[], &shown, &held).is_empty());
    }

    #[test]
    fn a_resize_in_progress_keeps_its_size() {
        let before = [drawn(&patch("scope", NodeBody::Scope, 0.0))];
        let shown = applied(diff(&before, &[], &Graph::new(), &Held::default()));
        let mut grown = patch("scope", NodeBody::Scope, 0.0);
        grown.size = Some(NodeSize { w: 700.0, h: 500.0 });
        let after = [drawn(&grown)];
        assert_eq!(diff(&after, &[], &shown, &Held::default()).nodes.len(), 1);
        let held = Held {
            resizing: true,
            ..Held::default()
        };
        assert!(diff(&after, &[], &shown, &held).is_empty());
    }

    #[test]
    fn a_removed_node_goes_with_its_wires_and_a_retyped_wire_is_redrawn() {
        let nodes = [
            drawn(&patch("a", NodeBody::Scope, 0.0)),
            drawn(&patch("b", NodeBody::Speaker, 400.0)),
            drawn(&patch("c", NodeBody::Speaker, 800.0)),
        ];
        let wires = [
            wire(&link("a", "b"), PortType::Iq),
            wire(&link("b", "c"), PortType::Iq),
        ];
        let shown = applied(diff(&nodes, &wires, &Graph::new(), &Held::default()));
        let changes = diff(
            &nodes[..2],
            &[wire(&link("a", "b"), PortType::Audio)],
            &shown,
            &Held::default(),
        );
        assert_eq!(changes.nodes, vec![NodeChange::Remove("c".into())]);
        assert_eq!(changes.edges.len(), 2);
        let shown = applied_onto(shown, changes);
        assert_eq!(shown.node_count(), 2);
        assert_eq!(shown.edge_count(), 1);
        assert_eq!(
            shown
                .edges()
                .next()
                .and_then(|edge| edge.class.clone())
                .as_deref(),
            Some("audio")
        );
    }

    fn applied_onto(mut shown: Graph<(), ()>, changes: ChangeSet<(), ()>) -> Graph<(), ()> {
        shown.apply(changes);
        shown
    }

    #[test]
    fn a_wire_takes_the_type_its_source_port_carries() {
        let device = patch("device", NodeBody::Device(Default::default()), 0.0);
        let places = HashMap::from([(device.id.clone(), node::places(&device, None))]);
        let graph = PatchGraph {
            nodes: vec![device, patch("scope", NodeBody::Scope, 400.0)],
            edges: vec![link("device", "scope")],
        };
        let drawn = wires(&graph, &places);
        assert_eq!(drawn.len(), 1);
        assert_eq!(drawn[0].class, "iq");
        assert_eq!(drawn[0].id, graph::edge_key(&graph.edges[0]));
    }
}
