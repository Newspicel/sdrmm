use std::collections::HashMap;

use rusqlite::{Connection, OptionalExtension, params};
use sdrmm_wire::{EventOutputTarget, NodeBody, NodeKind, WorkspaceSnapshot, event_output::Secret};
use serde_json::Value;

use super::{Store, StoreError, now_rfc3339, rfc3339, steps::graph_nodes};

/// How long a secret nothing names yet is kept: the UI stores a credential first and saves the
/// node that names it a moment later.
const UNCLAIMED_GRACE: jiff::SignedDuration = jiff::SignedDuration::from_mins(10);

impl Store {
    /// Keeps `value` under a fresh id and answers that id.
    pub fn create_secret(&self, value: &str) -> Result<String, StoreError> {
        let id = insert(&self.lock(), value)?;
        self.secrets_changed();
        Ok(id)
    }

    /// Writes `value` under `id`, the id a workspace from another server may already name.
    pub fn put_secret(&self, id: &str, value: &str) -> Result<(), StoreError> {
        self.lock().execute(
            "INSERT INTO secrets (id, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(id) DO UPDATE SET value = ?2, updated_at = ?3",
            params![id, value, now_rfc3339()],
        )?;
        self.secrets_changed();
        Ok(())
    }

    pub fn secret(&self, id: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .lock()
            .query_row(
                "SELECT value FROM secrets WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub(crate) fn subscribe_secrets(&self) -> tokio::sync::watch::Receiver<u64> {
        self.secret_changes.subscribe()
    }

    fn secrets_changed(&self) {
        self.secret_changes
            .send_modify(|count| *count = count.wrapping_add(1));
    }
}

/// Drops every secret older than [`UNCLAIMED_GRACE`] that no workspace and no history entry
/// names. A reference is the id as a JSON string anywhere in a snapshot, so a snapshot this
/// server can no longer read still keeps its credentials.
pub(super) fn collect(conn: &Connection) -> Result<usize, StoreError> {
    let cutoff = jiff::Timestamp::now()
        .checked_sub(UNCLAIMED_GRACE)
        .unwrap_or(jiff::Timestamp::MIN);
    let removed = conn.execute(
        "DELETE FROM secrets WHERE updated_at < ?1 \
         AND NOT EXISTS (SELECT 1 FROM workspaces \
             WHERE instr(snapshot, '\"' || secrets.id || '\"') > 0) \
         AND NOT EXISTS (SELECT 1 FROM workspace_history \
             WHERE instr(snapshot, '\"' || secrets.id || '\"') > 0)",
        params![rfc3339(cutoff)],
    )?;
    if removed > 0 {
        tracing::info!(removed, "dropped secrets no workspace names");
    }
    Ok(removed)
}

fn insert(conn: &Connection, value: &str) -> Result<String, StoreError> {
    Ok(conn.query_row(
        "INSERT INTO secrets (id, value, updated_at) \
         VALUES (lower(hex(randomblob(16))), ?1, ?2) RETURNING id",
        params![value, now_rfc3339()],
        |row| row.get(0),
    )?)
}

/// Moves every credential written in place into the store, leaving the id behind.
pub(super) fn lift(conn: &Connection, snapshot: &mut WorkspaceSnapshot) -> Result<(), StoreError> {
    for node in &mut snapshot.graph.nodes {
        if let NodeBody::EventOutput(output) = &mut node.body {
            lift_target(conn, &mut output.target, &mut HashMap::new())?;
        }
    }
    Ok(())
}

type Lifted = HashMap<String, String>;

const LIFTED_KEY: &str = "step:lift_secrets";

fn lift_target(
    conn: &Connection,
    target: &mut EventOutputTarget,
    lifted: &mut Lifted,
) -> Result<bool, StoreError> {
    let Some(secret) = target.secret_mut() else {
        return Ok(false);
    };
    let Some(value) = secret.inline().map(str::to_owned) else {
        return Ok(false);
    };
    let id = match lifted.get(&value) {
        Some(id) => id.clone(),
        None => {
            let id = insert(conn, &value)?;
            lifted.insert(value, id.clone());
            id
        }
    };
    *secret = Secret::stored(id);
    Ok(true)
}

/// Older servers kept credentials inside the workspace, in its history too. Each output's value
/// moves into the store once, and every entry that held it names the same id afterwards.
pub(super) fn lift_stored(conn: &Connection) -> Result<(), StoreError> {
    let done: Option<String> = conn
        .query_row(
            "SELECT value FROM server_meta WHERE key = ?1",
            params![LIFTED_KEY],
            |row| row.get(0),
        )
        .optional()?;
    if done.is_some() {
        return Ok(());
    }
    let tx = conn.unchecked_transaction()?;
    let mut lifted: HashMap<(i64, String), Lifted> = HashMap::new();
    for (table, key) in [("workspaces", "id"), ("workspace_history", "rowid")] {
        let rows: Vec<(i64, i64, String)> = conn
            .prepare(&format!(
                "SELECT {key}, {workspace}, snapshot FROM {table}",
                workspace = if table == "workspaces" {
                    "id"
                } else {
                    "workspace_id"
                },
            ))?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (row, workspace, json) in rows {
            let Ok(mut snapshot) = serde_json::from_str::<Value>(&json) else {
                continue;
            };
            if !lift_value(&tx, &mut snapshot, workspace, &mut lifted)? {
                continue;
            }
            tx.execute(
                &format!("UPDATE {table} SET snapshot = ?2 WHERE {key} = ?1"),
                params![row, serde_json::to_string(&snapshot)?],
            )?;
        }
    }
    tx.execute(
        "INSERT INTO server_meta (key, value) VALUES (?1, 'done')",
        params![LIFTED_KEY],
    )?;
    tx.commit()?;
    Ok(())
}

fn lift_value(
    conn: &Connection,
    snapshot: &mut Value,
    workspace: i64,
    lifted: &mut HashMap<(i64, String), Lifted>,
) -> Result<bool, StoreError> {
    let mut changed = false;
    for node in graph_nodes(snapshot) {
        if node.get("kind").and_then(Value::as_str) != Some(NodeKind::EventOutput.as_str()) {
            continue;
        }
        let id = node
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let Some(target) = node.get_mut("data").and_then(|data| data.get_mut("target")) else {
            continue;
        };
        let Ok(mut typed) = serde_json::from_value::<EventOutputTarget>(target.clone()) else {
            continue;
        };
        if lift_target(conn, &mut typed, lifted.entry((workspace, id)).or_default())? {
            *target = serde_json::to_value(&typed)?;
            changed = true;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::{EventOutputNode, PatchNode, Position, UpdateWorkspaceRequest};

    use super::*;

    const TOKEN: &str = "matrix-secret";

    fn matrix(token: Secret) -> EventOutputTarget {
        EventOutputTarget::Matrix {
            homeserver_url: "https://matrix.example".to_owned(),
            room_id: "!radio:matrix.example".to_owned(),
            access_token: token,
        }
    }

    fn inline(token: &str) -> WorkspaceSnapshot {
        let mut snapshot = WorkspaceSnapshot::starter();
        snapshot.graph.nodes.push(PatchNode {
            id: "out".to_owned(),
            body: NodeBody::EventOutput(EventOutputNode {
                target: matrix(Secret::Inline(token.to_owned())),
            }),
            position: Position { x: 0.0, y: 400.0 },
            size: None,
            label: None,
        });
        snapshot
    }

    fn stored_id(snapshot: &WorkspaceSnapshot) -> String {
        let Some(NodeBody::EventOutput(output)) = snapshot.graph.node("out").map(|node| &node.body)
        else {
            panic!("the output survives");
        };
        match output.target.secret() {
            Some((_, Secret::Stored { secret })) => secret.clone(),
            other => panic!("not stored: {other:?}"),
        }
    }

    #[test]
    fn a_saved_workspace_keeps_only_the_id_of_its_credential() {
        let store = Store::open(None).expect("store");
        let id = store
            .create_workspace("Out", &inline(TOKEN))
            .expect("created");
        let detail = store.workspace(id).expect("read");
        let secret = stored_id(&detail.snapshot);
        assert_eq!(store.secret(&secret).expect("read").as_deref(), Some(TOKEN));
        let raw: String = store
            .hold()
            .query_row(
                "SELECT snapshot FROM workspaces WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .expect("row");
        assert!(!raw.contains(TOKEN));
        let export =
            serde_json::to_string(&store.export_workspace(id).expect("export")).expect("encode");
        assert!(!export.contains(TOKEN), "{export}");

        let request = UpdateWorkspaceRequest {
            revision: detail.info.revision,
            name: None,
            snapshot: Some(inline("next")),
        };
        let updated = store.update_workspace(id, &request, None).expect("updated");
        let next = stored_id(&updated.snapshot);
        assert_eq!(store.secret(&next).expect("read").as_deref(), Some("next"));
    }

    #[test]
    fn a_workspace_from_another_server_names_a_credential_this_one_lacks() {
        let first = Store::open(None).expect("first");
        let id = first
            .create_workspace("Out", &inline(TOKEN))
            .expect("created");
        let export = first.export_workspace(id).expect("export");
        let second = Store::open(None).expect("second");

        let imported = second.import_workspace(&export, &[]).expect("imported");

        let secret = stored_id(&second.workspace(imported).expect("read").snapshot);
        assert_eq!(second.secret(&secret).expect("read"), None);
        second.put_secret(&secret, TOKEN).expect("filled");
        assert_eq!(
            second.secret(&secret).expect("read").as_deref(),
            Some(TOKEN)
        );
    }

    const LONG_AGO: &str = "2000-01-01T00:00:00.000000000Z";

    fn age_secrets(store: &Store) {
        store
            .hold()
            .execute("UPDATE secrets SET updated_at = ?1", [LONG_AGO])
            .expect("aged");
    }

    fn kept(store: &Store, id: &str) -> bool {
        store.secret(id).expect("read").is_some()
    }

    fn with_stored(id: &str) -> WorkspaceSnapshot {
        let mut snapshot = inline("unused");
        if let Some(NodeBody::EventOutput(output)) =
            snapshot.graph.nodes.last_mut().map(|node| &mut node.body)
        {
            output.target = matrix(Secret::stored(id));
        }
        snapshot
    }

    fn write(store: &Store, id: i64, snapshot: WorkspaceSnapshot) {
        let revision = store.workspace(id).expect("read").info.revision;
        let request = UpdateWorkspaceRequest {
            revision,
            name: None,
            snapshot: Some(snapshot),
        };
        store.update_workspace(id, &request, None).expect("updated");
    }

    #[test]
    fn a_fresh_secret_waits_for_the_node_that_names_it() {
        let store = Store::open(None).expect("store");
        let fresh = store.create_secret(TOKEN).expect("kept");

        collect(&store.hold()).expect("collected");
        assert!(kept(&store, &fresh));

        age_secrets(&store);
        collect(&store.hold()).expect("collected");
        assert!(!kept(&store, &fresh));
    }

    #[test]
    fn a_deleted_workspace_takes_the_secrets_only_it_names() {
        let store = Store::open(None).expect("store");
        let first = store.create_workspace("A", &inline(TOKEN)).expect("a");
        let shared = stored_id(&store.workspace(first).expect("read").snapshot);
        let second = store
            .create_workspace("B", &with_stored(&shared))
            .expect("b");
        let alone = store.create_workspace("C", &inline("alone")).expect("c");
        let own = stored_id(&store.workspace(alone).expect("read").snapshot);
        age_secrets(&store);

        store.delete_workspace(first).expect("deleted");
        store.delete_workspace(alone).expect("deleted");

        assert!(kept(&store, &shared));
        assert!(!kept(&store, &own));
        store.delete_workspace(second).expect("deleted");
        assert!(!kept(&store, &shared));
    }

    #[test]
    fn a_secret_only_history_names_lasts_until_the_history_forgets_it() {
        let store = Store::open(None).expect("store");
        let id = store
            .create_workspace("Out", &inline(TOKEN))
            .expect("created");
        let secret = stored_id(&store.workspace(id).expect("read").snapshot);
        let labelled = |at: i64| {
            let mut snapshot = WorkspaceSnapshot::starter();
            snapshot.graph.nodes[0].label = Some(format!("at {at}"));
            snapshot
        };
        write(&store, id, labelled(0));
        age_secrets(&store);
        collect(&store.hold()).expect("collected");
        assert!(kept(&store, &secret), "undo can still bring it back");

        for at in 1..super::super::history::WORKSPACE_HISTORY_DEPTH {
            write(&store, id, labelled(at));
        }

        assert!(!kept(&store, &secret));
    }

    #[test]
    fn an_old_database_moves_credentials_out_of_the_workspace_and_its_history() {
        let file = tempfile::NamedTempFile::new().expect("temp db");
        let old = serde_json::to_string(&inline(TOKEN)).expect("encode");
        let id = {
            let store = Store::open(Some(file.path())).expect("open");
            let id = store
                .create_workspace("Old", &WorkspaceSnapshot::starter())
                .expect("created");
            let conn = store.hold();
            conn.execute(
                "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
                params![id, old],
            )
            .expect("old row");
            conn.execute(
                "INSERT INTO workspace_history (workspace_id, seq, created_at, snapshot) \
                 VALUES (?1, 99, '2026-01-01T00:00:00Z', ?2)",
                params![id, old],
            )
            .expect("old history");
            conn.execute("DELETE FROM server_meta WHERE key = ?1", [LIFTED_KEY])
                .expect("not lifted yet");
            drop(conn);
            store.create_secret("stray").expect("stray");
            age_secrets(&store);
            id
        };

        let store = Store::open(Some(file.path())).expect("reopen");

        let rows: Vec<String> = store
            .hold()
            .prepare(
                "SELECT snapshot FROM workspaces WHERE id = ?1 \
                 UNION ALL SELECT snapshot FROM workspace_history WHERE workspace_id = ?1",
            )
            .expect("query")
            .query_map([id], |row| row.get(0))
            .expect("rows")
            .collect::<Result<_, _>>()
            .expect("read");
        assert!(rows.iter().all(|row| !row.contains(TOKEN)));
        let ids: Vec<String> = rows
            .iter()
            .map(|row| stored_id(&serde_json::from_str(row).expect("snapshot")))
            .collect();
        assert!(ids.windows(2).all(|pair| pair[0] == pair[1]), "{ids:?}");
        assert_eq!(store.secret(&ids[0]).expect("read").as_deref(), Some(TOKEN));
        let left: i64 = store
            .hold()
            .query_row("SELECT COUNT(*) FROM secrets", [], |row| row.get(0))
            .expect("count");
        assert_eq!(left, 1, "no secret is left that nothing names");
    }
}
