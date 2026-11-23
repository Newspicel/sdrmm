//! Matrix voice: `MatrixRTC` calls carried over `LiveKit`.

mod account;
mod call;
mod e2ee;
mod focus;
mod livekit;
mod membership;
mod sign_in;

use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;

pub(crate) use account::Matrix;
pub use sign_in::{SignIn, SignedIn, matrix_sign_in};

/// A Matrix bot account.
#[derive(Clone, PartialEq, Eq)]
pub struct MatrixAccount {
    /// A homeserver URL, or a server name found through `.well-known`.
    pub homeserver: String,
    /// `@name:server`.
    pub user: String,
    pub login: MatrixLogin,
    /// Empty keeps the account's own display name.
    pub nickname: String,
    /// Unlocks cross-signing an earlier device set up.
    pub recovery_key: Option<String>,
    pub store: MatrixStore,
    /// The saved session JSON a [`crate::Session`] hands out, restoring the same device.
    pub session: Option<String>,
}

#[derive(Clone, PartialEq, Eq)]
pub enum MatrixLogin {
    Password(String),
    /// Device sign-in through [`matrix_sign_in`]; the account then runs on its saved session.
    OAuth,
}

/// Where the account keeps its state and keys.
#[derive(Clone, PartialEq, Eq)]
pub struct MatrixStore {
    /// None keeps everything in memory.
    pub dir: Option<PathBuf>,
    pub passphrase: String,
}

/// Leaves the password, recovery key, store passphrase and session out.
impl fmt::Debug for MatrixAccount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matrix")
            .field("homeserver", &self.homeserver)
            .field("user", &self.user)
            .field("login", &self.login)
            .field("nickname", &self.nickname)
            .field("store", &self.store)
            .finish_non_exhaustive()
    }
}

/// Leaves the password out.
impl fmt::Debug for MatrixLogin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password(_) => f.write_str("Password"),
            Self::OAuth => f.write_str("OAuth"),
        }
    }
}

/// Leaves the passphrase out.
impl fmt::Debug for MatrixStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatrixStore")
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

impl MatrixAccount {
    /// Whether `next` is this account under another nickname, or with the session its live
    /// backend last saved (`latest`): a refresh, or the backend's own first login handed back.
    /// A session of another device, or none, needs a new session.
    pub(crate) fn renamed_to(&self, next: &Self, latest: Option<&str>) -> bool {
        self.homeserver == next.homeserver
            && self.user == next.user
            && self.login == next.login
            && (self.recovery_key == next.recovery_key
                || next.recovery_key.is_some() && next.recovery_key == made_key(latest))
            && self.store == next.store
            && (next.session == self.session || same_device(next.session.as_deref(), latest))
    }
}

/// Who a saved session signs in as.
#[derive(Deserialize, PartialEq)]
struct Owner {
    user_id: String,
    device_id: String,
}

#[derive(Deserialize)]
struct Made {
    recovery_key: Option<String>,
}

fn made_key(session: Option<&str>) -> Option<String> {
    serde_json::from_str::<Made>(session?).ok()?.recovery_key
}

/// Splits the recovery key a backend set up off the session it saved: the session to keep and
/// the key, which belongs in the account's secrets. None when the session carries no key.
#[must_use]
pub fn take_recovery_key(session: &str) -> Option<(String, String)> {
    let mut json: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(session).ok()?;
    let serde_json::Value::String(key) = json.remove("recovery_key")? else {
        return None;
    };
    let rest = serde_json::to_string(&json).ok()?;
    Some((rest, key))
}

/// Whether two user ids name the same account, compared as the homeserver does: in lowercase.
#[must_use]
pub fn same_user(one: &str, other: &str) -> bool {
    one.eq_ignore_ascii_case(other)
}

/// Who a saved session signs in as.
#[must_use]
pub fn session_user(session: &str) -> Option<String> {
    serde_json::from_str::<Owner>(session)
        .ok()
        .map(|owner| owner.user_id)
}

fn same_device(next: Option<&str>, latest: Option<&str>) -> bool {
    let owner = |json: &str| serde_json::from_str::<Owner>(json).ok();
    match (next.and_then(owner), latest.and_then(owner)) {
        (Some(next), Some(latest)) => next == latest,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
