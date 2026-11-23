use std::{collections::HashMap, convert::Infallible};

use rusqlite::{Connection, OptionalExtension, params};
use sdrmm_wire::{
    Account, AccountService, MAX_ACCOUNT_LEN, MatrixLogin, NodeBody, NodeKind, Secret,
    WorkspaceSnapshot,
};
use serde_json::{Map, Value, json};

use super::{StoreError, accounts, migrate_event_outputs, steps::graph_nodes};

const LIFTED_KEY: &str = "step:output_accounts";
const PLACEHOLDER: &str = "legacy-";

/// The logins old output nodes carried in an imported document, each standing in the document
/// under a placeholder account id until the import finds or makes its account.
#[derive(Debug, Default)]
pub(crate) struct LegacyLogins {
    logins: Vec<AccountService>,
}

impl LegacyLogins {
    /// Moves every login an old output in `document` carries out of its node. A credential the
    /// document names only by id stays behind: an import never reaches into this server's
    /// secrets.
    pub(crate) fn take(document: &mut Value) -> Self {
        let mut taken = Self::default();
        if let Some(snapshot) = document.get_mut("snapshot") {
            let Ok(()) = lift_snapshot::<Infallible>(snapshot, |service| {
                Ok(taken.placeholder(without_stored(service)))
            });
        }
        taken
    }

    fn placeholder(&mut self, service: AccountService) -> String {
        let index = self
            .logins
            .iter()
            .position(|known| *known == service)
            .unwrap_or_else(|| {
                self.logins.push(service);
                self.logins.len() - 1
            });
        format!("{PLACEHOLDER}{index}")
    }

    /// Points every output that names a placeholder at its account, found or made in `conn`.
    pub(super) fn settle(
        &self,
        conn: &Connection,
        snapshot: &mut WorkspaceSnapshot,
    ) -> Result<(), StoreError> {
        if self.logins.is_empty() {
            return Ok(());
        }
        let mut made = Made::default();
        let mut ids = Vec::with_capacity(self.logins.len());
        for login in &self.logins {
            ids.push(made.account_for(conn, login.clone())?);
        }
        for node in &mut snapshot.graph.nodes {
            let NodeBody::EventOutput(output) = &mut node.body else {
                continue;
            };
            let Some(account) = output.target.account_mut() else {
                continue;
            };
            if let Some(id) = account
                .strip_prefix(PLACEHOLDER)
                .and_then(|index| index.parse::<usize>().ok())
                .and_then(|index| ids.get(index))
            {
                account.clone_from(id);
            }
        }
        Ok(())
    }
}

/// Older servers kept each output's login inside the workspace and its history. Every one moves
/// into an account once; outputs that shared a login share the account afterwards.
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
    let mut made = Made::default();
    for (table, key) in [("workspaces", "id"), ("workspace_history", "rowid")] {
        let rows: Vec<(i64, String)> = tx
            .prepare(&format!("SELECT {key}, snapshot FROM {table}"))?
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (row, json) in rows {
            let Ok(stored) = serde_json::from_str::<Value>(&json) else {
                continue;
            };
            let mut snapshot = stored.clone();
            migrate_event_outputs(&mut snapshot);
            lift_snapshot(&mut snapshot, |service| {
                made.account_for(&tx, revealed(&tx, service)?)
            })?;
            if snapshot != stored {
                tx.execute(
                    &format!("UPDATE {table} SET snapshot = ?2 WHERE {key} = ?1"),
                    params![row, serde_json::to_string(&snapshot)?],
                )?;
            }
        }
    }
    tx.execute(
        "INSERT INTO server_meta (key, value) VALUES (?1, 'done')",
        params![LIFTED_KEY],
    )?;
    tx.commit()?;
    Ok(())
}

/// Rewrites each old output in `snapshot` to name an account, asking `account` for the id of
/// the login it carried. An output that carried no login at all names none.
fn lift_snapshot<E>(
    snapshot: &mut Value,
    mut account: impl FnMut(AccountService) -> Result<String, E>,
) -> Result<(), E> {
    for node in graph_nodes(snapshot) {
        if node.get("kind").and_then(Value::as_str) != Some(NodeKind::EventOutput.as_str()) {
            continue;
        }
        let Some(target) = node.get_mut("data").and_then(|data| data.get_mut("target")) else {
            continue;
        };
        let Some((service, mut kept)) = split(target) else {
            continue;
        };
        let id = if carries_login(&service) {
            account(service)?
        } else {
            String::new()
        };
        kept.insert("account".to_owned(), json!(id));
        *target = Value::Object(kept);
    }
    Ok(())
}

/// The login an old target carried and the rest of the target, the place inside the service.
/// A target that names an account already, or never signs in, gives none.
fn split(target: &Value) -> Option<(AccountService, Map<String, Value>)> {
    let fields = target.as_object()?;
    if fields.contains_key("account") {
        return None;
    }
    let text = |key: &str| {
        fields
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let secret = |key: &str| {
        fields
            .get(key)
            .and_then(|value| serde_json::from_value::<Secret>(value.clone()).ok())
            .unwrap_or_default()
    };
    let service_name = text("service");
    let (service, place) = match service_name.as_str() {
        "webhook" => (AccountService::Webhook { url: secret("url") }, "format"),
        "matrix" => (
            AccountService::Matrix {
                homeserver_url: text("homeserver_url"),
                user: String::new(),
                login: MatrixLogin::Token,
                access_token: secret("access_token"),
                password: Secret::default(),
                nickname: String::new(),
                recovery_key: Secret::default(),
                session: Secret::default(),
                store_key: Secret::default(),
            },
            "room_id",
        ),
        "mqtt" => (
            AccountService::Mqtt {
                broker_url: text("broker_url"),
                username: text("username"),
                password: secret("password"),
            },
            "topic",
        ),
        "postgres" => (
            AccountService::Postgres {
                url: text("url"),
                username: text("username"),
                password: secret("password"),
            },
            "table",
        ),
        "influx" => (
            AccountService::Influx {
                url: text("url"),
                org: text("org"),
                token: secret("token"),
            },
            "bucket",
        ),
        _ => return None,
    };
    let mut kept = Map::new();
    kept.insert("service".to_owned(), json!(service_name));
    if let Some(value) = fields.get(place) {
        kept.insert(place.to_owned(), value.clone());
    }
    Some((service, kept))
}

fn carries_login(service: &AccountService) -> bool {
    let settings = match service {
        AccountService::Matrix { homeserver_url, .. } => vec![homeserver_url],
        AccountService::Mqtt {
            broker_url,
            username,
            ..
        } => vec![broker_url, username],
        AccountService::Postgres { url, username, .. } => vec![url, username],
        AccountService::Influx { url, org, .. } => vec![url, org],
        AccountService::Webhook { .. }
        | AccountService::Discord { .. }
        | AccountService::Teamspeak { .. } => Vec::new(),
    };
    settings.iter().any(|value| !value.trim().is_empty())
        || service.credentials().iter().any(|secret| secret.is_set())
}

fn without_stored(mut service: AccountService) -> AccountService {
    for (secret, _) in service.credentials_mut() {
        if matches!(secret, Secret::Stored { .. }) {
            *secret = Secret::default();
        }
    }
    service
}

/// `service` with every credential it names by id read back in place, unset where this server
/// does not hold it.
fn revealed(conn: &Connection, mut service: AccountService) -> Result<AccountService, StoreError> {
    for (secret, _) in service.credentials_mut() {
        if let Secret::Stored { secret: id } = secret {
            let value: Option<String> = conn
                .query_row(
                    "SELECT value FROM secrets WHERE id = ?1",
                    params![id.as_str()],
                    |row| row.get(0),
                )
                .optional()?;
            *secret = Secret::Inline(value.unwrap_or_default());
        }
    }
    Ok(service)
}

/// The accounts one lift has found or made, by the login they hold.
#[derive(Default)]
struct Made {
    ids: HashMap<String, String>,
}

impl Made {
    /// The account holding `login`, whose credentials are all in place: an existing account
    /// with the same settings and values, else a new one named after the service and its host.
    fn account_for(
        &mut self,
        conn: &Connection,
        login: AccountService,
    ) -> Result<String, StoreError> {
        let key = serde_json::to_string(&login)?;
        if let Some(id) = self.ids.get(&key) {
            return Ok(id.clone());
        }
        let id = match existing(conn, &login)? {
            Some(id) => id,
            None => accounts::create(conn, &name_of(&login), login)?.id,
        };
        self.ids.insert(key, id.clone());
        Ok(id)
    }
}

fn existing(conn: &Connection, login: &AccountService) -> Result<Option<String>, StoreError> {
    for account in accounts::list(conn)? {
        if account.service.kind() != login.kind() {
            continue;
        }
        let Account { id, service, .. } = account;
        if revealed(conn, service)? == *login {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// "Matrix matrix.example": the service, then the host its login reaches.
fn name_of(login: &AccountService) -> String {
    let address = match login {
        AccountService::Webhook { url } => url.inline().unwrap_or_default(),
        AccountService::Matrix { homeserver_url, .. } => homeserver_url,
        AccountService::Mqtt { broker_url, .. } => broker_url,
        AccountService::Postgres { url, .. } | AccountService::Influx { url, .. } => url,
        AccountService::Discord { .. } | AccountService::Teamspeak { .. } => "",
    };
    let label = login.kind().label();
    let name = url::Url::parse(address)
        .ok()
        .and_then(|url| url.host_str().map(|host| format!("{label} {host}")))
        .unwrap_or_else(|| label.to_owned());
    name.chars().take(MAX_ACCOUNT_LEN).collect()
}

#[cfg(test)]
mod tests;
