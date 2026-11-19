use std::collections::{HashMap, HashSet};

use sdrmm_wire::{AudioProcessing, DroppedNode, WorkspaceNoticeKind};
use serde_json::{Value, json};

use crate::store::{edge_end, node_kind, remove_nodes};

const RETIRED_AUDIO_FX: &str = "audio_fx";
const AUDIO_PORT: &str = "audio";

/// Filters, denoise and AGC live in the decoder now. Each Audio FX node hands its settings to
/// the decoder it hears and its listeners wire straight to that decoder. Settings with nowhere
/// to go, or a second chain on one decoder, are dropped and said so.
pub(super) fn audio_fx_joins_decoders(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let fx = fx_nodes(snapshot);
    if fx.is_empty() {
        return Vec::new();
    }
    let roots: HashMap<String, Option<(String, String)>> = fx
        .keys()
        .map(|id| (id.clone(), root_of(snapshot, id, &fx)))
        .collect();
    let mut dropped = Vec::new();
    for (id, root) in &roots {
        let settings = &fx[id];
        let kept = root
            .as_ref()
            .is_some_and(|(node, _)| give_to_decoder(snapshot, node, settings));
        if !kept && settings_matter(settings) {
            dropped.push(dropped_node(snapshot, id));
        }
    }
    rewire_listeners(snapshot, &roots);
    let ids: HashSet<String> = fx.into_keys().collect();
    remove_nodes(snapshot, &ids);
    if dropped.is_empty() {
        Vec::new()
    } else {
        vec![WorkspaceNoticeKind::DroppedNodes { nodes: dropped }]
    }
}

fn fx_nodes(snapshot: &Value) -> HashMap<String, Value> {
    nodes(snapshot)
        .filter(|node| node_kind(node) == Some(RETIRED_AUDIO_FX))
        .filter_map(|node| {
            let id = node.get("id")?.as_str()?.to_owned();
            let settings = node
                .get("data")
                .and_then(|data| data.get("settings"))
                .cloned()
                .unwrap_or_else(|| json!({}));
            Some((id, settings))
        })
        .collect()
}

/// The output an FX node heard, walking back through any FX before it.
fn root_of(snapshot: &Value, fx: &str, all: &HashMap<String, Value>) -> Option<(String, String)> {
    let mut seen = HashSet::new();
    let mut at = fx.to_owned();
    loop {
        if !seen.insert(at.clone()) {
            return None;
        }
        let (node, port) = edges(snapshot)
            .filter_map(|edge| Some((edge_end(edge, "from")?, edge_end(edge, "to")?)))
            .find(|(_, to)| *to == (at.as_str(), AUDIO_PORT))
            .map(|(from, _)| (from.0.to_owned(), from.1.to_owned()))?;
        if !all.contains_key(&node) {
            return Some((node, port));
        }
        at = node;
    }
}

fn give_to_decoder(snapshot: &mut Value, decoder: &str, settings: &Value) -> bool {
    let Some(node) = nodes_mut(snapshot).find(|node| {
        node.get("id").and_then(Value::as_str) == Some(decoder)
            && node_kind(node) == Some("channel")
    }) else {
        return false;
    };
    let data = &mut node["data"];
    if !data.is_object() {
        *data = json!({});
    }
    if data.get("audio").is_some_and(settings_matter) {
        return data.get("audio") == Some(settings);
    }
    data["audio"] = settings.clone();
    true
}

fn rewire_listeners(snapshot: &mut Value, roots: &HashMap<String, Option<(String, String)>>) {
    let Some(edges) = snapshot
        .get_mut("graph")
        .and_then(|graph| graph.get_mut("edges"))
        .and_then(Value::as_array_mut)
    else {
        return;
    };
    for edge in edges.iter_mut() {
        let Some((from, _)) = edge_end(edge, "from") else {
            continue;
        };
        if let Some(Some((node, port))) = roots.get(from) {
            edge["from"] = json!({ "node": node, "port": port });
        }
    }
    let mut kept = HashSet::new();
    edges.retain(|edge| kept.insert(edge.to_string()));
}

fn settings_matter(settings: &Value) -> bool {
    crate::json::from_value::<AudioProcessing>(settings).is_ok_and(|chain| chain.is_active())
}

fn dropped_node(snapshot: &Value, id: &str) -> DroppedNode {
    let label = nodes(snapshot)
        .find(|node| node.get("id").and_then(Value::as_str) == Some(id))
        .and_then(|node| node.get("label"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    DroppedNode {
        id: id.to_owned(),
        kind: RETIRED_AUDIO_FX.to_owned(),
        label,
    }
}

fn nodes(snapshot: &Value) -> impl Iterator<Item = &Value> {
    snapshot
        .get("graph")
        .and_then(|graph| graph.get("nodes"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

fn nodes_mut(snapshot: &mut Value) -> impl Iterator<Item = &mut Value> {
    super::graph_nodes(snapshot)
}

fn edges(snapshot: &Value) -> impl Iterator<Item = &Value> {
    snapshot
        .get("graph")
        .and_then(|graph| graph.get("edges"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}
