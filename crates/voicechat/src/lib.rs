//! Discord and `TeamSpeak` voice links: an account opens a [`Session`], a joined channel is a
//! [`Link`] of plain channels carrying 20 ms mono frames at 48 kHz.

mod discord;
mod link;
mod opus;
mod teamspeak;

use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

pub use link::{Hold, Link};
use tokio::runtime::Handle;

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
        }
    }
}

impl Service {
    /// The name shown in the channel.
    fn nickname(&self) -> &str {
        match self {
            Self::Discord { nickname, .. } | Self::TeamSpeak { nickname, .. } => nickname,
        }
    }

    /// Whether `next` is this account under another nickname, which a live session takes
    /// without leaving its channels.
    fn renamed_to(&self, next: &Self) -> bool {
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
        };
        Self { service, backend }
    }

    pub fn service(&self) -> &Service {
        &self.service
    }

    /// Takes `service` in place when only its nickname changed, so every link stays joined.
    /// Any other change hands it back: the account must open a new session.
    pub fn update(&mut self, service: Service) -> Result<(), Service> {
        if service == self.service {
            return Ok(());
        }
        if !self.service.renamed_to(&service) {
            return Err(service);
        }
        let nickname = service.nickname().to_owned();
        match &self.backend {
            Backend::Discord(discord) => discord.rename(nickname),
            Backend::TeamSpeak(teamspeak) => teamspeak.rename(nickname),
        }
        self.service = service;
        Ok(())
    }

    /// Joins `channel`: a Discord voice channel id, or a `TeamSpeak` channel name or path,
    /// empty for the server's default channel.
    pub fn join(&self, channel: &str) -> Result<Link, VoiceChatError> {
        match &self.backend {
            Backend::Discord(discord) => {
                let runtime = Handle::try_current().map_err(|_| VoiceChatError::NoRuntime)?;
                discord.join(&runtime, channel)
            }
            Backend::TeamSpeak(teamspeak) => teamspeak.join(channel),
        }
    }
}

/// A fresh `TeamSpeak` identity in the text form [`Service::TeamSpeak`] takes.
pub fn new_teamspeak_identity() -> String {
    tsclient_rs::generateIdentity(TEAMSPEAK_LEVEL).to_string()
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
}
