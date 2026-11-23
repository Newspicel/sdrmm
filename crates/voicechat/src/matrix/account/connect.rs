//! Opening the account's client: store, sign-in, cross-signing and display name.

use matrix_sdk::authentication::matrix::MatrixSession;
use matrix_sdk::authentication::oauth::OAuthSession;
use matrix_sdk::encryption::CrossSigningResetAuthType;
use matrix_sdk::ruma::UserId;
use matrix_sdk::ruma::api::client::uiaa;
use matrix_sdk::ruma::api::error::ErrorKind;
use matrix_sdk::{AuthSession, Client, ClientBuildError};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::matrix::{MatrixAccount, MatrixLogin, MatrixStore};

pub(crate) const NOT_SIGNED_IN: &str = "Not signed in";
const UNREACHABLE: &str = "Homeserver unreachable";
const STORE_UNREADABLE: &str = "Store unreadable";
const DEVICE_NAME: &str = "sdrmm";

/// Why the account stopped, and whether trying again may help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Fault {
    pub(crate) reason: String,
    pub(crate) retry: bool,
    /// The homeserver forgot the session: a password account may log in anew.
    pub(crate) revoked: bool,
}

impl Fault {
    /// Needs the operator: a new password, sign-in or recovery key.
    pub(crate) fn lasting(reason: &str) -> Self {
        Self {
            reason: reason.to_owned(),
            retry: false,
            revoked: false,
        }
    }

    pub(crate) fn passing(reason: &str) -> Self {
        Self {
            reason: reason.to_owned(),
            retry: true,
            revoked: false,
        }
    }

    pub(crate) fn revoked() -> Self {
        Self {
            revoked: true,
            ..Self::lasting(NOT_SIGNED_IN)
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

/// A session handed out to be kept, with the recovery key this device made. The server moves
/// the key into the account's secrets.
#[derive(Serialize)]
struct Kept<'a> {
    #[serde(flatten)]
    saved: Saved,
    #[serde(skip_serializing_if = "Option::is_none")]
    recovery_key: Option<&'a str>,
}

/// Hands the client's current session out to be kept, with `made`, the recovery key this
/// device set up, if any.
pub(crate) fn save(client: &Client, saved: &watch::Sender<Option<String>>, made: Option<&str>) {
    let Some(session) = Saved::of(client) else {
        return;
    };
    let kept = Kept {
        saved: session,
        recovery_key: made,
    };
    match serde_json::to_string(&kept) {
        Ok(json) => {
            saved.send_replace(Some(json));
        }
        Err(error) => tracing::warn!(%error, "Matrix session not saved"),
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
            ClientBuildError::SqliteStore(_) => Fault::lasting(STORE_UNREADABLE),
            _ => Fault::passing(UNREACHABLE),
        }
    })
}

/// The account's client, signed in. A fresh login starts on an empty store: keys of another
/// device there would refuse the new one, and a store without its session cannot be restored.
pub(crate) async fn open(
    account: &MatrixAccount,
    session: Option<&str>,
    saved: &watch::Sender<Option<String>>,
    made: Option<&str>,
) -> Result<Client, Fault> {
    if session.is_none() {
        if account.login == MatrixLogin::OAuth {
            return Err(Fault::lasting(NOT_SIGNED_IN));
        }
        clear_store(&account.store).await?;
    }
    let client = build_client(&account.homeserver, Some(&account.store)).await?;
    sign_in(&client, account, session, saved, made).await?;
    Ok(client)
}

/// Restores `session`, or logs in with the password and saves the new session.
pub(crate) async fn sign_in(
    client: &Client,
    account: &MatrixAccount,
    session: Option<&str>,
    saved: &watch::Sender<Option<String>>,
    made: Option<&str>,
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
        .map_err(|error| login_fault(&error))?;
    save(client, saved, made);
    Ok(())
}

/// A refusal or a lost connection before the login, or a store fault after it. The last must
/// not retry: each retry would log in again and leave another device behind.
fn login_fault(error: &matrix_sdk::Error) -> Fault {
    tracing::warn!(%error, "Matrix login failed");
    if refused(error) {
        Fault::lasting("Login refused")
    } else if matches!(error, matrix_sdk::Error::Http(_)) {
        Fault::passing(UNREACHABLE)
    } else {
        Fault::lasting(STORE_UNREADABLE)
    }
}

/// A wrong password or a closed account: only the operator can fix it. Any other answer, a
/// rate limit or a server error, may pass.
fn refused(error: &matrix_sdk::Error) -> bool {
    matches!(
        error.client_api_error_kind(),
        Some(ErrorKind::Forbidden | ErrorKind::UserDeactivated)
    ) || error
        .as_client_api_error()
        .is_some_and(|error| matches!(error.status_code.as_u16(), 401 | 403))
}

/// Whether `error` comes from the account's own store, which no retry fixes.
pub(crate) fn store_failed(error: &matrix_sdk::Error) -> bool {
    matches!(
        error,
        matrix_sdk::Error::CryptoStoreError(_)
            | matrix_sdk::Error::BadCryptoStoreState
            | matrix_sdk::Error::StateStore(_)
            | matrix_sdk::Error::EventCacheStore(_)
            | matrix_sdk::Error::MediaStore(_)
    )
}

/// Empties the store folder for a new device.
pub(crate) async fn clear_store(store: &MatrixStore) -> Result<(), Fault> {
    let Some(dir) = store.dir.clone() else {
        return Ok(());
    };
    let cleared = tokio::task::spawn_blocking(move || match std::fs::remove_dir_all(&dir) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    })
    .await
    .unwrap_or_else(|error| Err(std::io::Error::other(error)));
    cleared.map_err(|error| {
        tracing::warn!(%error, "Matrix store not cleared");
        Fault::lasting(STORE_UNREADABLE)
    })
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
        Fault::lasting(STORE_UNREADABLE)
    })
}

/// Makes the device trusted: cross-signing set up on a fresh account, or unlocked with the
/// recovery key on one that has it, so Element shares keys with it. A fresh account's new
/// recovery key lands in `made`; it unlocks a later device when the account has none.
pub(crate) async fn trust(
    client: &Client,
    account: &MatrixAccount,
    made: &mut Option<String>,
) -> Result<(), Fault> {
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
        bootstrap(client, account, &user).await?;
        *made = enable_recovery(client).await;
        return Ok(());
    }
    let key = account.recovery_key.as_deref().or(made.as_deref());
    if key.is_none() && !secret_storage(client).await? {
        reset(client, account, &user).await?;
        *made = enable_recovery(client).await;
        return Ok(());
    }
    recover(client, key).await
}

/// True when the account keeps its cross-signing keys in secret storage on the homeserver.
async fn secret_storage(client: &Client) -> Result<bool, Fault> {
    client
        .encryption()
        .secret_storage()
        .is_enabled()
        .await
        .map_err(|error| {
            tracing::warn!(%error, "Matrix secret storage not read");
            Fault::passing(UNREACHABLE)
        })
}

/// Replaces cross-signing that nothing can unlock: no recovery key is known and the account
/// keeps no secret storage, so the old keys are gone for good.
async fn reset(client: &Client, account: &MatrixAccount, user: &UserId) -> Result<(), Fault> {
    tracing::warn!("Matrix cross-signing has no recovery: resetting it");
    let refused = |error: &dyn std::fmt::Display| {
        tracing::warn!(%error, "Matrix cross-signing not reset");
        Fault::lasting("Cross-signing refused")
    };
    let Some(handle) = client
        .encryption()
        .recovery()
        .reset_identity()
        .await
        .map_err(|error| refused(&error))?
    else {
        return Ok(());
    };
    let auth = match (handle.auth_type(), &account.login) {
        (CrossSigningResetAuthType::Uiaa(prompt), MatrixLogin::Password(password)) => prompt
            .as_password_auth_data(
                uiaa::UserIdentifier::Matrix(uiaa::MatrixUserIdentifier::new(user.to_string())),
                password.clone(),
            ),
        (CrossSigningResetAuthType::OAuth(prompt), _) => {
            tracing::warn!(url = %prompt.approval_url, "approve the Matrix cross-signing reset");
            prompt.as_auth_data()
        }
        (CrossSigningResetAuthType::Uiaa(_), MatrixLogin::OAuth) => {
            handle.cancel().await;
            return Err(refused(&"no password to answer the prompt"));
        }
    };
    handle.reset(auth).await.map_err(|error| refused(&error))
}

/// Keeps cross-signing behind a new recovery key, so a later device can unlock it.
async fn enable_recovery(client: &Client) -> Option<String> {
    client
        .encryption()
        .recovery()
        .enable()
        .await
        .inspect_err(|error| tracing::warn!(%error, "Matrix recovery not enabled"))
        .ok()
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

async fn recover(client: &Client, key: Option<&str>) -> Result<(), Fault> {
    let key = key
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
