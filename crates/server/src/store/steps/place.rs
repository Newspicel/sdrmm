use serde_json::{Value, json};

use super::graph_nodes;

const BESIDE_PX: f64 = 600.0;
const BELOW_PX: f64 = 640.0;

pub(super) fn free_id(snapshot: &mut Value, wanted: &str) -> String {
    let taken: Vec<String> = graph_nodes(snapshot)
        .filter_map(|node| Some(node.get("id")?.as_str()?.to_owned()))
        .collect();
    (1..)
        .map(|n| {
            if n == 1 {
                wanted.to_owned()
            } else {
                format!("{wanted}-{n}")
            }
        })
        .find(|id| !taken.contains(id))
        .unwrap_or_else(|| wanted.to_owned())
}

pub(super) fn beside(snapshot: &mut Value, node: &str) -> Value {
    near(snapshot, node, BESIDE_PX, 0.0)
}

pub(super) fn below(snapshot: &mut Value, node: &str, rows: u32) -> Value {
    near(snapshot, node, 0.0, BELOW_PX * f64::from(rows))
}

pub(super) fn push_node(snapshot: &mut Value, node: Value) {
    if let Some(nodes) = snapshot
        .get_mut("graph")
        .and_then(|graph| graph.get_mut("nodes"))
        .and_then(Value::as_array_mut)
    {
        nodes.push(node);
    }
}

fn near(snapshot: &mut Value, node: &str, dx: f64, dy: f64) -> Value {
    let at = graph_nodes(snapshot)
        .find(|found| found.get("id").and_then(Value::as_str) == Some(node))
        .and_then(|found| found.get("position").cloned())
        .unwrap_or_else(|| json!({ "x": 0.0, "y": 0.0 }));
    let x = at.get("x").and_then(Value::as_f64).unwrap_or_default() + dx;
    let y = at.get("y").and_then(Value::as_f64).unwrap_or_default() + dy;
    json!({ "x": x, "y": y })
}
