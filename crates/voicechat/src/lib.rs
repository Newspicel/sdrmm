//! Discord, `TeamSpeak` and Matrix voice links: an account opens a [`Session`], a joined
//! channel is a [`Link`] of plain channels carrying 20 ms mono frames at 48 kHz.

mod discord;
mod link;
mod matrix;
mod opus;
mod teamspeak;

use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub use link::{Hold, Link};
pub use matrix::{
    MatrixAccount, MatrixLogin, MatrixStore, SignIn, SignedIn, matrix_sign_in, same_user,
    session_access_token, session_user, take_recovery_key,
};
use tokio::runtime::Handle;
use tokio::sync::watch;

/// Sample rate of every frame a link carries.
pub const RATE_HZ: u32 = 48_000;
/// Samples in one 20 ms frame.
pub const FRAME: usize = 960;

/// Security level of a new `TeamSpeak` identity; servers ask for 8 unless set higher.
const TEAMSPEAK_LEVEL: i32 = 8;

/// A chat account.
#[derive(Clone, PartialEq, Eq)]
pub enum Service {
    Discord {
        token: String,
        /// Empty keeps the bot's own name.
        nickname: String,
    },
    TeamSpeak {
        address: String,
        password: Option<String>,
        nickname: String,
        identity: String,
    },
    Matrix(MatrixAccount),
}

/// Leaves the token, password and identity out, so logging an account leaks no secret.
impl fmt::Debug for Service {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Discord { nickname, .. } => f
                .debug_struct("Discord")
                .field("nickname", nickname)
                .finish_non_exhaustive(),
            Self::TeamSpeak {
                address, nickname, ..
            } => f
                .debug_struct("TeamSpeak")
                .field("address", address)
                .field("nickname", nickname)
                .finish_non_exhaustive(),
            Self::Matrix(account) => account.fmt(f),
        }
    }
}

impl Service {
    /// The name shown in the channel.
    fn nickname(&self) -> &str {
        match self {
            Self::Discord { nickname, .. } | Self::TeamSpeak { nickname, .. } => nickname,
            Self::Matrix(account) => &account.nickname,
        }
    }

    /// Whether `next` is this account under another nickname, which a live session takes
    /// without leaving its channels. `latest` is the Matrix session the live backend runs on.
    fn renamed_to(&self, next: &Self, latest: Option<&str>) -> bool {
        match (self, next) {
            (Self::Discord { token, .. }, Self::Discord { token: next, .. }) => token == next,
            (
                Self::TeamSpeak {
                    address,
                    password,
                    identity,
                    ..
                },
                Self::TeamSpeak {
                    address: next_address,
                    password: next_password,
                    identity: next_identity,
                    ..
                },
            ) => address == next_address && password == next_password && identity == next_identity,
            (Self::Matrix(account), Self::Matrix(next)) => account.renamed_to(next, latest),
            _ => false,
        }
    }
}

/// Someone in the voice channel: a Discord user id or a `TeamSpeak` unique identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Talker {
    pub id: String,
    pub name: String,
    /// False while the service has not tied the voice to a user, so `id` is a stand-in that
    /// no allow list may match.
    pub verified: bool,
}

/// One frame of what a talker said, mono at [`RATE_HZ`].
#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub talker: Talker,
    pub pcm: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Connecting,
    Joined,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VoiceChatError {
    #[error("already in {0}")]
    Clash(String),
    #[error("no channel")]
    NoChannel,
    #[error("not a channel: {0}")]
    BadChannel(String),
    #[error("no async runtime")]
    NoRuntime,
    #[error("cannot start: {0}")]
    Start(String),
}

/// One live account. It connects on the first join and goes offline when dropped.
pub struct Session {
    service: Service,
    backend: Backend,
}

enum Backend {
    Discord(discord::Discord),
    TeamSpeak(teamspeak::TeamSpeak),
    Matrix(matrix::Matrix),
}

impl Session {
    pub fn open(service: Service) -> Self {
        let backend = match service.clone() {
            Service::Discord { token, nickname } => {
                Backend::Discord(discord::Discord::new(token, nickname))
            }
            Service::TeamSpeak {
                address,
                password,
                nickname,
                identity,
            } => Backend::TeamSpeak(teamspeak::TeamSpeak::new(
                teamspeak::Account {
                    address,
                    password,
                    identity,
                },
                nickname,
            )),
            Service::Matrix(account) => Backend::Matrix(matrix::Matrix::new(account)),
        };
        Self { service, backend }
    }

    pub fn service(&self) -> &Service {
        &self.service
    }

    /// Takes `service` in place when only its nickname changed, so every link stays joined.
    /// Any other change hands it back: the account must open a new session.
    #[expect(
        clippy::result_large_err,
        reason = "the caller opens a session from the service handed back, so it moves once"
    )]
    pub fn update(&mut self, service: Service) -> Result<(), Service> {
        if service == self.service {
            return Ok(());
        }
        let latest = match &self.backend {
            Backend::Matrix(matrix) => matrix.latest(),
            Backend::Discord(_) | Backend::TeamSpeak(_) => None,
        };
        if !self.service.renamed_to(&service, latest.as_deref()) {
            return Err(service);
        }
        let nickname = service.nickname().to_owned();
        match &self.backend {
            Backend::Discord(discord) => discord.rename(nickname),
            Backend::TeamSpeak(teamspeak) => teamspeak.rename(nickname),
            Backend::Matrix(matrix) => matrix.rename(nickname),
        }
        self.service = service;
        Ok(())
    }

    /// Joins `channel`: a Discord voice channel id, a `TeamSpeak` channel name or path (empty
    /// for the server's default channel), or a Matrix room id or alias.
    pub fn join(&self, channel: &str) -> Result<Link, VoiceChatError> {
        match &self.backend {
            Backend::Discord(discord) => discord.join(&runtime()?, channel),
            Backend::TeamSpeak(teamspeak) => teamspeak.join(channel),
            Backend::Matrix(matrix) => matrix.join(&runtime()?, channel),
        }
    }

    /// The latest Matrix session to keep, for restoring the same device later. None for the
    /// other services.
    pub fn saved(&self) -> Option<watch::Receiver<Option<String>>> {
        match &self.backend {
            Backend::Matrix(matrix) => Some(matrix.saved()),
            Backend::Discord(_) | Backend::TeamSpeak(_) => None,
        }
    }
}

fn runtime() -> Result<Handle, VoiceChatError> {
    Handle::try_current().map_err(|_| VoiceChatError::NoRuntime)
}

/// A fresh `TeamSpeak` identity in the text form [`Service::TeamSpeak`] takes.
pub fn new_teamspeak_identity() -> String {
    tsclient_rs::generateIdentity(TEAMSPEAK_LEVEL).to_string()
}

/// Installs the process TLS provider once. twilight, reqwest and matrix-sdk build their TLS
/// configs from the process default, which rustls cannot pick by itself once more than one
/// provider is compiled in.
fn install_tls_provider() {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    }
}

/// Locks a control-plane map; a panic elsewhere must not take every link down with it.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn teamspeak() -> Session {
        Session::open(Service::TeamSpeak {
            address: "127.0.0.1:9".into(),
            password: None,
            nickname: "sdrmm".into(),
            identity: new_teamspeak_identity(),
        })
    }

    #[test]
    fn accounts_print_without_secrets() {
        let printed = format!(
            "{:?} {:?}",
            Service::Discord {
                token: "bot-token".into(),
                nickname: "Relay".into(),
            },
            Service::TeamSpeak {
                address: "ts.example".into(),
                password: Some("hunter2".into()),
                nickname: "sdrmm".into(),
                identity: "key:1".into(),
            }
        );
        assert!(printed.contains("ts.example"));
        for secret in ["bot-token", "hunter2", "key:1"] {
            assert!(!printed.contains(secret), "{printed}");
        }
    }

    #[test]
    fn teamspeak_identity_is_reusable() {
        let text = new_teamspeak_identity();
        let identity = tsclient_rs::identityFromString(&text).unwrap();
        assert_eq!(identity.to_string(), text);
        assert!(identity.security_level() >= TEAMSPEAK_LEVEL);
        assert_ne!(new_teamspeak_identity(), text);
    }

    #[test]
    fn second_teamspeak_channel_clashes() {
        let session = teamspeak();
        let first = session.join("Lobby").unwrap();
        assert_eq!(
            session.join("Ops").unwrap_err(),
            VoiceChatError::Clash("Lobby".into())
        );
        drop(first);
        assert!(session.join("Ops").is_ok());
    }

    #[test]
    fn a_teamspeak_channel_frees_with_its_hold() {
        let session = teamspeak();
        let Link { heard, hold, .. } = session.join("Lobby").unwrap();
        drop(hold);
        assert!(session.join("Ops").is_ok());
        drop(heard);
    }

    fn discord() -> Session {
        Session::open(Service::Discord {
            token: "token".into(),
            nickname: String::new(),
        })
    }

    fn renamed(service: &Service, to: &str) -> Service {
        let mut next = service.clone();
        match &mut next {
            Service::Discord { nickname, .. } | Service::TeamSpeak { nickname, .. } => {
                to.clone_into(nickname);
            }
            Service::Matrix(account) => to.clone_into(&mut account.nickname),
        }
        next
    }

    #[test]
    fn a_new_nickname_keeps_the_session_and_its_link() {
        let mut session = teamspeak();
        let _lobby = session.join("Lobby").unwrap();
        let next = renamed(session.service(), "relay");
        assert_eq!(session.update(next.clone()), Ok(()));
        assert_eq!(session.service(), &next);
        assert_eq!(
            session.join("Ops").unwrap_err(),
            VoiceChatError::Clash("Lobby".into())
        );
        let mut session = discord();
        let next = renamed(session.service(), "Relay");
        assert_eq!(session.update(next.clone()), Ok(()));
        assert_eq!(session.service(), &next);
    }

    #[test]
    fn other_changes_need_a_new_session() {
        let mut session = teamspeak();
        let mut moved = renamed(session.service(), "relay");
        if let Service::TeamSpeak { address, .. } = &mut moved {
            "127.0.0.1:10".clone_into(address);
        }
        assert_eq!(session.update(moved.clone()), Err(moved));
        let mut session = discord();
        let swapped = Service::Discord {
            token: "other".into(),
            nickname: String::new(),
        };
        assert_eq!(session.update(swapped.clone()), Err(swapped));
        assert!(session.update(renamed(&teamspeak().service, "x")).is_err());
    }

    #[test]
    fn discord_needs_a_runtime() {
        assert_eq!(discord().join("1").unwrap_err(), VoiceChatError::NoRuntime);
    }

    #[tokio::test]
    async fn discord_channel_must_be_an_id() {
        assert_eq!(discord().join(" ").unwrap_err(), VoiceChatError::NoChannel);
        assert_eq!(
            discord().join("general").unwrap_err(),
            VoiceChatError::BadChannel("general".into())
        );
    }

    fn matrix_account(session: Option<&str>) -> MatrixAccount {
        MatrixAccount {
            homeserver: "http://127.0.0.1:9".into(),
            user: "@bot:x.org".into(),
            login: MatrixLogin::Password("hunter2".into()),
            nickname: "Relay".into(),
            recovery_key: Some("EsTc recovery".into()),
            store: MatrixStore {
                dir: None,
                passphrase: "store-pass".into(),
            },
            session: session.map(str::to_owned),
        }
    }

    fn matrix(session: Option<&str>) -> Session {
        Session::open(Service::Matrix(matrix_account(session)))
    }

    fn session_of(device: &str, token: &str) -> String {
        format!(
            r#"{{"kind":"matrix","user_id":"@bot:x.org","device_id":"{device}","access_token":"{token}"}}"#
        )
    }

    fn with_session(service: &Service, session: &str) -> Service {
        let mut next = service.clone();
        if let Service::Matrix(account) = &mut next {
            account.session = Some(session.to_owned());
        }
        next
    }

    #[test]
    fn matrix_accounts_print_without_secrets() {
        let printed = format!("{:?}", Service::Matrix(matrix_account(Some("tok-123"))));
        assert!(printed.contains("@bot:x.org"), "{printed}");
        for secret in ["hunter2", "EsTc recovery", "store-pass", "tok-123"] {
            assert!(!printed.contains(secret), "{printed}");
        }
    }

    #[tokio::test]
    async fn a_refreshed_session_keeps_the_link() {
        let mut session = matrix(Some(&session_of("A", "first")));
        let _room = session.join("!room:x.org").unwrap();
        let next = with_session(session.service(), &session_of("A", "refreshed"));
        assert_eq!(session.update(next.clone()), Ok(()));
        assert_eq!(session.service(), &next);
        assert_eq!(
            session.join("!room:x.org").unwrap_err(),
            VoiceChatError::Clash("!room:x.org".into())
        );
        assert!(session.saved().is_some());
    }

    #[test]
    fn a_first_session_reopens() {
        let mut session = matrix(None);
        let signed_in = with_session(session.service(), &session_of("A", "first"));
        assert_eq!(session.update(signed_in.clone()), Err(signed_in));
        assert!(teamspeak().saved().is_none());
    }

    #[test]
    fn a_sign_in_with_a_new_device_reopens() {
        let mut session = matrix(Some(&session_of("A", "first")));
        let other = with_session(session.service(), &session_of("B", "second"));
        assert_eq!(session.update(other.clone()), Err(other));
        let mut dropped = session.service().clone();
        if let Service::Matrix(account) = &mut dropped {
            account.session = None;
        }
        assert_eq!(session.update(dropped.clone()), Err(dropped));
    }

    #[tokio::test]
    async fn second_link_in_a_room_clashes() {
        let session = matrix(None);
        let first = session.join("!room:x.org").unwrap();
        assert_eq!(
            session.join(" !room:x.org ").unwrap_err(),
            VoiceChatError::Clash("!room:x.org".into())
        );
        assert!(session.join("#other:x.org").is_ok());
        drop(first);
        assert!(session.join("!room:x.org").is_ok());
    }

    #[tokio::test]
    async fn a_room_must_be_an_id_or_alias() {
        let session = matrix(None);
        assert_eq!(session.join(" ").unwrap_err(), VoiceChatError::NoChannel);
        assert_eq!(
            session.join("general").unwrap_err(),
            VoiceChatError::BadChannel("general".into())
        );
    }
}
