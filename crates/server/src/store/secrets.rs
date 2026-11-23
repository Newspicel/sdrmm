use rusqlite::{Connection, OptionalExtension, params};

use super::{Store, StoreError, now_rfc3339};

impl Store {
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

    pub(super) fn secrets_changed(&self) {
        self.secret_changes
            .send_modify(|count| *count = count.wrapping_add(1));
    }
}

/// Drops every secret no account, workspace or history entry names. A reference is the id as a
/// JSON string anywhere in the row, so a snapshot this server can no longer read still keeps
/// its credentials.
pub(super) fn collect(conn: &Connection) -> Result<usize, StoreError> {
    let removed = conn.execute(
        "DELETE FROM secrets WHERE \
         NOT EXISTS (SELECT 1 FROM accounts \
             WHERE instr(body, '\"' || secrets.id || '\"') > 0) \
         AND NOT EXISTS (SELECT 1 FROM workspaces \
             WHERE instr(snapshot, '\"' || secrets.id || '\"') > 0) \
         AND NOT EXISTS (SELECT 1 FROM workspace_history \
             WHERE instr(snapshot, '\"' || secrets.id || '\"') > 0)",
        [],
    )?;
    if removed > 0 {
        tracing::info!(removed, "dropped secrets nothing names");
    }
    Ok(removed)
}

pub(super) fn put(conn: &Connection, id: &str, value: &str) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO secrets (id, value, updated_at) VALUES (?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET value = ?2, updated_at = ?3",
        params![id, value, now_rfc3339()],
    )?;
    Ok(())
}

pub(super) fn remove(conn: &Connection, id: &str) -> Result<(), StoreError> {
    conn.execute("DELETE FROM secrets WHERE id = ?1", params![id])?;
    Ok(())
}

pub(super) fn insert(conn: &Connection, value: &str) -> Result<String, StoreError> {
    Ok(conn.query_row(
        "INSERT INTO secrets (id, value, updated_at) \
         VALUES (lower(hex(randomblob(16))), ?1, ?2) RETURNING id",
        params![value, now_rfc3339()],
        |row| row.get(0),
    )?)
}

#[cfg(test)]
mod tests {
    use sdrmm_wire::WorkspaceSnapshot;

    use super::*;

    fn kept(store: &Store, id: &str) -> bool {
        store.secret(id).expect("read").is_some()
    }

    #[test]
    fn a_secret_nothing_names_is_dropped() {
        let store = Store::open(None).expect("store");
        let stray = insert(&store.hold(), "stray").expect("kept");

        collect(&store.hold()).expect("collected");

        assert!(!kept(&store, &stray));
    }

    #[test]
    fn a_secret_an_unread_snapshot_still_names_is_kept() {
        let store = Store::open(None).expect("store");
        let id = store
            .create_workspace("Old", &WorkspaceSnapshot::starter())
            .expect("created");
        let conn = store.hold();
        let named = insert(&conn, "old").expect("kept");
        conn.execute(
            "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
            params![id, format!(r#"{{"unreadable":{{"secret":"{named}"}}}}"#)],
        )
        .expect("old row");

        collect(&conn).expect("collected");
        drop(conn);

        assert!(kept(&store, &named));
    }
}
