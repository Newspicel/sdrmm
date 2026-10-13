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
            decoder("pager", "pocsag"),
            at("trunk", "dmr_trunk", json!({})),
            at("loud", "spectrum_monitor", json!({ "record_audio": true })),
            at(
                "quiet",
                "spectrum_monitor",
                json!({ "record_audio": false })
            ),
            at("keep", "event_filter", json!({})),
            at("log", "decoder_log", json!({})),
            at(
                "hook",
                "event_output",
                json!({ "target": { "service": "webhook", "format": "json", "url": "" } }),
            ),
            at(
                "chat",
                "event_output",
                json!({ "target": { "service": "webhook", "format": "discord", "url": "" } }),
            ),
            json!({ "id": "map", "kind": "map", "position": { "x": 0.0, "y": 0.0 } }),
        ]),
        json!([
            wire(("dmr", "events"), ("keep", "events")),
            wire(("keep", "events"), ("log", "events")),
            wire(("pager", "events"), ("log", "events")),
            wire(("trunk", "events"), ("hook", "events")),
            wire(("trunk", "events"), ("chat", "events")),
            wire(("loud", "events"), ("log", "events")),
            wire(("quiet", "events"), ("log", "events")),
            wire(("dmr", "events"), ("map", "events")),
        ]),
    )
}

#[test]
fn call_audio_gets_a_wire_to_each_log_and_output_its_events_reach() {
    let mut snapshot = calling();
    let notices = call_audio::wire_call_audio(&mut snapshot);
    assert_eq!(
        notices,
        vec![WorkspaceNoticeKind::AddedWires {
            wires: vec![
                notice_wire(("dmr", "audio"), ("log", "audio"), "Keeps call audio"),
                notice_wire(("trunk", "audio"), ("chat", "audio"), "Keeps call audio"),
                notice_wire(("loud", "audio"), ("log", "audio"), "Keeps call audio"),
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
    let mut again = snapshot.clone();
    assert!(call_audio::wire_call_audio(&mut again).is_empty());
    assert_eq!(again, snapshot);
    assert_eq!(edge_count(&snapshot), 11);
    parse_workspace_snapshot(&snapshot.to_string()).expect("the wired workspace loads");
}

#[test]
fn an_import_wires_call_audio_once() {
    let mut imported = json!({ "version": 1, "name": "Old", "snapshot": calling() });
    let notices = upgrade_import(&mut imported);
    assert!(notices.iter().any(|notice| matches!(
        notice,
        WorkspaceNoticeKind::AddedWires { wires } if wires.iter().all(|wire| wire.reason == "Keeps call audio")
    )));
    assert_eq!(edge_count(&imported["snapshot"]), 11);
}

#[test]
fn a_current_export_keeps_the_audio_wire_the_user_removed() {
    let trunk_to_log = || {
        with_wires(
            json!([
                at("trunk", "dmr_trunk", json!({})),
                at("log", "decoder_log", json!({})),
            ]),
            json!([wire(("trunk", "events"), ("log", "events"))]),
        )
    };
    let mut current = json!({
        "version": sdrmm_wire::WORKSPACE_EXPORT_VERSION,
        "name": "New",
        "snapshot": trunk_to_log(),
    });
    assert!(upgrade_import(&mut current).is_empty());
    assert_eq!(edge_count(&current["snapshot"]), 1);

    let mut old = json!({ "version": 1, "name": "Old", "snapshot": trunk_to_log() });
    upgrade_import(&mut old);
    assert_eq!(edge_count(&old["snapshot"]), 2);
    assert_eq!(old["version"], sdrmm_wire::WORKSPACE_EXPORT_VERSION);
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
