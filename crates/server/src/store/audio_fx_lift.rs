use rusqlite::{Connection, OptionalExtension, params};
use sdrmm_wire::{AudioAgcMode, AudioProcessing};
use serde_json::{Value, json};

use super::StoreError;

struct Lifted {
    node: String,
    chain: AudioProcessing,
}

pub(super) fn lift_audio_chains(conn: &Connection) -> Result<(), StoreError> {
    let rows: Vec<(i64, String)> = conn
        .prepare("SELECT workspace_id, state FROM workspace_state WHERE state LIKE '%\"audio\"%'")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (id, state_json) in rows {
        let mut state: Value = serde_json::from_str(&state_json)?;
        let lifted = take_chains(&mut state);
        if lifted.is_none() {
            continue;
        }
        let snapshot_json: Option<String> = conn
            .query_row(
                "SELECT snapshot FROM workspaces WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        let tx = conn.unchecked_transaction()?;
        if let Some(snapshot_json) = snapshot_json {
            let mut snapshot: Value = serde_json::from_str(&snapshot_json)?;
            for chain in lifted.into_iter().flatten() {
                give_to_decoder(&mut snapshot, &chain);
            }
            let nodes = snapshot["graph"]["nodes"].as_array().map_or(0, Vec::len);
            tx.execute(
                "UPDATE workspaces SET snapshot = ?2, nodes = ?3, revision = revision + 1 \
                 WHERE id = ?1",
                params![id, serde_json::to_string(&snapshot)?, nodes as i64],
            )?;
        }
        tx.execute(
            "UPDATE workspace_state SET state = ?2 WHERE workspace_id = ?1",
            params![id, serde_json::to_string(&state)?],
        )?;
        tx.commit()?;
    }
    Ok(())
}

fn take_chains(state: &mut Value) -> Option<Vec<Lifted>> {
    let channels = state.get_mut("channels")?.as_array_mut()?;
    let mut touched = false;
    let mut lifted = Vec::new();
    for channel in channels {
        let Some(node) = channel
            .get("node")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let Some(settings) = channel.get_mut("settings").and_then(Value::as_object_mut) else {
            continue;
        };
        let Some(audio) = settings.remove("audio") else {
            continue;
        };
        touched = true;
        if let Some(blanker) = audio.get("blanker") {
            settings.entry("blanker").or_insert_with(|| blanker.clone());
        }
        let type_id = settings
            .get("params")
            .and_then(|params| params.get("type"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Ok(chain) = crate::json::from_value::<AudioProcessing>(&audio) else {
            continue;
        };
        if worth_keeping(&chain, type_id) {
            lifted.push(Lifted { node, chain });
        }
    }
    touched.then_some(lifted)
}

fn worth_keeping(chain: &AudioProcessing, type_id: &str) -> bool {
    let levelled_by_its_demod = chain.agc == AudioAgcMode::Medium
        && sdrmm_decoder::descriptor(type_id).is_some_and(|descriptor| descriptor.levels_audio);
    let rest = AudioProcessing {
        agc: if levelled_by_its_demod {
            AudioAgcMode::Off
        } else {
            chain.agc
        },
        ..chain.clone()
    };
    rest.is_active() && rest.validate().is_ok()
}

/// Puts the chain on the decoder node the channel belonged to.
fn give_to_decoder(snapshot: &mut Value, lifted: &Lifted) {
    let Some(node) = snapshot["graph"]["nodes"].as_array_mut().and_then(|nodes| {
        nodes
            .iter_mut()
            .find(|node| node.get("id").and_then(Value::as_str) == Some(&lifted.node))
    }) else {
        return;
    };
    if !node["data"].is_object() {
        node["data"] = json!({});
    }
    node["data"]["audio"] = json!(lifted.chain);
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{NodeBody, WorkspaceSnapshot, WorkspaceState};

    use super::*;

    fn store_with(snapshot: &Value, state: &Value) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        super::super::migrate(&conn).expect("schema");
        conn.execute(
            "INSERT INTO workspaces (id, name, created_at, updated_at, revision, nodes, snapshot) \
             VALUES (1, 'w', 't', 't', 1, 3, ?1)",
            params![snapshot.to_string()],
        )
        .expect("workspace row");
        conn.execute(
            "INSERT INTO workspace_state (workspace_id, updated_at, state) VALUES (1, 't', ?1)",
            params![state.to_string()],
        )
        .expect("state row");
        conn
    }

    fn graph() -> Value {
        json!({
            "version": sdrmm_wire::WORKSPACE_SNAPSHOT_VERSION,
            "graph": {
                "nodes": [
                    {"id": "ch", "kind": "channel", "data": {"channel_type": "nfm"}, "position": {"x": 10.0, "y": 20.0}},
                    {"id": "spk", "kind": "speaker", "position": {"x": 900.0, "y": 20.0}},
                    {"id": "rec", "kind": "audio_recorder", "data": {}, "position": {"x": 900.0, "y": 300.0}}
                ],
                "edges": [
                    {"from": {"node": "ch", "port": "audio"}, "to": {"node": "spk", "port": "audio"}},
                    {"from": {"node": "ch", "port": "audio"}, "to": {"node": "rec", "port": "audio"}}
                ]
            },
            "rack": {},
            "settings": {}
        })
    }

    fn state(type_id: &str, audio: &Value) -> Value {
        json!({
            "version": 2,
            "devices": [],
            "channels": [{
                "node": "ch",
                "settings": {
                    "frequency_hz": 145_500_000.0,
                    "params": {"type": type_id, "settings": {}},
                    "audio": audio
                }
            }],
            "trunks": []
        })
    }

    fn read(conn: &Connection) -> (WorkspaceSnapshot, WorkspaceState, Value) {
        let (snapshot, state): (String, String) = conn
            .query_row(
                "SELECT w.snapshot, s.state FROM workspaces w JOIN workspace_state s \
                 ON s.workspace_id = w.id WHERE w.id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("rows");
        (
            serde_json::from_str(&snapshot).expect("snapshot parses"),
            serde_json::from_str(&state).expect("state parses"),
            serde_json::from_str(&state).expect("state json"),
        )
    }

    #[test]
    fn a_channel_chain_moves_onto_its_decoder_node() {
        let conn = store_with(
            &graph(),
            &state(
                "nfm",
                &json!({"auto_notch": true, "blanker": {"enabled": true, "threshold": 7.0}}),
            ),
        );
        lift_audio_chains(&conn).expect("lift");
        let (snapshot, state, raw) = read(&conn);
        snapshot
            .graph
            .validate()
            .expect("the lifted graph is valid");
        let NodeBody::Channel(decoder) = &snapshot.graph.nodes[0].body else {
            panic!("wrong kind {:?}", snapshot.graph.nodes[0].body);
        };
        assert!(decoder.audio.auto_notch);
        assert_eq!(snapshot.graph.nodes.len(), 3);
        assert_eq!(snapshot.graph.edges.len(), 2);
        assert!(state.channels[0].settings.blanker.enabled);
        assert!(raw["channels"][0]["settings"].get("audio").is_none());
    }

    #[test]
    fn the_old_am_default_adds_nothing() {
        let conn = store_with(&graph(), &state("am", &json!({"agc": "medium"})));
        lift_audio_chains(&conn).expect("lift");
        let (snapshot, _, raw) = read(&conn);
        let NodeBody::Channel(decoder) = &snapshot.graph.nodes[0].body else {
            panic!("wrong kind {:?}", snapshot.graph.nodes[0].body);
        };
        assert!(decoder.audio.is_off());
        assert!(raw["channels"][0]["settings"].get("audio").is_none());
    }

    #[test]
    fn a_database_error_aborts_the_lift_instead_of_stripping_the_chain() {
        let conn = store_with(&graph(), &state("nfm", &json!({"auto_notch": true})));
        conn.execute_batch("DROP TABLE workspaces").expect("drop");
        let error = lift_audio_chains(&conn).expect_err("the failed read surfaces");
        assert!(matches!(error, StoreError::Db(_)), "{error}");
        let state: String = conn
            .query_row(
                "SELECT state FROM workspace_state WHERE workspace_id = 1",
                [],
                |row| row.get(0),
            )
            .expect("state row");
        assert!(state.contains("\"audio\""), "the chain was stripped anyway");
    }

    #[test]
    fn lifting_twice_changes_nothing_more() {
        let conn = store_with(&graph(), &state("nfm", &json!({"agc": "fast"})));
        lift_audio_chains(&conn).expect("lift");
        let once = read(&conn).0;
        lift_audio_chains(&conn).expect("lift again");
        assert_eq!(read(&conn).0, once);
    }
}
