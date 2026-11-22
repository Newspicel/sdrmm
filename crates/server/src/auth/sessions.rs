use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, Instant},
};

use axum::http::{HeaderMap, header};
use sdrmm_wire::{UserRole, phone::hex};
use sha2::{Digest, Sha256};

use crate::store::{SessionRow, Store, StoreError};

pub(crate) const COOKIE: &str = "sdrmm_session";
pub(crate) const TICKET_PREFIX: &str = "ticket-";
const LIFETIME: jiff::SignedDuration = jiff::SignedDuration::from_hours(12);
const TICKET_LIFETIME: Duration = Duration::from_secs(30);
const TICKET_CAP: usize = 256;
const SECRET_BYTES: usize = 32;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionError {
    #[error("no randomness for a session: {0}")]
    Random(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// A person the identity provider vouched for, with the role their groups grant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct User {
    pub(crate) subject: String,
    pub(crate) name: String,
    pub(crate) email: Option<String>,
    pub(crate) role: UserRole,
}

struct Held {
    user: Arc<User>,
    expires_at: jiff::Timestamp,
    id_token: Option<String>,
}

/// Signed-in browsers, kept in memory and in the store so a restart signs no one out.
pub(crate) struct Sessions {
    store: Arc<Store>,
    held: Mutex<HashMap<[u8; 32], Held>>,
    tickets: Mutex<Tickets>,
}

type Tickets = HashMap<[u8; 32], (Arc<User>, Instant)>;

impl Sessions {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        let held = store
            .live_sessions()
            .unwrap_or_else(|error| {
                tracing::warn!(%error, "stored sessions unreadable: everyone signs in again");
                Vec::new()
            })
            .into_iter()
            .map(|row| (row.id_sha256, held_from(row)))
            .collect();
        Self {
            store,
            held: Mutex::new(held),
            tickets: Mutex::new(HashMap::new()),
        }
    }

    /// Starts a session and answers the cookie value that names it.
    pub(crate) fn open(
        &self,
        user: User,
        id_token: Option<String>,
    ) -> Result<String, SessionError> {
        let secret = fresh_secret().map_err(|error| SessionError::Random(error.to_string()))?;
        let row = SessionRow {
            id_sha256: digest(&secret),
            subject: user.subject,
            name: user.name,
            email: user.email,
            role: user.role,
            id_token,
            expires_at: jiff::Timestamp::now() + LIFETIME,
        };
        self.store.insert_session(&row)?;
        lock(&self.held).insert(row.id_sha256, held_from(row));
        Ok(secret)
    }

    pub(crate) fn find(&self, cookie: &str) -> Option<Arc<User>> {
        let key = digest(cookie);
        let mut held = lock(&self.held);
        let session = held.get(&key)?;
        if session.expires_at <= jiff::Timestamp::now() {
            held.remove(&key);
            return None;
        }
        Some(session.user.clone())
    }

    /// Ends a session and answers the ID token it was opened with, the hint a provider's logout
    /// page takes.
    pub(crate) fn close(&self, cookie: &str) -> Result<Option<String>, StoreError> {
        let key = digest(cookie);
        let removed = lock(&self.held).remove(&key);
        self.store.delete_session(&key)?;
        Ok(removed.and_then(|held| held.id_token))
    }

    pub(crate) fn ticket(&self, user: Arc<User>) -> Option<String> {
        let secret = fresh_secret()
            .map_err(|error| tracing::warn!(%error, "no randomness for a WebTransport ticket"))
            .ok()?;
        let mut tickets = lock(&self.tickets);
        let now = Instant::now();
        tickets.retain(|_, (_, minted)| now.duration_since(*minted) < TICKET_LIFETIME);
        if tickets.len() >= TICKET_CAP {
            tracing::warn!("too many WebTransport tickets waiting; refusing another");
            return None;
        }
        tickets.insert(digest(&secret), (user, now));
        Some(format!("{TICKET_PREFIX}{secret}"))
    }

    /// Takes a ticket once.
    pub(crate) fn redeem(&self, ticket: &str) -> Option<Arc<User>> {
        let secret = ticket.strip_prefix(TICKET_PREFIX)?;
        let (user, minted) = lock(&self.tickets).remove(&digest(secret))?;
        (minted.elapsed() < TICKET_LIFETIME).then_some(user)
    }
}

fn held_from(row: SessionRow) -> Held {
    Held {
        user: Arc::new(User {
            subject: row.subject,
            name: row.name,
            email: row.email,
            role: row.role,
        }),
        expires_at: row.expires_at,
        id_token: row.id_token,
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn fresh_secret() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; SECRET_BYTES];
    getrandom::fill(&mut bytes)?;
    Ok(hex(&bytes))
}

fn digest(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

/// The value of the cookie `name`, from any `Cookie` header.
pub(crate) fn cookie<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| value)
}

pub(crate) fn set_cookie(name: &str, value: &str, max_age: Duration, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!(
        "{name}={value}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{secure}",
        max_age.as_secs()
    )
}

pub(crate) fn clear_cookie(name: &str, secure: bool) -> String {
    set_cookie(name, "", Duration::ZERO, secure)
}

pub(crate) fn session_max_age() -> Duration {
    LIFETIME.unsigned_abs()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn ada() -> User {
        User {
            subject: "ada".into(),
            name: "Ada".into(),
            email: None,
            role: UserRole::Viewer,
        }
    }

    #[test]
    fn an_opened_session_is_found_by_its_cookie_and_survives_a_restart() {
        let store = Arc::new(Store::open(None).expect("store"));
        let cookie = Sessions::new(store.clone())
            .open(ada(), None)
            .expect("open");
        let user = Sessions::new(store).find(&cookie).expect("found");
        assert_eq!(*user, ada());
    }

    #[test]
    fn a_closed_session_is_gone_and_hands_back_its_id_token() {
        let sessions = Sessions::new(Arc::new(Store::open(None).expect("store")));
        let cookie = sessions.open(ada(), Some("jwt".into())).expect("open");
        assert_eq!(
            sessions.close(&cookie).expect("close").as_deref(),
            Some("jwt")
        );
        assert!(sessions.find(&cookie).is_none());
    }

    #[test]
    fn a_ticket_is_redeemed_once() {
        let sessions = Sessions::new(Arc::new(Store::open(None).expect("store")));
        let ticket = sessions.ticket(Arc::new(ada())).expect("ticket");
        assert!(sessions.redeem(&ticket).is_some());
        assert!(sessions.redeem(&ticket).is_none());
        assert!(sessions.redeem("ticket-nonsense").is_none());
    }

    #[test]
    fn the_cookie_is_read_from_among_others() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; sdrmm_session=abc; b=2"),
        );
        assert_eq!(cookie(&headers, COOKIE), Some("abc"));
        assert_eq!(cookie(&headers, "missing"), None);
    }
}
