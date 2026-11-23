use std::collections::{HashMap, hash_map::Entry};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::AtomicU64};

use sdrmm_voicechat::{Link, MatrixAccount, MatrixStore, Service, Session};
use sdrmm_wire::{Account, AccountService, MatrixLogin, MatrixSignInState, Secret};
use tokio::{runtime::Handle, sync::watch, task::AbortHandle};

use crate::Store;

mod sign_in;

pub(crate) use sign_in::SignInError;

const DEFAULT_NICKNAME: &str = "sdrmm";
const VOICE_LOGIN: &str = "Voice needs password or OAuth login";
const SIGNING_IN: &str = "Signing in";
const NOT_SAVED: &str = "Session not saved";

/// Where voice channels come from: the chat services, or a fake in tests.
pub(crate) trait Links: Send + Sync {
    fn join(&self, account: &Account, channel: &str) -> Result<Link, String>;

    /// Keeps a session only for the accounts in `used`, as they read now. Answers the accounts
    /// whose session it closed, so every link joined through one of them is gone.
    fn settle(&self, used: &[Account]) -> Vec<String>;

    /// What went wrong with the account's session outside any one link.
    fn fault(&self, account: &str) -> Option<String>;
}

/// A live session and the task that keeps each sign-in it saves.
struct Held {
    session: Session,
    keeper: Option<AbortHandle>,
}

impl Drop for Held {
    fn drop(&mut self) {
        if let Some(keeper) = &self.keeper {
            keeper.abort();
        }
    }
}

/// One live session per chat account in use, shared by every node on that account.
pub(crate) struct Sessions {
    store: Arc<Store>,
    /// Each Matrix account keeps its state and keys in a folder of its own here.
    data_dir: Option<PathBuf>,
    held: Mutex<HashMap<String, Held>>,
    faults: Arc<Mutex<HashMap<String, String>>>,
    sign_ins: Mutex<HashMap<String, sign_in::Run>>,
    attempts: AtomicU64,
}

impl Sessions {
    pub(crate) fn new(store: Arc<Store>, data_dir: Option<PathBuf>) -> Self {
        Self {
            store,
            data_dir,
            held: Mutex::new(HashMap::new()),
            faults: Arc::default(),
            sign_ins: Mutex::new(HashMap::new()),
            attempts: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<String, Held>> {
        lock(&self.held)
    }

    /// The Matrix account's store folder, none while the server keeps no data on disk.
    pub(crate) fn store_dir(&self, account: &str) -> Option<PathBuf> {
        self.data_dir
            .as_ref()
            .map(|dir| dir.join("matrix").join(account))
    }

    /// Closes the account's session and forgets everything kept for it.
    pub(crate) fn forget(&self, account: &str) {
        self.lock().remove(account);
        lock(&self.faults).remove(account);
        lock(&self.sign_ins).remove(account);
    }

    /// The account with its credentials read from this server's store.
    fn service(&self, account: &Account) -> Result<Service, String> {
        match &account.service {
            AccountService::Discord { token, nickname } => Ok(Service::Discord {
                token: self.required(token, "Token")?,
                nickname: nickname.trim().to_owned(),
            }),
            AccountService::Teamspeak {
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
            AccountService::Matrix { .. } => self.matrix(account).map(Service::Matrix),
            other => Err(format!(
                "{} is a {} account, not a chat",
                account.name,
                other.kind().label()
            )),
        }
    }

    fn matrix(&self, account: &Account) -> Result<MatrixAccount, String> {
        let AccountService::Matrix {
            homeserver_url,
            user,
            login,
            password,
            nickname,
            recovery_key,
            session,
            store_key,
            ..
        } = &account.service
        else {
            return Err(VOICE_LOGIN.to_owned());
        };
        let login = match login {
            MatrixLogin::Token => return Err(VOICE_LOGIN.to_owned()),
            MatrixLogin::Password => {
                sdrmm_voicechat::MatrixLogin::Password(self.required(password, "Password")?)
            }
            MatrixLogin::Oauth => sdrmm_voicechat::MatrixLogin::OAuth,
        };
        if self.signing_in(&account.id) {
            return Err(SIGNING_IN.to_owned());
        }
        Ok(MatrixAccount {
            homeserver: homeserver_url.clone(),
            user: user.clone(),
            login,
            nickname: nickname.trim().to_owned(),
            recovery_key: self.reveal(recovery_key, "Recovery key")?,
            store: self.matrix_store(&account.id, store_key)?,
            session: self.reveal(session, "Session")?,
        })
    }

    fn matrix_store(&self, account: &str, store_key: &Secret) -> Result<MatrixStore, String> {
        Ok(MatrixStore {
            dir: self.store_dir(account),
            passphrase: self.required(store_key, "Store key")?,
        })
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

    fn open(&self, account: &str, service: Service) -> Held {
        let known = match &service {
            Service::Matrix(matrix) => matrix.session.clone(),
            Service::Discord { .. } | Service::TeamSpeak { .. } => None,
        };
        let session = Session::open(service);
        let keeper = session
            .saved()
            .and_then(|saved| self.keep(account, known, saved));
        Held { session, keeper }
    }

    /// Writes each session the account saves into the store, so the next start restores the
    /// same device. A write that fails is the account's fault until one succeeds.
    fn keep(
        &self,
        account: &str,
        mut known: Option<String>,
        mut saved: watch::Receiver<Option<String>>,
    ) -> Option<AbortHandle> {
        let Ok(runtime) = Handle::try_current() else {
            tracing::warn!(account, "no runtime: Matrix sessions will not be kept");
            return None;
        };
        let (store, faults, account) =
            (self.store.clone(), self.faults.clone(), account.to_owned());
        let task = runtime.spawn(async move {
            while saved.changed().await.is_ok() {
                let next = saved.borrow_and_update().clone();
                let Some(session) = next.filter(|next| known.as_ref() != Some(next)) else {
                    continue;
                };
                known = Some(session.clone());
                let fault = save(&store, &account, session).await.err();
                match fault {
                    Some(text) => lock(&faults).insert(account.clone(), text),
                    None => lock(&faults).remove(&account),
                };
            }
        });
        Some(task.abort_handle())
    }
}

/// Keeps `session` as the account's sign-in, or says why it could not.
async fn save(store: &Arc<Store>, account: &str, session: String) -> Result<(), String> {
    let (store, id) = (store.clone(), account.to_owned());
    tokio::task::spawn_blocking(move || store.set_account_session(&id, &session))
        .await
        .map_err(|error| error.to_string())
        .and_then(|saved| saved.map_err(|error| error.to_string()))
        .map_err(|error| {
            tracing::error!(account, %error, "Matrix session not saved");
            NOT_SAVED.to_owned()
        })
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Links for Sessions {
    fn join(&self, account: &Account, channel: &str) -> Result<Link, String> {
        let service = self.service(account)?;
        let mut held = self.lock();
        let live = match held.entry(account.id.clone()) {
            Entry::Occupied(found) => {
                let live = found.into_mut();
                if let Err(service) = live.session.update(service) {
                    *live = self.open(&account.id, service);
                }
                live
            }
            Entry::Vacant(free) => free.insert(self.open(&account.id, service)),
        };
        live.session
            .join(channel)
            .map_err(|error| error.to_string())
    }

    fn settle(&self, used: &[Account]) -> Vec<String> {
        let mut now: HashMap<&str, Option<Service>> = used
            .iter()
            .map(|account| (account.id.as_str(), self.service(account).ok()))
            .collect();
        let mut closed = Vec::new();
        self.lock().retain(|id, live| {
            let kept = now
                .remove(id.as_str())
                .flatten()
                .is_some_and(|service| live.session.update(service).is_ok());
            if !kept {
                closed.push(id.clone());
            }
            kept
        });
        closed
    }

    fn fault(&self, account: &str) -> Option<String> {
        lock(&self.faults).get(account).cloned()
    }
}

impl Sessions {
    /// Where the account's OAuth sign-in stands.
    pub(crate) fn sign_in_state(&self, account: &str) -> MatrixSignInState {
        lock(&self.sign_ins)
            .get(account)
            .map_or_default(|run| run.state.clone())
    }

    fn signing_in(&self, account: &str) -> bool {
        lock(&self.sign_ins)
            .get(account)
            .is_some_and(|run| matches!(run.state, MatrixSignInState::Pending(_)))
    }
}
