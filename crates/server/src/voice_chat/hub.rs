use std::collections::{HashMap, hash_map::Entry};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use sdrmm_voicechat::{Link, Service, Session};
use sdrmm_wire::{ChatAccount, ChatService, event_output::Secret};

use crate::Store;

const DEFAULT_NICKNAME: &str = "sdrmm";

/// Where voice channels come from: the chat services, or a fake in tests.
pub(crate) trait Links: Send + Sync {
    fn join(&self, account: &ChatAccount, channel: &str) -> Result<Link, String>;

    /// Keeps a session only for the accounts in `used`, as they read now. Answers the accounts
    /// whose session it closed, so every link joined through one of them is gone.
    fn settle(&self, used: &[ChatAccount]) -> Vec<String>;
}

struct Held {
    service: Service,
    session: Session,
}

/// One live session per chat account in use, shared by every node on that account.
pub(crate) struct Sessions {
    store: Arc<Store>,
    held: Mutex<HashMap<String, Held>>,
}

impl Sessions {
    pub(crate) fn new(store: Arc<Store>) -> Self {
        Self {
            store,
            held: Mutex::new(HashMap::new()),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Held>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The account with its credentials read from this server's store.
    fn service(&self, account: &ChatAccount) -> Result<Service, String> {
        match &account.service {
            ChatService::Discord { token } => Ok(Service::Discord {
                token: self.required(token, "Token")?,
            }),
            ChatService::Teamspeak {
                address,
                password,
                nickname,
                identity,
            } => Ok(Service::TeamSpeak {
                address: address.clone(),
                password: self.reveal(password, "Password")?,
                nickname: match nickname.trim() {
                    "" => DEFAULT_NICKNAME.to_owned(),
                    named => named.to_owned(),
                },
                identity: self.required(identity, "Identity")?,
            }),
        }
    }

    fn required(&self, secret: &Secret, what: &str) -> Result<String, String> {
        self.reveal(secret, what)?
            .ok_or_else(|| format!("{what} is not set"))
    }

    fn reveal(&self, secret: &Secret, what: &str) -> Result<Option<String>, String> {
        let value = match secret {
            Secret::Inline(value) => Some(value.clone()),
            Secret::Stored { secret } => self
                .store
                .secret(secret)
                .map_err(|error| format!("{what}: {error}"))?,
        };
        Ok(value.filter(|value| !value.trim().is_empty()))
    }
}

impl Links for Sessions {
    fn join(&self, account: &ChatAccount, channel: &str) -> Result<Link, String> {
        let service = self.service(account)?;
        let mut held = self.lock();
        let live = match held.entry(account.id.clone()) {
            Entry::Occupied(found) if found.get().service == service => found.into_mut(),
            Entry::Occupied(mut stale) => {
                stale.insert(Held::open(service));
                stale.into_mut()
            }
            Entry::Vacant(free) => free.insert(Held::open(service)),
        };
        live.session
            .join(channel)
            .map_err(|error| error.to_string())
    }

    fn settle(&self, used: &[ChatAccount]) -> Vec<String> {
        let now: HashMap<&str, Option<Service>> = used
            .iter()
            .map(|account| (account.id.as_str(), self.service(account).ok()))
            .collect();
        let mut closed = Vec::new();
        self.lock().retain(|id, held| {
            let kept = now
                .get(id.as_str())
                .is_some_and(|service| service.as_ref() == Some(&held.service));
            if !kept {
                closed.push(id.clone());
            }
            kept
        });
        closed
    }
}

impl Held {
    fn open(service: Service) -> Self {
        Self {
            session: Session::open(service.clone()),
            service,
        }
    }
}
