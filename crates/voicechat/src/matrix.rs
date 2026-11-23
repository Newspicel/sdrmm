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
    /// Whether `next` is this account under another nickname or with refreshed tokens, which a
    /// live session takes as it is. A first session, or one taken away, needs a new session.
    pub(crate) fn renamed_to(&self, next: &Self) -> bool {
        self.homeserver == next.homeserver
            && self.user == next.user
            && self.login == next.login
            && self.recovery_key == next.recovery_key
            && self.store == next.store
            && self.session.is_some() == next.session.is_some()
    }
}

#[cfg(test)]
mod tests;
