use super::*;
use crate::{array::ArrayNode, device::Coherence};

fn lanes(coherence: Coherence) -> StateSnapshot {
    let mut radio = set(1, "band", Vec::new());
    radio.capabilities.rx_streams = 2;
    radio.capabilities.coherence = coherence;
    live(vec![radio])
}

fn arrays() -> PatchGraph {
    PatchGraph {
        nodes: vec![
            radio("dev", "band"),
            node("arr", NodeBody::Array(ArrayNode::default())),
            node("north", NodeBody::Array(ArrayNode::default())),
            node("scope", NodeBody::Scope),
        ],
        edges: vec![wire(("dev", "iq"), ("arr", "lane"))],
    }
}

fn verdicts(
    graph: &PatchGraph,
    from: (&str, &str),
    state: &StateSnapshot,
) -> Vec<(String, String, Option<String>)> {
    let from = PortRef {
        node: from.0.to_owned(),
        port: from.1.to_owned(),
    };
    let ctx = ResolveCtx {
        descriptors: None,
        live: Some(state),
    };
    wiring(graph, &from, &ctx)
        .into_iter()
        .map(|verdict| (verdict.to.node, verdict.to.port, verdict.refusal))
        .collect()
}

fn verdict<'a>(
    all: &'a [(String, String, Option<String>)],
    node: &str,
    port: &str,
) -> Option<&'a Option<String>> {
    all.iter()
        .find(|(to, input, _)| to == node && input == port)
        .map(|(_, _, refusal)| refusal)
}

#[test]
fn a_second_lane_of_a_radio_without_a_shared_clock_is_refused() {
    let loose = verdicts(&arrays(), ("dev", "iq2"), &lanes(Coherence::None));
    assert_eq!(
        verdict(&loose, "arr", "lane2"),
        Some(&Some("This radio's lanes share no clock".to_owned()))
    );
    assert_eq!(verdict(&loose, "north", "lane"), Some(&None));
    assert_eq!(verdict(&loose, "scope", "iq"), Some(&None));
    let synced = verdicts(&arrays(), ("dev", "iq2"), &lanes(Coherence::TimeSync));
    assert_eq!(verdict(&synced, "arr", "lane2"), Some(&None));
}

#[test]
fn a_lane_already_in_an_array_is_refused_elsewhere() {
    let taken = verdicts(&arrays(), ("dev", "iq"), &lanes(Coherence::TimeSync));
    assert_eq!(
        verdict(&taken, "north", "lane"),
        Some(&Some("That lane is in Array".to_owned()))
    );
    assert_eq!(verdict(&taken, "scope", "iq"), Some(&None));
    assert!(
        verdict(&taken, "dev", "iq").is_none(),
        "a node is never offered to itself"
    );
}

#[test]
fn verdicts_cover_only_inputs_of_the_dragged_type() {
    let all = verdicts(&arrays(), ("dev", "iq2"), &lanes(Coherence::TimeSync));
    assert!(all.iter().all(|(_, port, _)| port != "position"));
}

#[test]
fn lanes_of_one_radio_without_a_shared_clock_are_a_node_problem() {
    let mut graph = arrays();
    graph.edges.push(wire(("dev", "iq2"), ("arr", "lane2")));
    let state = lanes(Coherence::None);
    let (_, problems) = bound(&graph, &state);
    assert!(problems.iter().any(
        |problem| problem.node == "arr" && problem.text == "This radio's lanes share no clock"
    ));
    let (_, problems) = bound(&graph, &lanes(Coherence::PhaseCoherent));
    assert!(problems.iter().all(|problem| problem.node != "arr"));
}
