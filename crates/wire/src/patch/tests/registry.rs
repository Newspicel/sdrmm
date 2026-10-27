use super::*;

const OLD_FACES: &[(&str, Face)] = &[
    ("device", Face::fit(420).dial()),
    ("gps", Face::fit(360)),
    ("channel", Face::fit(440).dial()),
    ("event_output", Face::fit(420)),
    ("event_filter", Face::fit(380)),
    ("audio_fx", Face::fit(400)),
    ("scope", Face::sized(520, 360).min(320, 200)),
    ("baseband_scope", Face::sized(420, 340).min(320, 200)),
    ("speaker", Face::fit(320)),
    ("map", Face::sized(520, 380).min(300, 220)),
    ("signal_map", Face::sized(600, 440).min(400, 300)),
    ("propagation", Face::sized(640, 560).min(440, 380)),
    ("readout", Face::sized(560, 320).min(300, 160)),
    ("decoder_log", Face::sized(720, 380).min(360, 200)),
    ("dmr_trunk", Face::sized(480, 360).min(380, 240)),
    ("spectrum_monitor", Face::fit(320)),
    ("video", Face::sized(380, 320).min(240, 200)),
    ("recorder", Face::fit(340)),
    ("audio_recorder", Face::fit(340)),
    ("baseband_recorder", Face::fit(340)),
    ("network_export", Face::fit(380)),
    ("export", Face::fit(320)),
    ("scanner", Face::fit(400)),
    ("hunt", Face::fit(360)),
    ("satellite", Face::fit(420).dial()),
    ("rotator", Face::fit(340)),
    ("triangulation", Face::fit(400)),
    ("array", Face::fit(460)),
    ("df", Face::fit(400)),
    ("beamformer", Face::fit(400)),
    ("passive_radar", Face::sized(560, 540).min(420, 380)),
    ("spatial_spectrum", Face::sized(560, 420).min(400, 300)),
    ("correlator", Face::sized(520, 460).min(380, 340)),
    ("polarimeter", Face::fit(360)),
    ("stitch", Face::fit(360)),
    ("note", Face::sized(320, 200).min(200, 120)),
];

fn as_sent(body: &NodeBody) -> serde_json::Value {
    let text = serde_json::to_string(body).expect("json");
    serde_json::from_str(&text).expect("json")
}

#[test]
fn every_default_body_keeps_its_json() {
    let saved: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("default_bodies.json")).expect("fixture");
    let now: Vec<serde_json::Value> = NodeKind::ALL
        .iter()
        .map(|&kind| as_sent(&NodeBody::default_for(kind)))
        .collect();
    assert_eq!(now, saved);
    for body in saved {
        let parsed: NodeBody = serde_json::from_value(body.clone()).expect("parses");
        assert_eq!(as_sent(&parsed), body);
    }
}

#[test]
fn every_kind_names_itself_the_same_way_everywhere() {
    for &kind in NodeKind::ALL {
        let tag = serde_json::to_value(kind).expect("json");
        assert_eq!(tag, kind.as_str());
        assert_eq!(NodeKind::parse(kind.as_str()), Some(kind));
        assert_eq!(kind.to_string(), kind.as_str());
        assert_eq!(NodeBody::default_for(kind).kind(), kind);
        let body = serde_json::to_value(NodeBody::default_for(kind)).expect("json");
        assert_eq!(body["kind"], kind.as_str());
    }
    assert_eq!(NodeKind::parse("combiner"), None);
}

#[test]
fn faces_equal_the_old_canvas_table() {
    assert_eq!(OLD_FACES.len(), NodeKind::ALL.len());
    for &(kind, face) in OLD_FACES {
        let kind = NodeKind::parse(kind).expect("a kind");
        assert_eq!(kind.face(), face, "{kind}");
    }
}

#[test]
fn every_kind_has_a_role_and_a_face() {
    for &kind in NodeKind::ALL {
        let face = kind.face();
        assert!(face.width > 0, "{kind}");
        assert_eq!(face.resizable(), face.min.is_some(), "{kind}");
        let body = NodeBody::default_for(kind);
        assert_eq!(
            kind.role() == Role::Processor,
            body.processor_params().is_some(),
            "{kind}"
        );
        if let Role::Pass(through) = kind.role() {
            let carries = |direction| {
                kind.ports()
                    .iter()
                    .any(|port| port.direction == direction && port.port_type == through)
            };
            assert!(
                carries(PortDirection::In) && carries(PortDirection::Out),
                "{kind}"
            );
        }
    }
    let radios: Vec<NodeKind> = NodeKind::ALL
        .iter()
        .copied()
        .filter(|kind| kind.role() == Role::OpensRadio)
        .collect();
    assert_eq!(radios, [NodeKind::Device], "one node picks a radio");
    assert_eq!(NodeKind::EventFilter.role(), Role::Pass(PortType::Events));
    assert_eq!(NodeKind::AudioFx.role(), Role::Pass(PortType::Audio));
}

#[test]
fn the_catalog_serves_roles_faces_and_every_port_type() {
    let catalog = PatchCatalog::build();
    assert_eq!(catalog.nodes.len(), NodeKind::ALL.len());
    for (entry, &kind) in catalog.nodes.iter().zip(NodeKind::ALL) {
        assert_eq!(entry.kind, kind);
        assert_eq!(entry.role, kind.role());
        assert_eq!(entry.face, kind.face());
    }
    let served: Vec<PortType> = catalog
        .port_types
        .iter()
        .map(|info| info.port_type)
        .collect();
    assert_eq!(served, PortType::ALL);
    let json = serde_json::to_value(&catalog).expect("json");
    let filter = json["nodes"]
        .as_array()
        .and_then(|nodes| nodes.iter().find(|node| node["kind"] == "event_filter"))
        .expect("a filter");
    assert_eq!(
        filter["role"],
        serde_json::json!({"role": "pass", "of": "events"})
    );
    assert_eq!(
        filter["face"],
        serde_json::json!({"width": 380, "dial": false})
    );
    assert_eq!(json["port_types"][0]["hue"], "blue");
    assert!(json["port_types"][0]["note"].is_string());
}

#[test]
fn the_catalog_serves_every_event_kind() {
    let catalog = PatchCatalog::build();
    let served: Vec<crate::EventKind> =
        catalog.event_kinds.iter().map(|entry| entry.kind).collect();
    assert_eq!(served, crate::EventKind::ALL);
    let json = serde_json::to_value(&catalog).expect("json");
    let adsb = json["event_kinds"]
        .as_array()
        .and_then(|kinds| kinds.iter().find(|entry| entry["kind"] == "adsb"))
        .expect("adsb");
    assert_eq!(adsb["noun"], "Aircraft");
    assert_eq!(adsb["symbol"], "plane");
    assert_eq!(adsb["caps"], serde_json::json!(["log", "station", "geo"]));
}

#[test]
fn a_node_takes_events_on_one_input_at_most() {
    for kind in NodeKind::ALL {
        let events = kind
            .ports()
            .iter()
            .filter(|port| {
                port.direction == PortDirection::In && port.port_type == PortType::Events
            })
            .count();
        assert!(
            events <= 1,
            "{kind} has {events} events inputs, so a sink id would not name one input"
        );
    }
}
