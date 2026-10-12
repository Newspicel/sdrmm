use std::collections::HashSet;

use sdrmm_wire::patch::{PatchEdge, PatchGraph, PatchNode};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphPatch {
    pub upserted: Vec<PatchNode>,
    pub removed: Vec<String>,
    pub wired: Vec<PatchEdge>,
    pub unwired: Vec<PatchEdge>,
}

impl GraphPatch {
    #[must_use]
    pub fn between(before: &PatchGraph, after: &PatchGraph) -> Self {
        let upserted = after
            .nodes
            .iter()
            .filter(|node| before.node(&node.id) != Some(*node))
            .cloned()
            .collect();
        let kept: HashSet<&str> = after.nodes.iter().map(|node| node.id.as_str()).collect();
        let removed = before
            .nodes
            .iter()
            .filter(|node| !kept.contains(node.id.as_str()))
            .map(|node| node.id.clone())
            .collect();
        Self {
            upserted,
            removed,
            wired: missing_from(&after.edges, &before.edges),
            unwired: missing_from(&before.edges, &after.edges),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.upserted.is_empty()
            && self.removed.is_empty()
            && self.wired.is_empty()
            && self.unwired.is_empty()
    }

    #[must_use]
    pub fn onto(&self, theirs: &PatchGraph) -> PatchGraph {
        let mut graph = theirs.clone();
        graph.nodes.retain(|node| !self.removed.contains(&node.id));
        for node in &self.upserted {
            match graph.nodes.iter_mut().find(|held| held.id == node.id) {
                Some(held) => held.clone_from(node),
                None => graph.nodes.push(node.clone()),
            }
        }
        graph.edges.retain(|edge| !self.unwired.contains(edge));
        for edge in &self.wired {
            if !graph.edges.contains(edge) {
                graph.edges.push(edge.clone());
            }
        }
        let present: HashSet<String> = graph.nodes.iter().map(|node| node.id.clone()).collect();
        graph
            .edges
            .retain(|edge| present.contains(&edge.from.node) && present.contains(&edge.to.node));
        graph
    }

    pub fn then(&mut self, next: Self) {
        self.upserted
            .retain(|node| !next.removed.contains(&node.id));
        for node in next.upserted {
            self.removed.retain(|id| *id != node.id);
            match self.upserted.iter_mut().find(|held| held.id == node.id) {
                Some(held) => *held = node,
                None => self.upserted.push(node),
            }
        }
        self.removed.extend(next.removed);
        self.wired.retain(|edge| !next.unwired.contains(edge));
        self.unwired.retain(|edge| !next.wired.contains(edge));
        self.wired.extend(next.wired);
        self.unwired.extend(next.unwired);
    }
}

fn missing_from(edges: &[PatchEdge], other: &[PatchEdge]) -> Vec<PatchEdge> {
    edges
        .iter()
        .filter(|edge| !other.contains(edge))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::patch::{NodeBody, PatchEdge, PatchNode, PortRef, Position};

    use super::*;

    fn device(id: &str, x: f32) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body: NodeBody::Scope,
            position: Position { x, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn channel(id: &str) -> PatchNode {
        device(id, -1.0)
    }

    fn wire(from: &str, to: &str) -> PatchEdge {
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

    fn graph(nodes: Vec<PatchNode>, edges: Vec<PatchEdge>) -> PatchGraph {
        PatchGraph { nodes, edges }
    }

    #[test]
    fn an_unchanged_graph_makes_an_empty_patch() {
        let base = graph(vec![device("dev", 0.0)], Vec::new());
        assert!(GraphPatch::between(&base, &base).is_empty());
    }

    #[test]
    fn my_move_lands_on_their_new_node() {
        let base = graph(vec![device("dev", 0.0)], Vec::new());
        let mine = graph(vec![device("dev", 40.0)], Vec::new());
        let theirs = graph(vec![device("dev", 0.0), channel("ch")], Vec::new());
        let merged = GraphPatch::between(&base, &mine).onto(&theirs);
        assert_eq!(merged.nodes, vec![device("dev", 40.0), channel("ch")]);
    }

    #[test]
    fn my_wire_and_removal_replay_over_their_copy() {
        let base = graph(
            vec![device("dev", 0.0), channel("a"), channel("b")],
            Vec::new(),
        );
        let mine = graph(
            vec![device("dev", 0.0), channel("a")],
            vec![wire("dev", "a")],
        );
        let theirs = graph(
            vec![device("dev", 0.0), channel("a"), channel("b"), channel("c")],
            vec![wire("dev", "b")],
        );
        let merged = GraphPatch::between(&base, &mine).onto(&theirs);
        let ids: Vec<&str> = merged.nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(ids, ["dev", "a", "c"]);
        assert_eq!(merged.edges, vec![wire("dev", "a")]);
    }

    #[test]
    fn a_wire_to_a_node_they_removed_is_dropped() {
        let base = graph(vec![device("dev", 0.0), channel("a")], Vec::new());
        let mine = graph(
            vec![device("dev", 0.0), channel("a")],
            vec![wire("dev", "a")],
        );
        let theirs = graph(vec![device("dev", 0.0)], Vec::new());
        let merged = GraphPatch::between(&base, &mine).onto(&theirs);
        assert!(merged.edges.is_empty());
    }

    #[test]
    fn chained_patches_equal_one_patch_over_both_edits() {
        let first = graph(vec![device("dev", 0.0)], Vec::new());
        let second = graph(
            vec![device("dev", 10.0), channel("a")],
            vec![wire("dev", "a")],
        );
        let third = graph(vec![device("dev", 20.0)], Vec::new());
        let mut chained = GraphPatch::between(&first, &second);
        chained.then(GraphPatch::between(&second, &third));
        let theirs = graph(vec![device("dev", 0.0), channel("z")], Vec::new());
        assert_eq!(
            chained.onto(&theirs),
            GraphPatch::between(&first, &third).onto(&theirs)
        );
    }

    #[test]
    fn a_node_removed_then_added_again_survives_the_chain() {
        let first = graph(vec![device("dev", 0.0), channel("a")], Vec::new());
        let second = graph(vec![device("dev", 0.0)], Vec::new());
        let third = graph(vec![device("dev", 0.0), channel("a")], Vec::new());
        let mut chained = GraphPatch::between(&first, &second);
        chained.then(GraphPatch::between(&second, &third));
        assert!(chained.onto(&first).node("a").is_some());
    }
}
