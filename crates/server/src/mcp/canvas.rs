use sdrmm_wire::{
    NodeCategory, NodeKind, PatchApplyReport, PatchEdge, PatchGraph, PatchNode, Position,
    StateScope, UpdateWorkspaceRequest, WorkspaceDetail, WorkspaceSnapshot,
};

use super::args::{PutNodeArgs, WireArgs};
use crate::{
    AppState,
    rest::{self, AppError},
};

const COLUMN: f32 = 520.0;
const NATURAL_NODE_H: f32 = 380.0;
const GAP: f32 = 120.0;
const ID_BYTES: usize = 4;

pub(super) fn active(state: &AppState) -> Result<WorkspaceDetail, AppError> {
    state
        .store
        .active_workspace()?
        .ok_or_else(|| AppError::bad_request("no workspace is open"))
}

pub(super) fn edit<T>(
    state: &AppState,
    change: impl FnOnce(&mut WorkspaceSnapshot) -> Result<T, AppError>,
) -> Result<(T, PatchApplyReport), AppError> {
    let _serialized = rest::lock_gate(&state.apply_gate);
    let WorkspaceDetail {
        info, mut snapshot, ..
    } = active(state)?;
    let before = snapshot.graph.clone();
    let outcome = change(&mut snapshot)?;
    let graph = &snapshot.graph;
    snapshot
        .rack
        .slots
        .retain(|slot| graph.node(&slot.node).is_some());
    snapshot
        .graph
        .validate_against(&state.engine.channel_types())
        .map_err(|err| AppError::bad_request(err.to_string()))?;
    let after = snapshot.graph.clone();
    let update = UpdateWorkspaceRequest {
        revision: info.revision,
        name: None,
        snapshot: Some(snapshot),
    };
    state
        .store
        .update_workspace(info.id, &update, Some(super::MCP_AUTHOR))?;
    state.engine.emit_scope(StateScope::Workspaces);
    let released = crate::apply::release(state, &before, &after);
    let mut report = crate::apply::bring_up_active(state, info.id)?;
    report.refused.extend(released);
    Ok((outcome, report))
}

pub(super) fn put_node(graph: &mut PatchGraph, args: PutNodeArgs) -> Result<String, AppError> {
    match args.node.clone() {
        Some(id) => {
            let node = graph
                .nodes
                .iter_mut()
                .find(|node| node.id == id)
                .ok_or_else(|| unknown(&id))?;
            update(node, args)
        }
        None => add(graph, args),
    }
}

pub(super) fn remove_node(graph: &mut PatchGraph, id: &str) -> Result<(), AppError> {
    if graph.node(id).is_none() {
        return Err(unknown(id));
    }
    graph.nodes.retain(|node| node.id != id);
    graph
        .edges
        .retain(|edge| edge.from.node != id && edge.to.node != id);
    Ok(())
}

pub(super) fn connect(graph: &mut PatchGraph, wire: WireArgs) {
    let edge = PatchEdge {
        from: wire.from,
        to: wire.to,
    };
    if !graph.edges.contains(&edge) {
        graph.edges.push(edge);
    }
}

pub(super) fn disconnect(graph: &mut PatchGraph, wire: WireArgs) -> Result<(), AppError> {
    let edge = PatchEdge {
        from: wire.from,
        to: wire.to,
    };
    let count = graph.edges.len();
    graph.edges.retain(|held| *held != edge);
    if graph.edges.len() == count {
        return Err(AppError::bad_request(format!(
            "no wire from {}.{} to {}.{}",
            edge.from.node, edge.from.port, edge.to.node, edge.to.port
        )));
    }
    Ok(())
}

pub(super) fn unknown(id: &str) -> AppError {
    AppError::bad_request(format!("no node {id:?} on the canvas"))
}

fn update(node: &mut PatchNode, args: PutNodeArgs) -> Result<String, AppError> {
    if let Some(body) = args.body {
        if body.kind() != node.body.kind() {
            return Err(AppError::bad_request(format!(
                "{} is a {} node; remove it and add a {} node",
                node.id,
                node.body.kind(),
                body.kind()
            )));
        }
        node.body = body;
    }
    if let Some(label) = args.label {
        node.label = Some(label).filter(|label| !label.is_empty());
    }
    if let Some(position) = args.position {
        node.position = position;
    }
    Ok(node.id.clone())
}

fn add(graph: &mut PatchGraph, args: PutNodeArgs) -> Result<String, AppError> {
    let body = args
        .body
        .ok_or_else(|| AppError::bad_request("a new node needs a body"))?;
    let id = fresh_id(graph, body.kind())?;
    let position = args
        .position
        .unwrap_or_else(|| place(graph, body.category()));
    graph.nodes.push(PatchNode {
        id: id.clone(),
        body,
        position,
        size: None,
        label: args.label.filter(|label| !label.is_empty()),
    });
    Ok(id)
}

fn fresh_id(graph: &PatchGraph, kind: NodeKind) -> Result<String, AppError> {
    loop {
        let mut bytes = [0u8; ID_BYTES];
        getrandom::fill(&mut bytes)
            .map_err(|err| AppError::internal(format!("no randomness for a node id: {err}")))?;
        let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let id = format!("{kind}:{hex}");
        if graph.node(&id).is_none() {
            return Ok(id);
        }
    }
}

fn place(graph: &PatchGraph, category: NodeCategory) -> Position {
    let x = column(category) * COLUMN;
    let y = graph
        .nodes
        .iter()
        .filter(|node| (node.position.x - x).abs() < COLUMN / 2.0)
        .map(|node| node.position.y + node.size.map_or(NATURAL_NODE_H, |size| size.h) + GAP)
        .fold(0.0, f32::max);
    Position { x, y }
}

const fn column(category: NodeCategory) -> f32 {
    match category {
        NodeCategory::Source => 0.0,
        NodeCategory::Tool => 1.0,
        NodeCategory::Channel => 2.0,
        NodeCategory::Output => 3.0,
    }
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::NodeBody;

    use super::*;

    fn graph() -> PatchGraph {
        WorkspaceSnapshot::starter().graph
    }

    fn adding(body: NodeBody) -> PutNodeArgs {
        PutNodeArgs {
            node: None,
            body: Some(body),
            label: None,
            position: None,
        }
    }

    #[test]
    fn a_new_node_gets_a_fresh_id_in_its_column() {
        let mut graph = graph();
        let id = put_node(&mut graph, adding(NodeBody::Readout)).expect("added");
        assert!(id.starts_with("readout:"), "{id}");
        let first = graph.node(&id).expect("on the canvas").position;
        assert!((first.x - 3.0 * COLUMN).abs() < f32::EPSILON);
        let again = put_node(&mut graph, adding(NodeBody::Readout)).expect("added");
        assert_ne!(again, id);
        assert!(graph.node(&again).expect("placed").position.y > first.y);
    }

    #[test]
    fn a_node_keeps_its_kind() {
        let mut graph = graph();
        let refused = put_node(
            &mut graph,
            PutNodeArgs {
                node: Some("scope".to_owned()),
                ..adding(NodeBody::Speaker)
            },
        );
        assert!(refused.is_err());
        let relabelled = put_node(
            &mut graph,
            PutNodeArgs {
                node: Some("scope".to_owned()),
                body: None,
                label: Some("Wide".to_owned()),
                position: None,
            },
        );
        assert_eq!(relabelled.expect("relabelled"), "scope");
        assert_eq!(
            graph.node("scope").and_then(|n| n.label.as_deref()),
            Some("Wide")
        );
    }

    #[test]
    fn an_unknown_node_is_refused_rather_than_added() {
        let mut graph = graph();
        let missing = PutNodeArgs {
            node: Some("nope".to_owned()),
            ..adding(NodeBody::Speaker)
        };
        assert!(put_node(&mut graph, missing).is_err());
        assert!(remove_node(&mut graph, "nope").is_err());
    }

    #[test]
    fn removing_a_node_takes_its_wires() {
        let mut graph = graph();
        remove_node(&mut graph, "scope").expect("removed");
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn a_wire_is_drawn_once_and_cut_once() {
        let mut graph = graph();
        let wire = || WireArgs {
            from: sdrmm_wire::PortRef {
                node: "device".to_owned(),
                port: "iq".to_owned(),
            },
            to: sdrmm_wire::PortRef {
                node: "scope".to_owned(),
                port: "iq".to_owned(),
            },
        };
        connect(&mut graph, wire());
        assert_eq!(graph.edges.len(), 1);
        disconnect(&mut graph, wire()).expect("cut");
        assert!(disconnect(&mut graph, wire()).is_err());
    }
}
