use serde_json::json;

use super::*;

fn graph(nodes: Value, edges: Value) -> Value {
    json!({ "graph": { "nodes": nodes, "edges": edges } })
}

#[test]
fn moves_of_different_nodes_both_land() {
    let base = graph(
        json!([{ "id": "a", "position": { "x": 0, "y": 0 } }, { "id": "b", "position": { "x": 0, "y": 0 } }]),
        json!([]),
    );
    let ours = graph(
        json!([{ "id": "a", "position": { "x": 5, "y": 0 } }, { "id": "b", "position": { "x": 0, "y": 0 } }]),
        json!([]),
    );
    let theirs = graph(
        json!([{ "id": "a", "position": { "x": 0, "y": 0 } }, { "id": "b", "position": { "x": 0, "y": 9 } }]),
        json!([]),
    );
    assert_eq!(
        merge3(&base, &ours, &theirs),
        graph(
            json!([{ "id": "a", "position": { "x": 5, "y": 0 } }, { "id": "b", "position": { "x": 0, "y": 9 } }]),
            json!([]),
        )
    );
}

#[test]
fn different_fields_of_one_node_both_land() {
    let base = json!({ "id": "a", "label": "x", "gain": 1 });
    let ours = json!({ "id": "a", "label": "y", "gain": 1 });
    let theirs = json!({ "id": "a", "label": "x", "gain": 7 });
    assert_eq!(
        merge3(&base, &ours, &theirs),
        json!({ "id": "a", "label": "y", "gain": 7 })
    );
}

#[test]
fn the_same_field_goes_to_the_later_write() {
    let base = json!({ "gain": 1 });
    assert_eq!(
        merge3(&base, &json!({ "gain": 2 }), &json!({ "gain": 3 })),
        json!({ "gain": 2 })
    );
}

#[test]
fn additions_from_both_sides_are_kept_in_order() {
    let base = json!([{ "id": "a" }]);
    let ours = json!([{ "id": "a" }, { "id": "mine" }]);
    let theirs = json!([{ "id": "a" }, { "id": "yours" }]);
    assert_eq!(
        merge3(&base, &ours, &theirs),
        json!([{ "id": "a" }, { "id": "yours" }, { "id": "mine" }])
    );
}

#[test]
fn a_delete_beats_a_concurrent_edit() {
    let base = json!([{ "id": "a", "x": 0 }, { "id": "b" }]);
    let ours = json!([{ "id": "a", "x": 4 }, { "id": "b" }]);
    let theirs = json!([{ "id": "b" }]);
    assert_eq!(merge3(&base, &ours, &theirs), json!([{ "id": "b" }]));
    assert_eq!(merge3(&base, &theirs, &ours), json!([{ "id": "b" }]));
}

#[test]
fn wires_merge_as_a_set() {
    let wire = |from: &str, to: &str| json!({ "from": { "node": from, "port": "o" }, "to": { "node": to, "port": "i" } });
    let base = json!([wire("a", "b"), wire("a", "c")]);
    let ours = json!([wire("a", "b"), wire("a", "d")]);
    let theirs = json!([wire("a", "b"), wire("a", "c"), wire("a", "e")]);
    assert_eq!(
        merge3(&base, &ours, &theirs),
        json!([wire("a", "b"), wire("a", "e"), wire("a", "d")])
    );
}

#[test]
fn rack_slots_are_keyed_by_node() {
    let base = json!([{ "node": "a", "x": 0 }, { "node": "b", "x": 0 }]);
    let ours = json!([{ "node": "a", "x": 1 }, { "node": "b", "x": 0 }]);
    let theirs = json!([{ "node": "a", "x": 0 }, { "node": "b", "x": 2 }]);
    assert_eq!(
        merge3(&base, &ours, &theirs),
        json!([{ "node": "a", "x": 1 }, { "node": "b", "x": 2 }])
    );
}

#[test]
fn a_changed_kind_is_taken_whole() {
    let base = json!({ "kind": "nfm", "squelch": 1 });
    let ours = json!({ "kind": "am", "depth": 3 });
    let theirs = json!({ "kind": "nfm", "squelch": 5 });
    assert_eq!(merge3(&base, &ours, &theirs), ours);
}

#[test]
fn scalar_lists_are_not_spliced() {
    let base = json!([1, 2, 3]);
    let ours = json!([1, 5, 3]);
    let theirs = json!([1, 2, 3, 4]);
    assert_eq!(merge3(&base, &ours, &theirs), ours);
}

#[test]
fn typed_merge_round_trips() {
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    struct Tune {
        freq: u32,
        gain: u8,
    }
    let merged = merge_typed(
        &Tune { freq: 1, gain: 1 },
        &Tune { freq: 2, gain: 1 },
        &Tune { freq: 1, gain: 9 },
    )
    .expect("merge");
    assert_eq!(merged, Tune { freq: 2, gain: 9 });
}
