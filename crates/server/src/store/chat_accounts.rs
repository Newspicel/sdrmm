use rusqlite::{Connection, OptionalExtension, params};
use sdrmm_wire::{ChatAccount, ChatService, MAX_CHAT_ACCOUNT_LEN, event_output::Secret};

use super::{Store, StoreError, now_rfc3339, secrets};

impl Store {
    pub fn chat_accounts(&self) -> Result<Vec<ChatAccount>, StoreError> {
        let conn = self.lock();
        let mut rows = conn.prepare("SELECT body FROM chat_accounts ORDER BY rowid")?;
        let bodies: Vec<String> = rows
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;
        bodies
            .iter()
            .map(|body| Ok(serde_json::from_str(body)?))
            .collect()
    }

    /// Keeps the account under a fresh id. Credentials written in place move into the secrets
    /// store, and a `TeamSpeak` account without an identity gets a new one.
    pub fn create_chat_account(&self, request: ChatAccount) -> Result<ChatAccount, StoreError> {
        check(&request)?;
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let id: String = tx.query_row("SELECT lower(hex(randomblob(8)))", [], |row| row.get(0))?;
        let account = ChatAccount {
            service: settle_service(&tx, request.service, None)?,
            id,
            name: request.name.trim().to_owned(),
        };
        write(&tx, &account, true)?;
        tx.commit()?;
        drop(conn);
        self.chat_accounts_changed();
        self.secrets_changed();
        Ok(account)
    }

    /// Replaces the account. A credential the request leaves stored or empty stays as it is, one
    /// written in place replaces it. The one exception is a `TeamSpeak` password, which is
    /// optional: an empty one clears it.
    pub fn update_chat_account(
        &self,
        id: &str,
        request: ChatAccount,
    ) -> Result<ChatAccount, StoreError> {
        check(&request)?;
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let current = read(&tx, id)?;
        let account = ChatAccount {
            service: settle_service(&tx, request.service, Some(&current.service))?,
            id: current.id.clone(),
            name: request.name.trim().to_owned(),
        };
        write(&tx, &account, false)?;
        drop_unused(&tx, &current.service, &account.service)?;
        tx.commit()?;
        drop(conn);
        self.chat_accounts_changed();
        self.secrets_changed();
        Ok(account)
    }

    pub fn delete_chat_account(&self, id: &str) -> Result<(), StoreError> {
        let conn = self.lock();
        let tx = conn.unchecked_transaction()?;
        let current = read(&tx, id)?;
        tx.execute("DELETE FROM chat_accounts WHERE id = ?1", params![id])?;
        for secret in secret_ids(&current.service) {
            secrets::remove(&tx, secret)?;
        }
        tx.commit()?;
        drop(conn);
        self.chat_accounts_changed();
        self.secrets_changed();
        Ok(())
    }

    pub(crate) fn subscribe_chat_accounts(&self) -> tokio::sync::watch::Receiver<u64> {
        self.chat_account_changes.subscribe()
    }

    fn chat_accounts_changed(&self) {
        self.chat_account_changes
            .send_modify(|count| *count = count.wrapping_add(1));
    }
}

fn check(account: &ChatAccount) -> Result<(), StoreError> {
    let name = account.name.trim();
    if name.is_empty() || name.chars().count() > MAX_CHAT_ACCOUNT_LEN {
        return Err(StoreError::ChatAccountField("name"));
    }
    match &account.service {
        ChatService::Teamspeak {
            address,
            password,
            nickname,
            identity,
        } => {
            if address.trim().is_empty() {
                return Err(StoreError::ChatAccountField("address"));
            }
            if nickname.chars().count() > MAX_CHAT_ACCOUNT_LEN {
                return Err(StoreError::ChatAccountField("nickname"));
            }
            if !password.valid() || !identity.valid() {
                return Err(StoreError::ChatAccountField("secret"));
            }
            Ok(())
        }
        ChatService::Discord { token } => token
            .valid()
            .then_some(())
            .ok_or(StoreError::ChatAccountField("token")),
    }
}

fn read(conn: &Connection, id: &str) -> Result<ChatAccount, StoreError> {
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM chat_accounts WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .optional()?;
    let body = body.ok_or_else(|| StoreError::ChatAccountNotFound(id.to_owned()))?;
    Ok(serde_json::from_str(&body)?)
}

fn write(conn: &Connection, account: &ChatAccount, new: bool) -> Result<(), StoreError> {
    let body = serde_json::to_string(account)?;
    let sql = if new {
        "INSERT INTO chat_accounts (body, updated_at, id) VALUES (?1, ?2, ?3)"
    } else {
        "UPDATE chat_accounts SET body = ?1, updated_at = ?2 WHERE id = ?3"
    };
    conn.execute(sql, params![body, now_rfc3339(), account.id])?;
    Ok(())
}

fn secret_ids(service: &ChatService) -> Vec<&str> {
    let secrets: Vec<&Secret> = match service {
        ChatService::Discord { token } => vec![token],
        ChatService::Teamspeak {
            password, identity, ..
        } => vec![password, identity],
    };
    secrets
        .into_iter()
        .filter_map(|secret| match secret {
            Secret::Stored { secret } => Some(secret.as_str()),
            Secret::Inline(_) => None,
        })
        .collect()
}

fn drop_unused(
    conn: &Connection,
    before: &ChatService,
    after: &ChatService,
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

/// An empty password written in place clears the one held, since a server may drop its
/// password; a stored one stays, as with every other credential.
fn settle_password(
    conn: &Connection,
    incoming: &Secret,
    current: Option<&Secret>,
) -> Result<Secret, StoreError> {
    if matches!(incoming, Secret::Inline(value) if value.is_empty()) {
        return Ok(Secret::default());
    }
    settle(conn, incoming, current)
}

fn settle_service(
    conn: &Connection,
    incoming: ChatService,
    current: Option<&ChatService>,
) -> Result<ChatService, StoreError> {
    match incoming {
        ChatService::Discord { token } => {
            let held = match current {
                Some(ChatService::Discord { token }) => Some(token),
                _ => None,
            };
            Ok(ChatService::Discord {
                token: settle(conn, &token, held)?,
            })
        }
        ChatService::Teamspeak {
            address,
            password,
            nickname,
            identity,
        } => {
            let (held_password, held_identity) = match current {
                Some(ChatService::Teamspeak {
                    password, identity, ..
                }) => (Some(password), Some(identity)),
                _ => (None, None),
            };
            let identity = settle(conn, &identity, held_identity)?;
            let identity = if identity.is_set() {
                identity
            } else {
                let fresh = sdrmm_voicechat::new_teamspeak_identity();
                Secret::stored(secrets::insert(conn, &fresh)?)
            };
            Ok(ChatService::Teamspeak {
                address: address.trim().to_owned(),
                password: settle_password(conn, &password, held_password)?,
                nickname,
                identity,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn teamspeak(password: Secret, identity: Secret) -> ChatAccount {
        ChatAccount {
            id: String::new(),
            name: "Club".to_owned(),
            service: ChatService::Teamspeak {
                address: "ts.example".to_owned(),
                password,
                nickname: "sdr".to_owned(),
                identity,
            },
        }
    }

    fn inline(value: &str) -> Secret {
        Secret::Inline(value.to_owned())
    }

    fn held(store: &Store, secret: &Secret) -> Option<String> {
        let Secret::Stored { secret } = secret else {
            panic!("not stored: {secret:?}");
        };
        store.secret(secret).expect("read")
    }

    fn main_secret_of(account: &ChatAccount) -> Secret {
        match &account.service {
            ChatService::Teamspeak { password, .. } => password.clone(),
            ChatService::Discord { token } => token.clone(),
        }
    }

    fn identity_of(account: &ChatAccount) -> Secret {
        match &account.service {
            ChatService::Teamspeak { identity, .. } => identity.clone(),
            ChatService::Discord { .. } => panic!("a discord account has no identity"),
        }
    }

    #[test]
    fn a_stored_id_sent_by_a_client_never_claims_another_accounts_secret() {
        let store = Store::open(None).expect("store");
        let first = store
            .create_chat_account(teamspeak(inline("one"), Secret::default()))
            .expect("first");
        let claim = main_secret_of(&first);

        let second = store
            .create_chat_account(teamspeak(claim.clone(), Secret::default()))
            .expect("second");
        assert!(!main_secret_of(&second).is_set());

        let updated = store
            .update_chat_account(&second.id, teamspeak(claim, Secret::default()))
            .expect("updated");
        assert!(!main_secret_of(&updated).is_set());

        store.delete_chat_account(&second.id).expect("deleted");
        assert_eq!(
            held(&store, &main_secret_of(&first)).as_deref(),
            Some("one")
        );
    }

    #[test]
    fn an_update_keeps_secrets_it_leaves_stored_and_replaces_inline_ones() {
        let store = Store::open(None).expect("store");
        let made = store
            .create_chat_account(teamspeak(inline("one"), Secret::default()))
            .expect("created");
        let identity = held(&store, &identity_of(&made));

        let kept = store
            .update_chat_account(
                &made.id,
                teamspeak(main_secret_of(&made), Secret::default()),
            )
            .expect("kept");
        assert_eq!(held(&store, &main_secret_of(&kept)).as_deref(), Some("one"));
        assert_eq!(held(&store, &identity_of(&kept)), identity);

        let replaced = store
            .update_chat_account(&made.id, teamspeak(inline("two"), Secret::default()))
            .expect("replaced");
        assert_eq!(main_secret_of(&replaced), main_secret_of(&made));
        assert_eq!(
            held(&store, &main_secret_of(&replaced)).as_deref(),
            Some("two")
        );
    }

    #[test]
    fn an_empty_password_clears_the_stored_one_and_keeps_the_identity() {
        let store = Store::open(None).expect("store");
        let made = store
            .create_chat_account(teamspeak(inline("one"), Secret::default()))
            .expect("created");
        let identity = held(&store, &identity_of(&made));

        let cleared = store
            .update_chat_account(&made.id, teamspeak(Secret::default(), Secret::default()))
            .expect("cleared");

        assert!(!main_secret_of(&cleared).is_set());
        assert!(held(&store, &main_secret_of(&made)).is_none());
        assert_eq!(held(&store, &identity_of(&cleared)), identity);
    }

    #[test]
    fn changing_the_service_drops_the_old_secrets() {
        let store = Store::open(None).expect("store");
        let made = store
            .create_chat_account(teamspeak(inline("one"), Secret::default()))
            .expect("created");
        let discord = ChatAccount {
            id: String::new(),
            name: "Club".to_owned(),
            service: ChatService::Discord {
                token: inline("tok"),
            },
        };

        let changed = store
            .update_chat_account(&made.id, discord)
            .expect("changed");

        assert_eq!(
            held(&store, &main_secret_of(&changed)).as_deref(),
            Some("tok")
        );
        assert!(held(&store, &main_secret_of(&made)).is_none());
        assert!(held(&store, &identity_of(&made)).is_none());
    }

    #[test]
    fn deleting_takes_the_secrets_with_it() {
        let store = Store::open(None).expect("store");
        let made = store
            .create_chat_account(teamspeak(inline("one"), Secret::default()))
            .expect("created");

        store.delete_chat_account(&made.id).expect("deleted");

        assert!(store.chat_accounts().expect("list").is_empty());
        assert!(held(&store, &main_secret_of(&made)).is_none());
        assert!(held(&store, &identity_of(&made)).is_none());
        assert!(matches!(
            store.delete_chat_account(&made.id),
            Err(StoreError::ChatAccountNotFound(_))
        ));
    }

    #[test]
    fn an_aged_account_secret_survives_collection() {
        let store = Store::open(None).expect("store");
        let made = store
            .create_chat_account(teamspeak(inline("one"), Secret::default()))
            .expect("created");
        store
            .hold()
            .execute(
                "UPDATE secrets SET updated_at = '2000-01-01T00:00:00.000000000Z'",
                [],
            )
            .expect("aged");

        secrets::collect(&store.hold()).expect("collected");

        assert_eq!(held(&store, &main_secret_of(&made)).as_deref(), Some("one"));
    }

    #[test]
    fn an_account_needs_a_name_and_a_server() {
        let store = Store::open(None).expect("store");
        let mut nameless = teamspeak(Secret::default(), Secret::default());
        nameless.name = "  ".to_owned();
        assert!(store.create_chat_account(nameless).is_err());
        let mut homeless = teamspeak(Secret::default(), Secret::default());
        if let ChatService::Teamspeak { address, .. } = &mut homeless.service {
            address.clear();
        }
        assert!(store.create_chat_account(homeless).is_err());
    }
}
