use rusqlite::{Row, params, types::Type};
use sdrmm_wire::UserRole;

use super::{Store, StoreError, rfc3339};

const COLUMNS: &str = "id_sha256, subject, name, email, role, id_token, expires_at";

/// A signed-in browser. Only the hash of its cookie is kept, so a copy of the database signs
/// no one in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionRow {
    pub(crate) id_sha256: [u8; 32],
    pub(crate) subject: String,
    pub(crate) name: String,
    pub(crate) email: Option<String>,
    pub(crate) role: UserRole,
    pub(crate) id_token: Option<String>,
    pub(crate) expires_at: jiff::Timestamp,
}

impl Store {
    pub(crate) fn insert_session(&self, row: &SessionRow) -> Result<(), StoreError> {
        self.lock().execute(
            &format!(
                "INSERT OR REPLACE INTO sessions ({COLUMNS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
            ),
            params![
                row.id_sha256.as_slice(),
                row.subject,
                row.name,
                row.email,
                row.role.as_str(),
                row.id_token,
                rfc3339(row.expires_at),
            ],
        )?;
        Ok(())
    }

    pub(crate) fn delete_session(&self, id_sha256: &[u8; 32]) -> Result<(), StoreError> {
        self.lock().execute(
            "DELETE FROM sessions WHERE id_sha256 = ?1",
            params![id_sha256.as_slice()],
        )?;
        Ok(())
    }

    /// Drops the sessions that ran out, then answers the rest.
    pub(crate) fn live_sessions(&self) -> Result<Vec<SessionRow>, StoreError> {
        let conn = self.lock();
        conn.execute(
            "DELETE FROM sessions WHERE expires_at <= ?1",
            params![rfc3339(jiff::Timestamp::now())],
        )?;
        let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM sessions"))?;
        let rows = statement.query_map([], session_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }
}

fn session_row(row: &Row<'_>) -> rusqlite::Result<SessionRow> {
    let id: Vec<u8> = row.get(0)?;
    let id_sha256 = <[u8; 32]>::try_from(id.as_slice())
        .map_err(|error| rusqlite::Error::FromSqlConversionFailure(0, Type::Blob, error.into()))?;
    let role: String = row.get(4)?;
    let role = UserRole::parse(&role).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            4,
            Type::Text,
            format!("unknown role {role}").into(),
        )
    })?;
    let expires: String = row.get(6)?;
    let expires_at = expires.parse().map_err(|error: jiff::Error| {
        rusqlite::Error::FromSqlConversionFailure(6, Type::Text, error.into())
    })?;
    Ok(SessionRow {
        id_sha256,
        subject: row.get(1)?,
        name: row.get(2)?,
        email: row.get(3)?,
        role,
        id_token: row.get(5)?,
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(seed: u8, expires_at: jiff::Timestamp) -> SessionRow {
        SessionRow {
            id_sha256: [seed; 32],
            subject: format!("user-{seed}"),
            name: "Ada".into(),
            email: Some("ada@example.org".into()),
            role: UserRole::Operator,
            id_token: None,
            expires_at,
        }
    }

    #[test]
    fn a_session_survives_until_it_runs_out() {
        let store = Store::open(None).expect("store");
        let later = jiff::Timestamp::now() + jiff::SignedDuration::from_hours(1);
        let earlier = jiff::Timestamp::now() - jiff::SignedDuration::from_hours(1);
        store.insert_session(&row(1, later)).expect("insert");
        store.insert_session(&row(2, earlier)).expect("insert");
        let live = store.live_sessions().expect("live");
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].subject, "user-1");
        assert_eq!(live[0].role, UserRole::Operator);
    }

    #[test]
    fn a_deleted_session_is_gone() {
        let store = Store::open(None).expect("store");
        let later = jiff::Timestamp::now() + jiff::SignedDuration::from_hours(1);
        store.insert_session(&row(3, later)).expect("insert");
        store.delete_session(&[3; 32]).expect("delete");
        assert!(store.live_sessions().expect("live").is_empty());
    }
}
