//! Matrix OAuth sign-in of an account, beside its voice session.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use sdrmm_voicechat::{SignedIn, VoiceChatError, matrix_sign_in};
use sdrmm_wire::{Account, AccountService, MatrixLogin, MatrixSignInPrompt, MatrixSignInState};
use tokio::sync::oneshot;

use super::{Sessions, lock, save};

const DEVICE_NAME: &str = "sdrmm";
const OTHER_USER: &str = "Signed in as another user";
const FAILED: &str = "Sign-in failed";

/// One sign-in of an account. A newer attempt replaces it, so the older one cannot end it.
pub(super) struct Run {
    attempt: u64,
    pub(super) state: MatrixSignInState,
}

#[derive(Debug)]
pub(crate) enum SignInError {
    NotOAuth,
    Refused(String),
}

impl Sessions {
    /// Starts an OAuth sign-in of `account` and answers what the operator opens. From then on
    /// the account's session stays closed until the sign-in ends, since signing in clears its
    /// store.
    pub(crate) async fn sign_in(
        self: Arc<Self>,
        account: Account,
    ) -> Result<MatrixSignInPrompt, SignInError> {
        let AccountService::Matrix {
            homeserver_url,
            user,
            login: MatrixLogin::Oauth,
            store_key,
            ..
        } = &account.service
        else {
            return Err(SignInError::NotOAuth);
        };
        let (sessions, id, key) = (self.clone(), account.id.clone(), store_key.clone());
        let store = tokio::task::spawn_blocking(move || sessions.matrix_store(&id, &key))
            .await
            .map_err(|error| SignInError::Refused(error.to_string()))?
            .map_err(SignInError::Refused)?;
        let started = match matrix_sign_in(homeserver_url, &store, DEVICE_NAME).await {
            Ok(started) => started,
            Err(error) => {
                let reason = reason(error);
                let attempt = self.next_attempt();
                self.set_sign_in(&account.id, attempt, ended(Some(reason.clone())));
                return Err(SignInError::Refused(reason));
            }
        };
        let prompt = MatrixSignInPrompt {
            verification_uri: started.verification_uri,
            user_code: started.user_code,
            expires_in_s: u32::try_from(started.expires_in.as_secs()).unwrap_or(u32::MAX),
        };
        let attempt = self.next_attempt();
        self.set_sign_in(
            &account.id,
            attempt,
            MatrixSignInState::Pending(prompt.clone()),
        );
        self.lock().remove(&account.id);
        let finish = self.finish_sign_in(account.id.clone(), user.clone(), attempt, started.done);
        tokio::spawn(finish);
        Ok(prompt)
    }

    fn next_attempt(&self) -> u64 {
        self.attempts.fetch_add(1, Ordering::Relaxed)
    }

    fn set_sign_in(&self, account: &str, attempt: u64, state: MatrixSignInState) {
        lock(&self.sign_ins).insert(account.to_owned(), Run { attempt, state });
    }

    fn current(&self, account: &str, attempt: u64) -> bool {
        lock(&self.sign_ins)
            .get(account)
            .is_some_and(|run| run.attempt == attempt)
    }

    /// Saves the approved session unless it is another user's, then ends the sign-in.
    async fn finish_sign_in(
        self: Arc<Self>,
        account: String,
        user: String,
        attempt: u64,
        done: oneshot::Receiver<Result<SignedIn, String>>,
    ) {
        let outcome = match done.await.unwrap_or_else(|_| Err(FAILED.to_owned())) {
            Ok(signed) if signed.user != user => Err(OTHER_USER.to_owned()),
            Ok(_) if !self.current(&account, attempt) => return,
            Ok(signed) => save(&self.store, &account, signed.session).await,
            Err(error) => Err(error),
        };
        if self.current(&account, attempt) {
            self.set_sign_in(&account, attempt, ended(outcome.err()));
        }
    }
}

fn ended(error: Option<String>) -> MatrixSignInState {
    error.map_or(MatrixSignInState::Idle, |error| MatrixSignInState::Failed {
        error,
    })
}

fn reason(error: VoiceChatError) -> String {
    match error {
        VoiceChatError::Start(reason) => reason,
        other => other.to_string(),
    }
}
