use super::{
    ChannelDescriptor, NodeBody, PatchEdge, PatchError, PatchGraph, PortDirection, PortRef,
    PortSpec, PortType,
};

pub const REFUSAL_SELF: &str = "a node cannot wire to itself";
pub const REFUSAL_DIRECTION: &str = "wires run from an output to an input";
pub const REFUSAL_NO_PORT: &str = "that port does not exist";
pub const REFUSAL_WIRED: &str = "already wired";
pub const REFUSAL_BEAM_TAKES_ONE: &str = "a beam input takes one wire";
pub const REFUSAL_ONE_WIRE: &str = "that input takes one wire";
pub const REFUSAL_ONE_NODE: &str = "that output drives one node at a time";
pub const REFUSAL_LOOP: &str = "that wire would close a loop";

impl PatchGraph {
    #[must_use]
    pub fn connection_refusal(
        &self,
        edge: &PatchEdge,
        channels: Option<&[ChannelDescriptor]>,
    ) -> Option<String> {
        if edge.from.node == edge.to.node {
            return Some(REFUSAL_SELF.to_owned());
        }
        let (out, input) = match self.ends(edge, channels) {
            Ok(ends) => ends,
            Err(reason) => return Some(reason.to_owned()),
        };
        if out.port_type != input.port_type {
            return Some(input.note.clone().unwrap_or_else(|| {
                format!(
                    "{} cannot feed a {} input",
                    out.port_type.as_str(),
                    input.port_type.as_str()
                )
            }));
        }
        let mut tentative = self.clone();
        tentative.edges.push(edge.clone());
        if let Some(reason) = tentative.wiring_refusal(tentative.edges.len() - 1, &input) {
            return Some(reason);
        }
        self.crowding(edge, &out, &input)
            .or_else(|| tentative.check_acyclic().err().map(|_| REFUSAL_LOOP.to_owned()))
    }

    fn ends(
        &self,
        edge: &PatchEdge,
        channels: Option<&[ChannelDescriptor]>,
    ) -> Result<(PortSpec, PortSpec), &'static str> {
        let out = self.port(&edge.from, PortDirection::Out, channels);
        let input = self.port(&edge.to, PortDirection::In, channels);
        match (out, input) {
            (Ok(out), Ok(input)) => Ok((out, input)),
            (Err(PatchError::Direction(_)), _) | (_, Err(PatchError::Direction(_))) => {
                Err(REFUSAL_DIRECTION)
            }
            _ => Err(REFUSAL_NO_PORT),
        }
    }

    fn crowding(&self, edge: &PatchEdge, out: &PortSpec, input: &PortSpec) -> Option<String> {
        let landing: Vec<&PortRef> = self
            .edges
            .iter()
            .filter(|other| other.to == edge.to)
            .map(|other| &other.from)
            .collect();
        if landing.contains(&&edge.from) {
            return Some(REFUSAL_WIRED.to_owned());
        }
        if self.beam_crowded(edge, input, &landing) {
            return Some(REFUSAL_BEAM_TAKES_ONE.to_owned());
        }
        if !input.multi && !landing.is_empty() {
            return Some(REFUSAL_ONE_WIRE.to_owned());
        }
        let driving = self.edges.iter().any(|other| other.from == edge.from);
        (!out.multi && driving).then(|| REFUSAL_ONE_NODE.to_owned())
    }

    fn beam_crowded(&self, edge: &PatchEdge, input: &PortSpec, landing: &[&PortRef]) -> bool {
        let into_channel = self
            .node(&edge.to.node)
            .is_some_and(|node| matches!(node.body, NodeBody::Channel(_)));
        input.port_type == PortType::Iq
            && into_channel
            && !landing.is_empty()
            && std::iter::once(&edge.from)
                .chain(landing.iter().copied())
                .any(|source| self.is_lane_output(source))
    }

    fn is_lane_output(&self, source: &PortRef) -> bool {
        self.node(&source.node)
            .is_some_and(|node| node.body.lane_outputs().contains(&source.port.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::patch::{ChannelNode, DeviceNode, NodeBody, PatchNode, Position};

    fn node(id: &str, body: NodeBody) -> PatchNode {
        PatchNode {
            id: id.to_owned(),
            body,
            position: Position { x: 0.0, y: 0.0 },
            size: None,
            label: None,
        }
    }

    fn port(node: &str, port: &str) -> PortRef {
        PortRef {
            node: node.to_owned(),
            port: port.to_owned(),
        }
    }

    fn wire(from: (&str, &str), to: (&str, &str)) -> PatchEdge {
        PatchEdge {
            from: port(from.0, from.1),
            to: port(to.0, to.1),
        }
    }

    fn graph() -> PatchGraph {
        PatchGraph {
            nodes: vec![
                node("dev", NodeBody::Device(DeviceNode::default())),
                node("dev2", NodeBody::Device(DeviceNode::default())),
                node("scope", NodeBody::Scope),
                node("speaker", NodeBody::Speaker),
                node(
                    "ch",
                    NodeBody::Channel(ChannelNode {
                        channel_type: "nfm".to_owned(),
                        tuning_locked: false,
                    }),
                ),
            ],
            edges: vec![wire(("dev", "iq"), ("scope", "iq"))],
        }
    }

    fn refused(graph: &PatchGraph, from: (&str, &str), to: (&str, &str)) -> Option<String> {
        graph.connection_refusal(&wire(from, to), None)
    }

    #[test]
    fn a_fitting_wire_is_accepted() {
        assert_eq!(refused(&graph(), ("dev", "iq"), ("ch", "iq")), None);
    }

    #[test]
    fn each_refusal_says_why_in_a_few_words() {
        let graph = graph();
        assert_eq!(
            refused(&graph, ("dev", "iq"), ("dev", "iq")).as_deref(),
            Some(REFUSAL_SELF)
        );
        assert_eq!(
            refused(&graph, ("scope", "iq"), ("dev", "iq")).as_deref(),
            Some(REFUSAL_DIRECTION)
        );
        assert_eq!(
            refused(&graph, ("dev", "nope"), ("scope", "iq")).as_deref(),
            Some(REFUSAL_NO_PORT)
        );
        assert_eq!(
            refused(&graph, ("dev", "iq"), ("scope", "iq")).as_deref(),
            Some(REFUSAL_WIRED)
        );
        assert_eq!(
            refused(&graph, ("dev2", "iq"), ("scope", "iq")).as_deref(),
            Some(REFUSAL_ONE_WIRE)
        );
        assert!(
            refused(&graph, ("dev", "iq"), ("speaker", "audio"))
                .is_some_and(|reason| reason.contains("iq"))
        );
    }

    #[test]
    fn an_unrelated_broken_node_never_blocks_a_new_wire() {
        let mut graph = graph();
        graph.edges.push(wire(("dev2", "iq"), ("ghost", "iq")));
        assert_eq!(refused(&graph, ("dev", "iq"), ("ch", "iq")), None);
    }
}
