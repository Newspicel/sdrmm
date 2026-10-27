use rusqlite::params;
use sdrmm_wire::{EventKind, NodeBody, WorkspaceNoticeKind, WorkspaceSnapshot};
use serde_json::{Value, json};

use super::*;
use crate::store::{Store, parse_workspace_snapshot};

fn with_filter(kinds: Value) -> Value {
    let mut snapshot =
        serde_json::to_value(WorkspaceSnapshot::starter()).expect("the starter encodes");
    let Some(nodes) = snapshot["graph"]["nodes"].as_array_mut() else {
        panic!("the starter has nodes");
    };
    nodes.push(json!({
        "id": "only", "kind": "event_filter", "position": { "x": 0.0, "y": 0.0 },
        "data": { "mode": "keep", "kinds": kinds }
    }));
    snapshot
}

fn dropped(kinds: &[&str]) -> WorkspaceNoticeKind {
    WorkspaceNoticeKind::DroppedFilterKinds {
        node: "only".to_owned(),
        kinds: kinds.iter().map(|kind| (*kind).to_owned()).collect(),
    }
}

fn filter_kinds(snapshot: &WorkspaceSnapshot) -> Vec<EventKind> {
    snapshot
        .graph
        .nodes
        .iter()
        .find_map(|node| match &node.body {
            NodeBody::EventFilter(filter) => Some(filter.kinds.clone()),
            _ => None,
        })
        .expect("a filter")
}

#[test]
fn unknown_filter_kinds_are_dropped_with_one_notice() {
    let mut snapshot = with_filter(json!(["adsb", "teletext", "call", 7]));
    let notices = run(&mut snapshot);
    assert_eq!(notices, vec![dropped(&["teletext", "7"])]);
    assert_eq!(
        snapshot["graph"]["nodes"]
            .as_array()
            .and_then(|nodes| nodes.last())
            .map(|node| node["data"]["kinds"].clone()),
        Some(json!(["adsb", "call"]))
    );
}

#[test]
fn running_the_steps_twice_equals_once() {
    let mut once = with_filter(json!(["teletext", "rtty"]));
    run(&mut once);
    let mut twice = once.clone();
    assert!(run(&mut twice).is_empty());
    assert_eq!(twice, once);
}

#[test]
fn a_parsed_snapshot_reads_without_unknown_kinds() {
    let json = with_filter(json!(["teletext", "rtty"])).to_string();
    let snapshot = parse_workspace_snapshot(&json).expect("parses");
    assert_eq!(filter_kinds(&snapshot), vec![EventKind::Rtty]);
}

#[test]
fn starters_and_templates_pass_without_a_notice() {
    let quiet = |snapshot: &mut Value| {
        run(snapshot).is_empty() && ONCE.iter().all(|(_, step)| step(snapshot).is_empty())
    };
    let mut starter =
        serde_json::to_value(WorkspaceSnapshot::starter()).expect("the starter encodes");
    assert!(quiet(&mut starter));
    let noisy: Vec<String> = crate::templates::all()
        .iter()
        .filter(|template| !quiet(&mut json!({ "graph": template.patch })))
        .map(|template| template.id.clone())
        .collect();
    assert!(noisy.is_empty(), "{noisy:?}");
}

#[test]
fn a_stored_workspace_is_cleaned_once_at_startup() {
    let file = tempfile::NamedTempFile::new().expect("temp db");
    let id = {
        let store = Store::open(Some(file.path())).expect("open");
        let id = store
            .active_workspace_id()
            .expect("active")
            .expect("seeded");
        store
            .lock()
            .execute(
                "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
                params![id, with_filter(json!(["teletext", "ais"])).to_string()],
            )
            .expect("plant the old filter");
        id
    };
    let store = Store::open(Some(file.path())).expect("reopen");
    let detail = store.workspace(id).expect("opens");
    assert_eq!(filter_kinds(&detail.snapshot), vec![EventKind::Ais]);
    let notices: Vec<&WorkspaceNoticeKind> =
        detail.notices.iter().map(|notice| &notice.notice).collect();
    assert_eq!(notices, vec![&dropped(&["teletext"])]);
    drop(store);
    let again = Store::open(Some(file.path())).expect("reopen again");
    assert_eq!(again.workspace(id).expect("opens").notices.len(), 1);
}

#[test]
fn an_import_reports_the_kinds_it_dropped() {
    let mut export = json!({
        "version": 1,
        "name": "Old",
        "snapshot": with_filter(json!(["teletext"])),
    });
    let notices = upgrade_import(&mut export);
    assert_eq!(notices, vec![dropped(&["teletext"])]);
    assert_eq!(
        export["snapshot"]["graph"]["nodes"]
            .as_array()
            .and_then(|nodes| nodes.last())
            .map(|node| node["data"]["kinds"].clone()),
        Some(json!([]))
    );
}

fn with_wires(nodes: Value, edges: Value) -> Value {
    json!({
        "version": sdrmm_wire::WORKSPACE_SNAPSHOT_VERSION,
        "graph": { "nodes": nodes, "edges": edges }
    })
}

fn at(id: &str, kind: &str, data: Value) -> Value {
    json!({ "id": id, "kind": kind, "position": { "x": 0.0, "y": 0.0 }, "data": data })
}

fn decoder(id: &str, channel_type: &str) -> Value {
    at(id, "channel", json!({ "channel_type": channel_type }))
}

fn wire(from: (&str, &str), to: (&str, &str)) -> Value {
    json!({ "from": { "node": from.0, "port": from.1 }, "to": { "node": to.0, "port": to.1 } })
}

fn notice_wire(from: (&str, &str), to: (&str, &str), reason: &str) -> sdrmm_wire::NoticeWire {
    let port = |end: (&str, &str)| sdrmm_wire::PortRef {
        node: end.0.to_owned(),
        port: end.1.to_owned(),
    };
    sdrmm_wire::NoticeWire {
        from: port(from),
        to: port(to),
        reason: reason.to_owned(),
    }
}

fn twice_equals_once(mut snapshot: Value) -> (Value, Vec<WorkspaceNoticeKind>) {
    let notices = run(&mut snapshot);
    let mut again = snapshot.clone();
    assert!(run(&mut again).is_empty(), "a second run changes nothing");
    assert_eq!(again, snapshot);
    (snapshot, notices)
}

#[test]
fn a_steer_wire_moves_to_the_bearing_port() {
    let old = with_wires(
        json!([at("df", "df", json!({})), at("bf", "beamformer", json!({}))]),
        json!([wire(("df", "events"), ("bf", "steer"))]),
    );
    let (snapshot, notices) = twice_equals_once(old);
    assert!(notices.is_empty());
    assert_eq!(
        snapshot["graph"]["edges"],
        json!([wire(("df", "bearing"), ("bf", "steer"))])
    );
}

#[test]
fn wires_the_contracts_refuse_are_dropped_with_their_reason() {
    let old = with_wires(
        json!([
            decoder("pager", "pocsag"),
            decoder("text", "rtty"),
            decoder("planes", "adsb"),
            json!({ "id": "map", "kind": "map", "position": { "x": 0.0, "y": 0.0 } }),
            at("only", "event_filter", json!({})),
            at("radar", "passive_radar", json!({})),
        ]),
        json!([
            wire(("pager", "events"), ("map", "events")),
            wire(("planes", "events"), ("map", "events")),
            wire(("only", "events"), ("radar", "adsb")),
            wire(("text", "events"), ("only", "events")),
        ]),
    );
    let (snapshot, notices) = twice_equals_once(old);
    assert_eq!(
        notices,
        vec![WorkspaceNoticeKind::DroppedWires {
            wires: vec![
                notice_wire(
                    ("pager", "events"),
                    ("map", "events"),
                    "Map takes positions"
                ),
                notice_wire(
                    ("text", "events"),
                    ("only", "events"),
                    "Passive radar takes ADS-B"
                ),
            ]
        }]
    );
    assert_eq!(
        snapshot["graph"]["edges"],
        json!([
            wire(("planes", "events"), ("map", "events")),
            wire(("only", "events"), ("radar", "adsb")),
        ])
    );
    parse_workspace_snapshot(&snapshot.to_string()).expect("the cleaned workspace loads");
}

#[test]
fn wires_into_a_radio_transmit_input_are_dropped() {
    let old = with_wires(
        json!([
            at("dev", "device", json!({})),
            at("dev2", "device", json!({}))
        ]),
        json!([wire(("dev", "iq"), ("dev2", "tx2"))]),
    );
    let (snapshot, notices) = twice_equals_once(old);
    assert_eq!(
        notices,
        vec![WorkspaceNoticeKind::DroppedWires {
            wires: vec![notice_wire(
                ("dev", "iq"),
                ("dev2", "tx2"),
                "Radios take no transmit wire"
            )]
        }]
    );
    assert_eq!(snapshot["graph"]["edges"], json!([]));
}

fn watching() -> Value {
    with_wires(
        json!([
            at("dev", "device", json!({})),
            decoder("voice", "nfm"),
            decoder("finder", "nfm"),
            at("scan", "scanner", json!({})),
            at("walk", "hunt", json!({})),
        ]),
        json!([
            wire(("dev", "iq2"), ("voice", "iq")),
            wire(("dev", "iq"), ("finder", "iq")),
            wire(("scan", "control"), ("voice", "control")),
            wire(("walk", "control"), ("finder", "control")),
        ]),
    )
}

fn edge_count(snapshot: &Value) -> usize {
    snapshot["graph"]["edges"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default()
}

#[test]
fn a_scanner_and_a_hunt_get_the_lane_their_decoder_runs_on() {
    let mut snapshot = watching();
    let notices = watched_lanes::wire_watched_lanes(&mut snapshot);
    assert_eq!(
        notices,
        vec![WorkspaceNoticeKind::AddedWires {
            wires: vec![
                notice_wire(("dev", "iq2"), ("scan", "iq"), "The lane it watches"),
                notice_wire(("dev", "iq"), ("walk", "iq"), "The lane it watches"),
            ]
        }]
    );
    let mut again = snapshot.clone();
    assert!(watched_lanes::wire_watched_lanes(&mut again).is_empty());
    assert_eq!(again, snapshot);
    assert_eq!(edge_count(&snapshot), 6);
    parse_workspace_snapshot(&snapshot.to_string()).expect("the wired workspace loads");
}

#[test]
fn a_lane_wire_the_user_removed_stays_removed() {
    let snapshot = watching();
    let parsed = parse_workspace_snapshot(&snapshot.to_string()).expect("loads");
    assert_eq!(parsed.graph.edges.len(), 4);
    let mut imported = json!({ "version": 1, "name": "Old", "snapshot": watching() });
    upgrade_import(&mut imported);
    assert_eq!(edge_count(&imported["snapshot"]), 6);
}

#[test]
fn stored_scanners_get_their_lane_once() {
    let file = tempfile::NamedTempFile::new().expect("temp db");
    let plant = |store: &Store, id: i64| {
        store
            .lock()
            .execute(
                "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
                params![id, watching().to_string()],
            )
            .expect("plant the old workspace");
    };
    let id = {
        let store = Store::open(Some(file.path())).expect("open");
        let id = store
            .active_workspace_id()
            .expect("active")
            .expect("seeded");
        store
            .lock()
            .execute("DELETE FROM server_meta WHERE key LIKE 'step:%'", [])
            .expect("forget the step");
        plant(&store, id);
        id
    };
    let store = Store::open(Some(file.path())).expect("reopen");
    assert_eq!(
        store
            .workspace(id)
            .expect("opens")
            .snapshot
            .graph
            .edges
            .len(),
        6
    );
    plant(&store, id);
    drop(store);
    let again = Store::open(Some(file.path())).expect("reopen again");
    let detail = again.workspace(id).expect("opens");
    assert_eq!(detail.snapshot.graph.edges.len(), 4);
    assert_eq!(detail.notices.len(), 1);
}

#[test]
fn an_old_stored_workspace_reports_its_refused_wires_at_startup() {
    let old = with_wires(
        json!([
            decoder("pager", "pocsag"),
            json!({ "id": "map", "kind": "map", "position": { "x": 0.0, "y": 0.0 } }),
            json!({ "id": "log", "kind": "decoder_log", "position": { "x": 0.0, "y": 0.0 } }),
        ]),
        json!([
            wire(("pager", "events"), ("map", "events")),
            wire(("pager", "events"), ("log", "events")),
        ]),
    );
    let file = tempfile::NamedTempFile::new().expect("temp db");
    let id = {
        let store = Store::open(Some(file.path())).expect("open");
        let id = store
            .active_workspace_id()
            .expect("active")
            .expect("seeded");
        store
            .lock()
            .execute(
                "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
                params![id, old.to_string()],
            )
            .expect("plant the old workspace");
        id
    };
    let store = Store::open(Some(file.path())).expect("reopen");
    let detail = store.workspace(id).expect("opens");
    let notices: Vec<&WorkspaceNoticeKind> =
        detail.notices.iter().map(|notice| &notice.notice).collect();
    assert_eq!(
        notices,
        vec![&WorkspaceNoticeKind::DroppedWires {
            wires: vec![notice_wire(
                ("pager", "events"),
                ("map", "events"),
                "Map takes positions"
            )]
        }]
    );
    assert_eq!(detail.snapshot.graph.edges.len(), 1);
}

fn calling() -> Value {
    with_wires(
        json!([
            decoder("dmr", "dmr"),
            at("trunk", "dmr_trunk", json!({})),
            at("loud", "spectrum_monitor", json!({ "record_audio": true })),
            at("log", "decoder_log", json!({})),
            at(
                "chat",
                "event_output",
                json!({ "target": { "service": "webhook", "format": "discord", "url": "" } }),
            ),
            json!({ "id": "spk", "kind": "speaker", "position": { "x": 0.0, "y": 0.0 } }),
        ]),
        json!([
            wire(("dmr", "events"), ("log", "events")),
            wire(("dmr", "audio"), ("log", "audio")),
            wire(("loud", "audio"), ("log", "audio")),
            wire(("trunk", "audio"), ("chat", "audio")),
            wire(("loud", "events"), ("log", "events")),
            wire(("dmr", "audio"), ("spk", "audio")),
            wire(("trunk", "audio"), ("spk", "audio")),
        ]),
    )
}

const RIDES: &str = "Clips ride on the events wire";

#[test]
fn call_audio_wires_are_dropped_because_clips_ride_on_events() {
    let (snapshot, notices) = twice_equals_once(calling());
    assert_eq!(
        notices,
        vec![WorkspaceNoticeKind::DroppedWires {
            wires: vec![
                notice_wire(("dmr", "audio"), ("log", "audio"), RIDES),
                notice_wire(("loud", "audio"), ("log", "audio"), RIDES),
                notice_wire(("trunk", "audio"), ("chat", "audio"), RIDES),
            ]
        }]
    );
    assert!(
        snapshot["graph"]["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .all(|node| node["data"].get("record_audio").is_none()),
        "the audio switch is gone"
    );
    assert_eq!(edge_count(&snapshot), 4, "live audio to the speaker stays");
    parse_workspace_snapshot(&snapshot.to_string()).expect("the workspace loads");
}

#[test]
fn any_import_drops_call_audio_wires() {
    for version in [1, sdrmm_wire::WORKSPACE_EXPORT_VERSION] {
        let mut imported = json!({ "version": version, "name": "Old", "snapshot": calling() });
        let notices = upgrade_import(&mut imported);
        assert!(notices.iter().any(|notice| matches!(
            notice,
            WorkspaceNoticeKind::DroppedWires { wires } if wires.iter().all(|wire| wire.reason == RIDES)
        )));
        assert_eq!(edge_count(&imported["snapshot"]), 4);
    }
}

#[test]
fn wires_from_a_decoder_that_sends_no_events_are_dropped() {
    let (snapshot, notices) = twice_equals_once(with_wires(
        json!([
            decoder("tv", "atv"),
            decoder("fm", "wfm"),
            at("log", "decoder_log", json!({})),
            json!({ "id": "screen", "kind": "video", "position": { "x": 0.0, "y": 0.0 } }),
        ]),
        json!([
            wire(("tv", "events"), ("log", "events")),
            wire(("fm", "events"), ("log", "events")),
            wire(("tv", "video"), ("screen", "video")),
        ]),
    ));
    assert_eq!(
        notices,
        vec![WorkspaceNoticeKind::DroppedWires {
            wires: vec![notice_wire(
                ("tv", "events"),
                ("log", "events"),
                "Sends no events"
            )]
        }]
    );
    assert_eq!(edge_count(&snapshot), 2);
}

fn picturing() -> Value {
    with_wires(
        json!([
            decoder("sstv", "sstv"),
            decoder("seen", "sstv"),
            at("readout", "readout", Value::Null),
            json!({ "id": "screen", "kind": "video", "position": { "x": 0.0, "y": 0.0 } }),
        ]),
        json!([
            wire(("sstv", "events"), ("readout", "events")),
            wire(("seen", "events"), ("readout", "events")),
            wire(("seen", "video"), ("screen", "video")),
        ]),
    )
}

#[test]
fn a_picture_channel_shown_in_a_readout_gets_one_video_node_once() {
    let mut snapshot = picturing();
    let notices = pictures::wire_pictures(&mut snapshot);
    assert_eq!(
        notices,
        vec![
            WorkspaceNoticeKind::AddedNodes {
                nodes: vec![sdrmm_wire::DroppedNode {
                    id: "sstv-pictures".to_owned(),
                    kind: "video".to_owned(),
                    label: None,
                }]
            },
            WorkspaceNoticeKind::AddedWires {
                wires: vec![notice_wire(
                    ("sstv", "video"),
                    ("sstv-pictures", "video"),
                    "Keeps its pictures"
                )]
            },
        ]
    );
    let mut again = snapshot.clone();
    assert!(pictures::wire_pictures(&mut again).is_empty());
    assert_eq!(again, snapshot);
    let parsed = parse_workspace_snapshot(&snapshot.to_string()).expect("loads");
    assert_eq!(
        parsed
            .graph
            .nodes
            .iter()
            .filter(|node| node.body.kind() == sdrmm_wire::NodeKind::Video)
            .count(),
        2
    );
}

fn bank(id: &str) -> Value {
    at(
        id,
        "device",
        json!({ "device": { "backend": "kraken", "serial": "01" } }),
    )
}

fn dongle(key: &str) -> Value {
    json!({ "backend": "rtlsdr", "key": key })
}

fn banked() -> Value {
    let mut edges: Vec<Value> = (0..5)
        .map(|lane| {
            wire(
                ("kraken", &sdrmm_wire::stream_port("iq", lane)),
                ("arr", &sdrmm_wire::stream_port("lane", lane)),
            )
        })
        .collect();
    edges.push(wire(("gps", "position"), ("kraken", "position")));
    with_wires(
        json!([
            bank("kraken"),
            at("arr", "array", json!({})),
            at(
                "gps",
                "gps",
                json!({ "source": { "type": "fixed", "lat": 48.1, "lon": 11.5 } })
            ),
        ]),
        json!(edges),
    )
}

fn lane_keys(snapshot: &Value) -> Vec<Value> {
    snapshot["graph"]["nodes"]
        .as_array()
        .map(|nodes| {
            nodes
                .iter()
                .filter(|node| node["kind"] == json!("device"))
                .map(|node| node["data"]["device"].clone())
                .collect()
        })
        .unwrap_or_default()
}

fn array_lanes(snapshot: &Value, array: &str) -> Vec<(String, String)> {
    snapshot["graph"]["edges"]
        .as_array()
        .map(|edges| {
            edges
                .iter()
                .filter(|edge| edge["to"]["node"] == json!(array))
                .filter_map(|edge| {
                    Some((
                        edge["from"]["node"].as_str()?.to_owned(),
                        edge["to"]["port"].as_str()?.to_owned(),
                    ))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn kinds_added(notices: &[WorkspaceNoticeKind]) -> Vec<String> {
    notices
        .iter()
        .flat_map(|notice| match notice {
            WorkspaceNoticeKind::AddedNodes { nodes } => {
                nodes.iter().map(|node| node.id.clone()).collect()
            }
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn a_stored_bank_becomes_five_dongles_on_the_array_it_already_had() {
    let mut snapshot = banked();
    let notices = banks::split_kraken_banks(&mut snapshot);
    assert_eq!(
        kinds_added(&notices),
        vec![
            "kraken-lane2",
            "kraken-lane3",
            "kraken-lane4",
            "kraken-lane5"
        ]
    );
    assert!(
        !notices
            .iter()
            .any(|notice| matches!(notice, WorkspaceNoticeKind::AddedWires { .. })),
        "the array it had keeps its wires"
    );
    assert_eq!(
        lane_keys(&snapshot),
        (0..5)
            .map(|lane| dongle(&(1000 + lane).to_string()))
            .collect::<Vec<Value>>()
    );
    assert_eq!(
        array_lanes(&snapshot, "arr"),
        vec![
            ("kraken".to_owned(), "lane".to_owned()),
            ("kraken-lane2".to_owned(), "lane2".to_owned()),
            ("kraken-lane3".to_owned(), "lane3".to_owned()),
            ("kraken-lane4".to_owned(), "lane4".to_owned()),
            ("kraken-lane5".to_owned(), "lane5".to_owned()),
        ]
    );
    assert!(
        snapshot["graph"]["edges"]
            .as_array()
            .is_some_and(|edges| edges.contains(&wire(("gps", "position"), ("kraken", "position")))),
        "the position wire stays on the node that kept the id"
    );
    assert_eq!(
        snapshot["graph"]["nodes"].as_array().map(|nodes| nodes
            .iter()
            .filter(|node| node["kind"] == json!("array"))
            .count()),
        Some(1),
        "no second array for the same bank"
    );
    parse_workspace_snapshot(&snapshot.to_string()).expect("the split workspace loads");
}

#[test]
fn an_unwired_bank_gains_five_dongles_and_an_array_in_lane_order() {
    let mut snapshot = with_wires(json!([bank("kraken")]), json!([]));
    let notices = banks::split_kraken_banks(&mut snapshot);
    assert_eq!(
        kinds_added(&notices),
        vec![
            "kraken-lane2",
            "kraken-lane3",
            "kraken-lane4",
            "kraken-lane5",
            "kraken-array"
        ]
    );
    assert_eq!(
        notices.last(),
        Some(&WorkspaceNoticeKind::AddedWires {
            wires: (0..5)
                .map(|lane| {
                    let from = if lane == 0 {
                        "kraken".to_owned()
                    } else {
                        format!("kraken-lane{}", lane + 1)
                    };
                    notice_wire(
                        (from.as_str(), "iq"),
                        ("kraken-array", &sdrmm_wire::stream_port("lane", lane)),
                        "One dongle per antenna",
                    )
                })
                .collect()
        })
    );
    assert_eq!(
        array_lanes(&snapshot, "kraken-array"),
        vec![
            ("kraken".to_owned(), "lane".to_owned()),
            ("kraken-lane2".to_owned(), "lane2".to_owned()),
            ("kraken-lane3".to_owned(), "lane3".to_owned()),
            ("kraken-lane4".to_owned(), "lane4".to_owned()),
            ("kraken-lane5".to_owned(), "lane5".to_owned()),
        ]
    );
    let places: Vec<Value> = snapshot["graph"]["nodes"]
        .as_array()
        .map(|nodes| nodes.iter().map(|node| node["position"].clone()).collect())
        .unwrap_or_default();
    assert_eq!(places.len(), 6);
    assert!(
        places
            .iter()
            .enumerate()
            .all(|(n, place)| !places[..n].contains(place)),
        "{places:?}"
    );
    let parsed =
        parse_workspace_snapshot(&snapshot.to_string()).expect("the split workspace loads");
    assert_eq!(
        parsed
            .graph
            .edges
            .iter()
            .filter(|edge| edge.to.node == "kraken-array")
            .count(),
        5,
        "the contract keeps every lane"
    );
}

#[test]
fn a_bank_of_four_lanes_stays_four() {
    let mut snapshot = with_wires(
        json!([bank("kerberos"), at("scope", "scope", json!({}))]),
        json!([wire(("kerberos", "iq4"), ("scope", "iq"))]),
    );
    banks::split_kraken_banks(&mut snapshot);
    assert_eq!(
        lane_keys(&snapshot),
        (0..4)
            .map(|lane| dongle(&(1000 + lane).to_string()))
            .collect::<Vec<Value>>()
    );
    assert_eq!(
        snapshot["graph"]["edges"]
            .as_array()
            .and_then(|edges| edges.first())
            .cloned(),
        Some(wire(("kerberos-lane4", "iq"), ("scope", "iq")))
    );
}

#[test]
fn a_workspace_without_a_bank_is_left_alone() {
    let before = watching();
    let mut after = before.clone();
    assert!(banks::split_kraken_banks(&mut after).is_empty());
    assert_eq!(after, before);
}

#[test]
fn splitting_a_bank_twice_equals_once() {
    let mut snapshot = banked();
    banks::split_kraken_banks(&mut snapshot);
    let mut again = snapshot.clone();
    assert!(banks::split_kraken_banks(&mut again).is_empty());
    assert_eq!(again, snapshot);
}
