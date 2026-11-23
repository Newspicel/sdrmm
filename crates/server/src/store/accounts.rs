use rusqlite::{Connection, OptionalExtension, params};
use sdrmm_wire::{Account, AccountService, MatrixLogin, Secret};

use super::{Store, StoreError, now_rfc3339, secrets};

impl Store {
    pub fn accounts(&self) -> Result<Vec<Account>, StoreError> {
        list(&self.lock())
    }

    pub fn account(&self, id: &str) -> Result<Option<Account>, StoreError> {
        match read(&self.lock(), id) {
            Ok(account) => Ok(Some(account)),
            Err(StoreError::AccountNotFound(_)) => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Keeps the account under a fresh id. Credentials sent in place move into the secrets
    /// store, and a `TeamSpeak` account without an identity gets a new one.
    pub fn create_account(&self, request: Account) -> Result<Account, StoreError> {
        check(&request)?;
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let account = create(&tx, request.name.trim(), request.service)?;
        tx.commit()?;
        drop(conn);
        self.accounts_changed();
        self.secrets_changed();
        Ok(account)
    }

    /// Replaces the account. A credential the request leaves stored or empty stays as it is, one
    /// sent in place replaces it. An optional credential sent empty is cleared.
    pub fn update_account(&self, id: &str, request: Account) -> Result<Account, StoreError> {
        check(&request)?;
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let current = read(&tx, id)?;
        let account = Account {
            service: settle_service(&tx, request.service, Some(&current.service))?,
            id: current.id.clone(),
            name: request.name.trim().to_owned(),
        };
        write(&tx, &account, false)?;
        drop_unused(&tx, &current.service, &account.service)?;
        tx.commit()?;
        drop(conn);
        self.accounts_changed();
        self.secrets_changed();
        Ok(account)
    }

    /// Keeps `session` as the Matrix account's saved sign-in.
    pub fn set_account_session(&self, id: &str, session: &str) -> Result<(), StoreError> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let mut account = read(&tx, id)?;
        let AccountService::Matrix { session: held, .. } = &mut account.service else {
            return Err(StoreError::AccountField("service"));
        };
        if let Secret::Stored { secret } = held {
            secrets::put(&tx, secret, session)?;
        } else {
            *held = Secret::stored(secrets::insert(&tx, session)?);
            write(&tx, &account, false)?;
        }
        tx.commit()?;
        drop(conn);
        self.accounts_changed();
        self.secrets_changed();
        Ok(())
    }

    pub fn delete_account(&self, id: &str) -> Result<(), StoreError> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let current = read(&tx, id)?;
        tx.execute("DELETE FROM accounts WHERE id = ?1", params![id])?;
        for secret in secret_ids(&current.service) {
            secrets::remove(&tx, secret)?;
        }
        tx.commit()?;
        drop(conn);
        self.accounts_changed();
        self.secrets_changed();
        Ok(())
    }

    pub(crate) fn subscribe_accounts(&self) -> tokio::sync::watch::Receiver<u64> {
        self.account_changes.subscribe()
    }

    pub(super) fn accounts_changed(&self) {
        self.account_changes
            .send_modify(|count| *count = count.wrapping_add(1));
    }
}

fn check(account: &Account) -> Result<(), StoreError> {
    account.check().map_err(StoreError::AccountField)
}

pub(super) fn list(conn: &Connection) -> Result<Vec<Account>, StoreError> {
    let mut rows = conn.prepare("SELECT body FROM accounts ORDER BY rowid")?;
    let bodies: Vec<String> = rows
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;
    bodies
        .iter()
        .map(|body| Ok(serde_json::from_str(body)?))
        .collect()
}

/// Keeps a new account under a fresh id, its credentials moved into the secrets store.
pub(super) fn create(
    conn: &Connection,
    name: &str,
    service: AccountService,
) -> Result<Account, StoreError> {
    let id: String = conn.query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))?;
    let account = Account {
        service: settle_service(conn, service, None)?,
        id,
        name: name.to_owned(),
    };
    write(conn, &account, true)?;
    Ok(account)
}

fn read(conn: &Connection, id: &str) -> Result<Account, StoreError> {
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM accounts WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    let body = body.ok_or_else(|| StoreError::AccountNotFound(id.to_owned()))?;
    Ok(serde_json::from_str(&body)?)
}

fn write(conn: &Connection, account: &Account, new: bool) -> Result<(), StoreError> {
    let body = serde_json::to_string(account)?;
    let sql = if new {
        "INSERT INTO accounts (body, updated_at, id) VALUES (?1, ?2, ?3)"
    } else {
        "UPDATE accounts SET body = ?1, updated_at = ?2 WHERE id = ?3"
    };
    conn.execute(sql, params![body, now_rfc3339(), account.id])?;
    Ok(())
}

fn secret_ids(service: &AccountService) -> Vec<&str> {
    service
        .credentials()
        .into_iter()
        .chain(service.managed())
        .filter_map(|secret| match secret {
            Secret::Stored { secret } => Some(secret.as_str()),
            Secret::Inline(_) => None,
        })
        .collect()
}

fn drop_unused(
    conn: &Connection,
    before: &AccountService,
    after: &AccountService,
) -> Result<(), StoreError> {
    let kept = secret_ids(after);
    for secret in secret_ids(before) {
        if !kept.contains(&secret) {
            secrets::remove(conn, secret)?;
        }
    }
    Ok(())
}

fn settle(
    conn: &Connection,
    incoming: &Secret,
    current: Option<&Secret>,
) -> Result<Secret, StoreError> {
    match (incoming.inline(), current) {
        (Some(value), Some(Secret::Stored { secret })) => {
            secrets::put(conn, secret, value)?;
            Ok(Secret::stored(secret))
        }
        (Some(value), _) => Ok(Secret::stored(secrets::insert(conn, value)?)),
        (None, Some(current)) if current.is_set() => Ok(current.clone()),
        (None, _) => Ok(Secret::default()),
    }
}

/// Settles each credential against the one the account holds in the same place. A stored id the
/// request names is never trusted: only the account's own ids survive.
fn settle_service(
    conn: &Connection,
    mut incoming: AccountService,
    current: Option<&AccountService>,
) -> Result<AccountService, StoreError> {
    let held: Vec<Secret> = current
        .filter(|current| current.kind() == incoming.kind())
        .map_or_default(|current| current.credentials().into_iter().cloned().collect());
    for (index, (secret, optional)) in incoming.credentials_mut().into_iter().enumerate() {
        let cleared = optional && matches!(secret, Secret::Inline(value) if value.is_empty());
        *secret = if cleared {
            Secret::default()
        } else {
            settle(conn, secret, held.get(index))?
        };
    }
    if let AccountService::Teamspeak {
        address, identity, ..
    } = &mut incoming
    {
        *address = address.trim().to_owned();
        if !identity.is_set() {
            let fresh = sdrmm_voicechat::new_teamspeak_identity();
            *identity = Secret::stored(secrets::insert(conn, &fresh)?);
        }
    }
    settle_matrix(conn, &mut incoming, current)?;
    Ok(incoming)
}

/// Keeps the account's own session while it still signs in as the same user the same way, and
/// its store key for good. A login with a device of its own gets a fresh key.
fn settle_matrix(
    conn: &Connection,
    incoming: &mut AccountService,
    current: Option<&AccountService>,
) -> Result<(), StoreError> {
    let AccountService::Matrix {
        homeserver_url,
        user,
        login,
        session,
        store_key,
        ..
    } = incoming
    else {
        return Ok(());
    };
    let held = match current {
        Some(AccountService::Matrix {
            homeserver_url: held_url,
            user: held_user,
            login: held_login,
            session: held_session,
            store_key: held_key,
            ..
        }) => Some((
            held_url == homeserver_url && held_user == user && held_login == login,
            held_session,
            held_key,
        )),
        _ => None,
    };
    *session = held
        .filter(|(same, ..)| *same)
        .map_or_default(|(_, session, _)| session.clone());
    *store_key = held.map_or_default(|(.., key)| key.clone());
    if !store_key.is_set() && *login != MatrixLogin::Token {
        let fresh: String =
            conn.query_row("SELECT lower(hex(randomblob(32)))", [], |row| row.get(0))?;
        *store_key = Secret::stored(secrets::insert(conn, &fresh)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
