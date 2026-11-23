//! Opening the account's client: store, sign-in, cross-signing and display name.

use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::authentication::oauth::OAuthSession;
use matrix_sdk::ruma::UserId;
use matrix_sdk::ruma::api::client::uiaa;
use matrix_sdk::{AuthSession, Client, ClientBuildError};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::matrix::{MatrixAccount, MatrixLogin, MatrixStore};

pub(crate) const NOT_SIGNED_IN: &str = "Not signed in";
const UNREACHABLE: &str = "Homeserver unreachable";
const DEVICE_NAME: &str = "sdrmm";

/// Why the account stopped, and whether trying again may help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fault {
    pub(crate) reason: String,
    pub(crate) retry: bool,
}

impl Fault {
    /// Needs the operator: a new password, sign-in or recovery key.
    pub(crate) fn lasting(reason: &str) -> Self {
        Self {
            reason: reason.to_owned(),
            retry: false,
        }
    }

    pub(crate) fn passing(reason: &str) -> Self {
        Self {
            reason: reason.to_owned(),
            retry: true,
        }
    }
}

/// A session as saved between starts: what `restore_session` needs, tagged by login kind.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind")]
pub(crate) enum Saved {
    #[serde(rename = "matrix")]
    Matrix(MatrixSession),
    #[serde(rename = "oauth")]
    OAuth(OAuthSession),
}

impl Saved {
    fn of(client: &Client) -> Option<Self> {
        match client.session()? {
            AuthSession::Matrix(session) => Some(Self::Matrix(session)),
            AuthSession::OAuth(session) => Some(Self::OAuth(*session)),
            _ => None,
        }
    }

    pub(crate) fn json(&self) -> Option<String> {
        serde_json::to_string(self)
            .inspect_err(|error| tracing::warn!(%error, "Matrix session not saved"))
            .ok()
    }
}

/// Hands the client's current session out to be kept.
pub(crate) fn save(client: &Client, saved: &watch::Sender<Option<String>>) {
    if let Some(json) = Saved::of(client).as_ref().and_then(Saved::json) {
        saved.send_replace(Some(json));
    }
}

/// A client for `homeserver`, a URL or a server name, on `store` when it names a folder.
pub(crate) async fn build_client(
    homeserver: &str,
    store: Option<&MatrixStore>,
) -> Result<Client, Fault> {
    crate::install_tls_provider();
    let mut builder = Client::builder()
        .server_name_or_homeserver_url(homeserver.trim())
        .handle_refresh_tokens();
    if let Some(MatrixStore {
        dir: Some(dir),
        passphrase,
    }) = store
    {
        let passphrase = (!passphrase.is_empty()).then_some(passphrase.as_str());
        builder = builder.sqlite_store(dir, passphrase);
    }
    builder.build().await.map_err(|error| {
        tracing::warn!(%error, homeserver, "Matrix client not built");
        match error {
            ClientBuildError::SqliteStore(_) => Fault::lasting("Store unreadable"),
            _ => Fault::passing(UNREACHABLE),
        }
    })
}

/// Restores `session`, or logs in with the password and saves the new session.
pub(crate) async fn sign_in(
    client: &Client,
    account: &MatrixAccount,
    session: Option<&str>,
    saved: &watch::Sender<Option<String>>,
) -> Result<(), Fault> {
    if let Some(session) = session {
        return restore(client, session).await;
    }
    let MatrixLogin::Password(password) = &account.login else {
        return Err(Fault::lasting(NOT_SIGNED_IN));
    };
    client
        .matrix_auth()
        .login_username(account.user.trim(), password)
        .initial_device_display_name(DEVICE_NAME)
        .send()
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Matrix login failed");
            if error.as_client_api_error().is_some() {
                Fault::lasting("Login refused")
            } else {
                Fault::passing(UNREACHABLE)
            }
        })?;
    save(client, saved);
    Ok(())
}

async fn restore(client: &Client, session: &str) -> Result<(), Fault> {
    let session: AuthSession = match serde_json::from_str(session) {
        Ok(Saved::Matrix(session)) => session.into(),
        Ok(Saved::OAuth(session)) => session.into(),
        Err(error) => {
            tracing::warn!(%error, "saved Matrix session unreadable");
            return Err(Fault::lasting(NOT_SIGNED_IN));
        }
    };
    client.restore_session(session).await.map_err(|error| {
        tracing::warn!(%error, "Matrix session not restored");
        Fault::lasting(NOT_SIGNED_IN)
    })
}

/// Makes the device trusted: cross-signing set up on a fresh account, or unlocked with the
/// recovery key on one that has it, so Element shares keys with it.
pub(crate) async fn trust(client: &Client, account: &MatrixAccount) -> Result<(), Fault> {
    let encryption = client.encryption();
    encryption.wait_for_e2ee_initialization_tasks().await;
    if encryption
        .cross_signing_status()
        .await
        .is_some_and(|status| status.is_complete())
    {
        return Ok(());
    }
    let user = client
        .user_id()
        .ok_or_else(|| Fault::lasting(NOT_SIGNED_IN))?
        .to_owned();
    let identity = encryption
        .request_user_identity(&user)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Matrix identity not fetched");
            Fault::passing(UNREACHABLE)
        })?;
    if identity.is_none() {
        return bootstrap(client, account, &user).await;
    }
    recover(client, account).await
}

/// Sets up cross-signing, answering the password prompt a homeserver may raise.
async fn bootstrap(client: &Client, account: &MatrixAccount, user: &UserId) -> Result<(), Fault> {
    let encryption = client.encryption();
    let Err(error) = encryption.bootstrap_cross_signing(None).await else {
        return Ok(());
    };
    let refused = |error: &dyn std::fmt::Display| {
        tracing::warn!(%error, "Matrix cross-signing not set up");
        Fault::lasting("Cross-signing refused")
    };
    let (Some(prompt), MatrixLogin::Password(password)) =
        (error.as_uiaa_response(), &account.login)
    else {
        return Err(refused(&error));
    };
    let mut auth = uiaa::Password::new(
        uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(user.to_string())),
        password.clone(),
    );
    auth.session.clone_from(&prompt.session);
    encryption
        .bootstrap_cross_signing(Some(uiaa::AuthData::Password(auth)))
        .await
        .map_err(|error| refused(&error))
}

async fn recover(client: &Client, account: &MatrixAccount) -> Result<(), Fault> {
    let key = account
        .recovery_key
        .as_deref()
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| Fault::lasting("Recovery key needed"))?;
    client
        .encryption()
        .recovery()
        .recover(key)
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Matrix recovery failed");
            Fault::lasting("Recovery key refused")
        })?;
    verify_own_device(client).await;
    Ok(())
}

async fn verify_own_device(client: &Client) {
    match client.encryption().get_own_device().await {
        Ok(Some(device)) if !device.is_cross_signed_by_owner() => {
            if let Err(error) = device.verify().await {
                tracing::warn!(%error, "Matrix device not verified");
            }
        }
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "Matrix device not found"),
    }
}

/// Shows `name` as the account's display name; empty keeps the one it has.
pub(crate) async fn show_name(client: &Client, name: &str) {
    if name.is_empty() {
        return;
    }
    let account = client.account();
    if account
        .get_display_name()
        .await
        .is_ok_and(|current| current.as_deref() == Some(name))
    {
        return;
    }
    if let Err(error) = account.set_display_name(Some(name)).await {
        tracing::warn!(%error, "Matrix display name not set");
    }
}
