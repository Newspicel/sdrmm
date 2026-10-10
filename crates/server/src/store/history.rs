use rusqlite::{Connection, OptionalExtension, params};
use sdrmm_wire::{WorkspaceHistory, WorkspaceSnapshot, WorkspaceState};

use super::{StoreError, now_rfc3339, parse_workspace_snapshot};
use crate::merge::merge_typed;

pub(super) const WORKSPACE_HISTORY_DEPTH: i64 = 100;

const HISTORY_COALESCE: jiff::SignedDuration = jiff::SignedDuration::from_secs(1);

const EDIT: &str = "edit";
const BASE: &str = "base";
const SPENT: &str = "spent";

pub(super) struct RecordedSettings<'a> {
    pub(super) node: &'a str,
    pub(super) before: &'a str,
    pub(super) after: &'a str,
}

#[derive(Clone, Copy)]
pub(super) enum Step {
    Undo,
    Redo,
}

impl Step {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Undo => "undo",
            Self::Redo => "redo",
        }
    }

    fn target(self) -> &'static str {
        match self {
            Self::Undo => {
                "SELECT seq, snapshot FROM workspace_history \
                 WHERE workspace_id = ?1 AND author = ?2 AND kind = 'edit' AND undone_by IS NULL \
                 AND seq > (SELECT MIN(seq) FROM workspace_history WHERE workspace_id = ?1) \
                 ORDER BY seq DESC LIMIT 1"
            }
            Self::Redo => {
                "SELECT seq, snapshot FROM workspace_history \
                 WHERE workspace_id = ?1 AND author = ?2 AND kind = 'edit' \
                 AND undone_by IS NOT NULL \
                 AND seq > (SELECT MIN(seq) FROM workspace_history WHERE workspace_id = ?1) \
                 ORDER BY undone_by DESC LIMIT 1"
            }
        }
    }
}

pub(super) struct Reached {
    pub(super) snapshot: WorkspaceSnapshot,
    pub(super) settings: Option<WorkspaceState>,
    target: i64,
}

struct Head {
    seq: i64,
    snapshot: String,
    node: Option<String>,
    author: Option<String>,
    kind: String,
    created_at: String,
}

pub(super) fn record(
    conn: &Connection,
    id: i64,
    json: &str,
    settings: Option<RecordedSettings<'_>>,
    author: Option<&str>,
    revision: u64,
) -> Result<bool, StoreError> {
    let now = now_rfc3339();
    let head = match head(conn, id)? {
        Some(head) => head,
        None => seed(conn, id, &now)?,
    };
    if let Some(settings) = &settings {
        conn.execute(
            "UPDATE workspace_history SET state = ?3 \
             WHERE workspace_id = ?1 AND seq = ?2 AND state IS NULL",
            params![id, head.seq, settings.before],
        )?;
    }
    if head.snapshot == json
        && match &settings {
            None => true,
            Some(settings) => state_at(conn, id, head.seq)?.as_deref() == Some(settings.after),
        }
    {
        return Ok(false);
    }
    if let Some(settings) = &settings
        && coalesces(&head, settings, author, json, &now)
    {
        conn.execute(
            "UPDATE workspace_history SET state = ?3, created_at = ?4 \
             WHERE workspace_id = ?1 AND seq = ?2",
            params![id, head.seq, settings.after, now],
        )?;
        return Ok(false);
    }
    if let Some(author) = author {
        conn.execute(
            "UPDATE workspace_history SET kind = ?3 \
             WHERE workspace_id = ?1 AND author = ?2 AND kind = ?4 AND undone_by IS NOT NULL",
            params![id, author, SPENT, EDIT],
        )?;
    }
    append(
        conn,
        id,
        &Appended {
            seq: head.seq + 1,
            kind: EDIT,
            author,
            json,
            state: settings.as_ref().map(|settings| settings.after),
            node: settings.as_ref().map(|settings| settings.node),
            revision,
            now: &now,
        },
    )?;
    Ok(true)
}

fn coalesces(
    head: &Head,
    settings: &RecordedSettings<'_>,
    author: Option<&str>,
    json: &str,
    now: &str,
) -> bool {
    head.kind == EDIT
        && head.author.as_deref() == author
        && head.node.as_deref() == Some(settings.node)
        && head.snapshot == json
        && within_coalesce(&head.created_at, now)
}

pub(super) fn reach(
    conn: &Connection,
    id: i64,
    author: &str,
    step: Step,
    held: &WorkspaceSnapshot,
    saved: &WorkspaceState,
) -> Result<Option<Reached>, StoreError> {
    let target: Option<(i64, String)> = conn
        .query_row(step.target(), params![id, author], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .optional()?;
    let Some((seq, after)) = target else {
        return Ok(None);
    };
    let before: Option<(i64, String)> = conn
        .query_row(
            "SELECT seq, snapshot FROM workspace_history \
             WHERE workspace_id = ?1 AND seq < ?2 ORDER BY seq DESC LIMIT 1",
            params![id, seq],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let Some((prior, before)) = before else {
        return Ok(None);
    };
    let (from, to) = match step {
        Step::Undo => (after, before),
        Step::Redo => (before, after),
    };
    let snapshot = if from == to {
        held.clone()
    } else {
        let mut merged = merge_typed(
            &parse_workspace_snapshot(&from)?,
            &parse_workspace_snapshot(&to)?,
            held,
        )?;
        merged.drop_dangling();
        merged.validate()?;
        merged
    };
    let (after_state, before_state) = (state_at(conn, id, seq)?, state_at(conn, id, prior)?);
    let (from_state, to_state) = match step {
        Step::Undo => (after_state, before_state),
        Step::Redo => (before_state, after_state),
    };
    let settings = match (from_state, to_state) {
        (Some(from), Some(to)) if from != to => {
            Some(merge_typed(&parse_state(&from)?, &parse_state(&to)?, saved)?.current())
        }
        _ => None,
    };
    Ok(Some(Reached {
        snapshot,
        settings,
        target: seq,
    }))
}

pub(super) fn record_step(
    conn: &Connection,
    id: i64,
    author: &str,
    step: Step,
    reached: &Reached,
    revision: u64,
) -> Result<(), StoreError> {
    let now = now_rfc3339();
    let Some(head) = head(conn, id)? else {
        return Ok(());
    };
    let json = serde_json::to_string(&reached.snapshot)?;
    let state = reached
        .settings
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let seq = head.seq + 1;
    append(
        conn,
        id,
        &Appended {
            seq,
            kind: step.name(),
            author: Some(author),
            json: &json,
            state: state.as_deref(),
            node: None,
            revision,
            now: &now,
        },
    )?;
    let undone_by = match step {
        Step::Undo => Some(seq),
        Step::Redo => None,
    };
    conn.execute(
        "UPDATE workspace_history SET undone_by = ?3 WHERE workspace_id = ?1 AND seq = ?2",
        params![id, reached.target, undone_by],
    )?;
    Ok(())
}

pub(super) fn read(
    conn: &Connection,
    id: i64,
    author: Option<&str>,
) -> Result<WorkspaceHistory, StoreError> {
    let Some(author) = author else {
        return Ok(WorkspaceHistory::default());
    };
    let found = |step: Step| -> Result<bool, StoreError> {
        Ok(conn
            .query_row(step.target(), params![id, author], |_| Ok(()))
            .optional()?
            .is_some())
    };
    Ok(WorkspaceHistory {
        can_undo: found(Step::Undo)?,
        can_redo: found(Step::Redo)?,
    })
}

pub(super) fn snapshot_at(
    conn: &Connection,
    id: i64,
    revision: u64,
) -> Result<Option<String>, StoreError> {
    let Ok(revision) = i64::try_from(revision) else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT snapshot FROM workspace_history \
             WHERE workspace_id = ?1 AND revision = ?2 ORDER BY seq DESC LIMIT 1",
            params![id, revision],
            |row| row.get(0),
        )
        .optional()?)
}

fn parse_state(json: &str) -> Result<WorkspaceState, StoreError> {
    Ok(serde_json::from_str::<WorkspaceState>(json)?.current())
}

struct Appended<'a> {
    seq: i64,
    kind: &'a str,
    author: Option<&'a str>,
    json: &'a str,
    state: Option<&'a str>,
    node: Option<&'a str>,
    revision: u64,
    now: &'a str,
}

fn append(conn: &Connection, id: i64, entry: &Appended<'_>) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO workspace_history \
         (workspace_id, seq, created_at, snapshot, state, node, author, kind, revision) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            id,
            entry.seq,
            entry.now,
            entry.json,
            entry.state,
            entry.node,
            entry.author,
            entry.kind,
            i64::try_from(entry.revision).unwrap_or(i64::MAX)
        ],
    )?;
    if entry.kind == EDIT {
        conn.execute(
            "DELETE FROM workspace_history WHERE workspace_id = ?1 AND seq <= ?2",
            params![id, entry.seq - WORKSPACE_HISTORY_DEPTH],
        )?;
    }
    Ok(())
}

fn seed(conn: &Connection, id: i64, now: &str) -> Result<Head, StoreError> {
    let (snapshot, revision): (String, i64) = conn.query_row(
        "SELECT snapshot, revision FROM workspaces WHERE id = ?1",
        params![id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    conn.execute(
        "INSERT INTO workspace_history \
         (workspace_id, seq, created_at, snapshot, kind, revision) \
         VALUES (?1, 1, ?2, ?3, ?4, ?5)",
        params![id, now, snapshot, BASE, revision],
    )?;
    Ok(Head {
        seq: 1,
        snapshot,
        node: None,
        author: None,
        kind: BASE.to_owned(),
        created_at: now.to_owned(),
    })
}

fn head(conn: &Connection, id: i64) -> Result<Option<Head>, StoreError> {
    Ok(conn
        .query_row(
            "SELECT seq, snapshot, node, author, kind, created_at FROM workspace_history \
             WHERE workspace_id = ?1 ORDER BY seq DESC LIMIT 1",
            params![id],
            |row| {
                Ok(Head {
                    seq: row.get(0)?,
                    snapshot: row.get(1)?,
                    node: row.get(2)?,
                    author: row.get(3)?,
                    kind: row.get(4)?,
                    created_at: row.get(5)?,
                })
            },
        )
        .optional()?)
}

pub(super) fn within_coalesce(entry: &str, now: &str) -> bool {
    let (Ok(entry), Ok(now)) = (
        entry.parse::<jiff::Timestamp>(),
        now.parse::<jiff::Timestamp>(),
    ) else {
        return false;
    };
    let gap = now.duration_since(entry);
    gap >= jiff::SignedDuration::ZERO && gap <= HISTORY_COALESCE
}

fn state_at(conn: &Connection, id: i64, seq: i64) -> Result<Option<String>, StoreError> {
    let behind: Option<String> = conn
        .query_row(
            "SELECT state FROM workspace_history \
             WHERE workspace_id = ?1 AND seq <= ?2 AND state IS NOT NULL \
             ORDER BY seq DESC LIMIT 1",
            params![id, seq],
            |row| row.get(0),
        )
        .optional()?;
    if behind.is_some() {
        return Ok(behind);
    }
    Ok(conn
        .query_row(
            "SELECT state FROM workspace_history \
             WHERE workspace_id = ?1 AND seq > ?2 AND state IS NOT NULL \
             ORDER BY seq ASC LIMIT 1",
            params![id, seq],
            |row| row.get(0),
        )
        .optional()?)
}
