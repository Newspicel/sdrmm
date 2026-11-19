use super::*;
use crate::{Duplex, RadioUse, TRANSMIT_PORT, transmit::TransmitNode};

fn used_for(id: &str, radio_use: RadioUse) -> PatchNode {
    let mut found = radio(id, "band");
    if let NodeBody::Device(device) = &mut found.body {
        device.radio_use = radio_use;
    }
    found
}

fn transmitter() -> PatchNode {
    node("tx", NodeBody::Transmit(TransmitNode::default()))
}

fn port_names(patch: &ResolvedPatch, id: &str) -> Vec<String> {
    patch.node(id).map_or_default(|found| {
        found
            .ports
            .iter()
            .map(|port| port.spec.name.clone())
            .collect()
    })
}

fn lone(radio_use: RadioUse) -> Vec<String> {
    let graph = PatchGraph {
        nodes: vec![used_for("dev", radio_use)],
        edges: Vec::new(),
    };
    port_names(&compiled(&graph), "dev")
}

fn two_lanes(duplex: Duplex) -> StateSnapshot {
    let mut radio = set(1, "band", Vec::new());
    radio.capabilities.rx_streams = 2;
    radio.capabilities.duplex = duplex;
    live(vec![radio])
}

#[test]
fn each_use_offers_only_its_own_side() {
    assert_eq!(lone(RadioUse::Receive), ["iq", CONTROL_PORT]);
    assert_eq!(lone(RadioUse::Send), [CONTROL_PORT, TRANSMIT_PORT]);
    assert_eq!(lone(RadioUse::Both), ["iq", CONTROL_PORT, TRANSMIT_PORT]);
}

#[test]
fn a_receive_only_radio_never_offers_a_transmit_input() {
    let graph = PatchGraph {
        nodes: vec![used_for("dev", RadioUse::Both)],
        edges: Vec::new(),
    };
    let (patch, _) = bound(&graph, &two_lanes(Duplex::RxOnly));
    assert_eq!(port_names(&patch, "dev"), ["iq", "iq2", CONTROL_PORT]);
    let (patch, _) = bound(&graph, &two_lanes(Duplex::Half));
    assert_eq!(
        port_names(&patch, "dev"),
        ["iq", "iq2", CONTROL_PORT, TRANSMIT_PORT]
    );
}

#[test]
fn a_send_only_radio_offers_no_lanes() {
    let graph = PatchGraph {
        nodes: vec![used_for("dev", RadioUse::Receive)],
        edges: Vec::new(),
    };
    let (patch, _) = bound(&graph, &two_lanes(Duplex::TxOnly));
    assert_eq!(port_names(&patch, "dev"), [CONTROL_PORT, TRANSMIT_PORT]);
}

#[test]
fn a_transmitter_finds_no_input_on_a_receiving_radio() {
    let graph = PatchGraph {
        nodes: vec![
            used_for("rx", RadioUse::Receive),
            used_for("both", RadioUse::Both),
            transmitter(),
        ],
        edges: Vec::new(),
    };
    let from = PortRef {
        node: "tx".to_owned(),
        port: TRANSMIT_PORT.to_owned(),
    };
    let targets: Vec<String> = wiring(&graph, &from, &ResolveCtx::default())
        .into_iter()
        .map(|verdict| verdict.to.node)
        .collect();
    assert_eq!(targets, ["both"]);
}

#[test]
fn a_stale_wire_keeps_its_port_drawn_and_nothing_else() {
    let graph = PatchGraph {
        nodes: vec![
            used_for("dev", RadioUse::Send),
            channel("voice", "probe_voice"),
            used_for("quiet", RadioUse::Receive),
            transmitter(),
        ],
        edges: vec![
            wire(("dev", "iq2"), ("voice", "iq")),
            wire(("tx", TRANSMIT_PORT), ("quiet", TRANSMIT_PORT)),
        ],
    };
    let all = descriptors();
    let ctx = ResolveCtx {
        descriptors: Some(&all),
        live: None,
    };
    let patch = compile(&graph, &ctx).expect("a stale wire still compiles");
    assert_eq!(
        port_names(&patch, "dev"),
        ["iq2", CONTROL_PORT, TRANSMIT_PORT]
    );
    assert_eq!(
        port_names(&patch, "quiet"),
        ["iq", CONTROL_PORT, TRANSMIT_PORT]
    );
    assert_eq!(patch.edges.len(), 2, "both wires stay drawn");
}

#[test]
fn a_stale_lane_wire_on_a_sending_radio_carries_nothing() {
    let graph = PatchGraph {
        nodes: vec![
            used_for("dev", RadioUse::Send),
            channel("voice", "probe_voice"),
        ],
        edges: vec![wire(("dev", "iq"), ("voice", "iq"))],
    };
    let mut sending = set(1, "band", Vec::new());
    sending.capabilities.duplex = Duplex::Half;
    sending.radio_use = RadioUse::Send;
    let (patch, _) = bound(&graph, &live(vec![sending]));
    assert_eq!(
        port_names(&patch, "dev"),
        ["iq", CONTROL_PORT, TRANSMIT_PORT]
    );
    assert_eq!(
        patch.runs("voice"),
        Some(&Runs::Unbound {
            reason: UnboundReason::SendOnly
        })
    );
}
