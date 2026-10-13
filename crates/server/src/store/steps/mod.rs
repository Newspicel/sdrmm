use rusqlite::{Connection, OptionalExtension, params};
use sdrmm_wire::WorkspaceNoticeKind;
use serde_json::Value;

use super::{StoreError, coherent_break};

mod call_audio;
mod filter_kinds;
mod pictures;
mod refused;
mod steer;
mod tx;
mod watched_lanes;
mod wires;

type Step = fn(&mut Value) -> Vec<WorkspaceNoticeKind>;

const STEPS: &[Step] = &[
    filter_kinds::drop_unknown_filter_kinds,
    steer::steer_from_bearing,
    tx::drop_tx_wires,
    refused::drop_refused_edges,
];

const ONCE: &[(&str, Step)] = &[
    ("step:wire_watched_lanes", watched_lanes::wire_watched_lanes),
    ("step:wire_call_audio", call_audio::wire_call_audio),
    ("step:wire_pictures", pictures::wire_pictures),
];

pub(super) fn run(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    STEPS.iter().flat_map(|step| step(snapshot)).collect()
}

pub(super) fn upgrade(snapshot: &mut Value) -> Vec<WorkspaceNoticeKind> {
    upgrade_with(snapshot, &[])
}

fn upgrade_with(snapshot: &mut Value, once: &[Step]) -> Vec<WorkspaceNoticeKind> {
    super::migrate_legacy(snapshot);
    let mut notices: Vec<WorkspaceNoticeKind> =
        once.iter().flat_map(|step| step(snapshot)).collect();
    notices.extend(run(snapshot));
    notices
}

pub(crate) fn upgrade_import(export: &mut Value) -> Vec<WorkspaceNoticeKind> {
    let mut notices = coherent_break::upgrade_export(export).notices();
    if let Some(snapshot) = export.get_mut("snapshot") {
        let once: Vec<Step> = ONCE.iter().map(|(_, step)| *step).collect();
        notices.extend(upgrade_with(snapshot, &once));
    }
    notices
}

pub(super) fn upgrade_stored(conn: &Connection) -> Result<(), StoreError> {
    let pending = pending_once(conn)?;
    let once: Vec<Step> = pending.iter().map(|(_, step)| *step).collect();
    let rows: Vec<(i64, Option<String>)> = conn
        .prepare("SELECT id, snapshot FROM workspaces ORDER BY id")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1).ok())))?
        .collect::<Result<_, _>>()?;
    for (id, json) in rows {
        let Some(stored) = coherent_break::stored_layout(id, json.as_deref()) else {
            continue;
        };
        let mut snapshot = stored.clone();
        let notices = upgrade_with(&mut snapshot, &once);
        if snapshot == stored {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "UPDATE workspaces SET snapshot = ?2 WHERE id = ?1",
            params![id, serde_json::to_string(&snapshot)?],
        )?;
        coherent_break::insert_notices(&tx, id, &notices)?;
        tx.commit()?;
    }
    for (key, _) in pending {
        conn.execute(
            "INSERT OR IGNORE INTO server_meta (key, value) VALUES (?1, 'done')",
            params![key],
        )?;
    }
    Ok(())
}

fn pending_once(conn: &Connection) -> Result<Vec<(&'static str, Step)>, StoreError> {
    let mut pending = Vec::new();
    for (key, step) in ONCE {
        let done: Option<String> = conn
            .query_row(
                "SELECT value FROM server_meta WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        if done.is_none() {
            pending.push((*key, *step));
        }
    }
    Ok(pending)
}

pub(super) fn graph_nodes(snapshot: &mut Value) -> impl Iterator<Item = &mut Value> {
    snapshot
        .get_mut("graph")
        .and_then(|graph| graph.get_mut("nodes"))
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
}

#[cfg(test)]
mod tests;
