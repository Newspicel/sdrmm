//! OAuth sign-in by device code (RFC 8628): the operator approves the bot in a browser.

use std::fmt;
use std::time::Duration;

use matrix_sdk::authentication::oauth::registration::{
    ApplicationType, ClientMetadata, Localized, OAuthGrantType,
};
use matrix_sdk::authentication::oauth::{ClientId, OAuthSession, UserSession};
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{DeviceId, OwnedDeviceId, OwnedUserId};
use matrix_sdk::{Client, SessionMeta, SessionTokens};
use serde::Deserialize;
use tokio::sync::oneshot;
use tokio::time::Instant;
use url::Url;

use super::MatrixStore;
use super::account::connect::{Fault, Saved, build_client};
use crate::VoiceChatError;

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
const API_SCOPE: &str = "urn:matrix:client:api:*";
const DEVICE_SCOPE: &str = "urn:matrix:client:device:";
/// The page the homeserver shows as the client's home when it asks for approval.
const CLIENT_URI: &str = env!("CARGO_PKG_REPOSITORY");
/// RFC 8628 3.5: the poll interval when the server names none, and what `slow_down` adds.
const POLL: Duration = Duration::from_secs(5);

/// A pending sign-in: show the link and code, then wait on `done`.
#[derive(Debug)]
pub struct SignIn {
    pub verification_uri: String,
    pub user_code: String,
    pub expires_in: Duration,
    pub done: oneshot::Receiver<Result<SignedIn, String>>,
}

/// An approved sign-in: the account and the session to save for it.
pub struct SignedIn {
    pub user: String,
    pub session: String,
}

/// Leaves the session out.
impl fmt::Debug for SignedIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SignedIn")
            .field("user", &self.user)
            .finish_non_exhaustive()
    }
}

/// Starts a device sign-in on `homeserver`. `done` answers once the operator approves,
/// after the new device's keys are set up in `store`, which is closed again by then.
pub async fn matrix_sign_in(
    homeserver: &str,
    store: &MatrixStore,
    device_name: &str,
) -> Result<SignIn, VoiceChatError> {
    let discovery = build_client(homeserver, None).await.map_err(start)?;
    let metadata = discovery.oauth().server_metadata().await.map_err(|error| {
        tracing::warn!(%error, "Matrix OAuth metadata not found");
        VoiceChatError::Start("No OAuth sign-in".into())
    })?;
    let device_endpoint = metadata
        .device_authorization_endpoint
        .clone()
        .ok_or_else(|| VoiceChatError::Start("No device sign-in".into()))?;
    let client_id = register(&discovery, device_name).await?;
    let device = DeviceId::new();
    let http = reqwest::Client::new();
    let grant = authorize(&http, &device_endpoint, &client_id, &device).await?;
    let (answer, done) = oneshot::channel();
    let shown = SignIn {
        verification_uri: grant
            .verification_uri_complete
            .clone()
            .unwrap_or_else(|| grant.verification_uri.clone()),
        user_code: grant.user_code.clone(),
        expires_in: Duration::from_secs(grant.expires_in),
        done,
    };
    let pending = Pending {
        http,
        homeserver: discovery.homeserver(),
        token_endpoint: metadata.token_endpoint,
        client_id,
        grant,
        store: store.clone(),
    };
    tokio::spawn(async move {
        if answer.send(pending.finish().await).is_err() {
            tracing::debug!("Matrix sign-in no longer awaited");
        }
    });
    Ok(shown)
}

fn start(fault: Fault) -> VoiceChatError {
    VoiceChatError::Start(fault.reason)
}

/// Registers this app with the homeserver's authorization server (MSC2966).
async fn register(client: &Client, name: &str) -> Result<ClientId, VoiceChatError> {
    let refused = |error: &dyn fmt::Display| {
        tracing::warn!(%error, "Matrix OAuth registration refused");
        VoiceChatError::Start("Sign-in refused".into())
    };
    let home = Url::parse(CLIENT_URI).map_err(|error| refused(&error))?;
    let mut metadata = ClientMetadata::new(
        ApplicationType::Native,
        vec![OAuthGrantType::DeviceCode],
        Localized::new(home, []),
    );
    metadata.client_name = Some(Localized::new(name.to_owned(), []));
    let raw = Raw::new(&metadata).map_err(|error| refused(&error))?;
    client
        .oauth()
        .register_client(&raw)
        .await
        .map(|registered| registered.client_id)
        .map_err(|error| refused(&error))
}

/// RFC 8628 3.2 device authorization response.
#[derive(Deserialize)]
struct Grant {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

async fn authorize(
    http: &reqwest::Client,
    endpoint: &Url,
    client_id: &ClientId,
    device: &DeviceId,
) -> Result<Grant, VoiceChatError> {
    let refused = |error: &dyn fmt::Display| {
        tracing::warn!(%error, "Matrix device sign-in refused");
        VoiceChatError::Start("Sign-in refused".into())
    };
    let scope = format!("{API_SCOPE} {DEVICE_SCOPE}{device}");
    http.post(endpoint.clone())
        .form(&[("client_id", client_id.as_str()), ("scope", &scope)])
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| refused(&error))?
        .json()
        .await
        .map_err(|error| refused(&error))
}

/// RFC 8628 3.5 token response, a success or an error code.
#[derive(Deserialize)]
struct TokenAnswer {
    access_token: Option<String>,
    refresh_token: Option<String>,
    error: Option<String>,
}

enum Poll {
    Tokens(SessionTokens),
    Pending,
    SlowDown,
}

#[derive(Deserialize)]
struct WhoAmI {
    user_id: OwnedUserId,
    device_id: OwnedDeviceId,
}

/// A sign-in waiting for approval.
struct Pending {
    http: reqwest::Client,
    homeserver: Url,
    token_endpoint: Url,
    client_id: ClientId,
    grant: Grant,
    store: MatrixStore,
}

impl Pending {
    async fn finish(self) -> Result<SignedIn, String> {
        let tokens = self.poll().await?;
        let who = self.whoami(&tokens.access_token).await?;
        let session = OAuthSession {
            client_id: self.client_id,
            user: UserSession {
                meta: SessionMeta {
                    user_id: who.user_id.clone(),
                    device_id: who.device_id,
                },
                tokens,
            },
        };
        settle(&self.homeserver, &self.store, session.clone()).await?;
        let session = Saved::OAuth(session)
            .json()
            .ok_or_else(|| "Sign-in failed".to_owned())?;
        Ok(SignedIn {
            user: who.user_id.to_string(),
            session,
        })
    }

    async fn poll(&self) -> Result<SessionTokens, String> {
        let deadline = Instant::now() + Duration::from_secs(self.grant.expires_in);
        let mut wait = self.grant.interval.map_or(POLL, Duration::from_secs);
        loop {
            tokio::time::sleep(wait).await;
            if Instant::now() >= deadline {
                return Err("Sign-in expired".into());
            }
            match self.ask().await? {
                Poll::Tokens(tokens) => return Ok(tokens),
                Poll::Pending => {}
                Poll::SlowDown => wait += POLL,
            }
        }
    }

    async fn ask(&self) -> Result<Poll, String> {
        let answer: TokenAnswer = self
            .http
            .post(self.token_endpoint.clone())
            .form(&[
                ("grant_type", DEVICE_GRANT),
                ("device_code", &self.grant.device_code),
                ("client_id", self.client_id.as_str()),
            ])
            .send()
            .await
            .map_err(|error| failed(&error))?
            .json()
            .await
            .map_err(|error| failed(&error))?;
        match (answer.access_token, answer.error.as_deref()) {
            (Some(access_token), None) => Ok(Poll::Tokens(SessionTokens {
                access_token,
                refresh_token: answer.refresh_token,
            })),
            (_, Some("authorization_pending")) => Ok(Poll::Pending),
            (_, Some("slow_down")) => Ok(Poll::SlowDown),
            (_, Some("access_denied")) => Err("Sign-in refused".into()),
            (_, Some("expired_token")) => Err("Sign-in expired".into()),
            (_, error) => Err(failed(&error.unwrap_or("no token"))),
        }
    }

    async fn whoami(&self, token: &str) -> Result<WhoAmI, String> {
        let url = format!(
            "{}/_matrix/client/v3/account/whoami",
            self.homeserver.as_str().trim_end_matches('/')
        );
        self.http
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|error| failed(&error))?
            .json()
            .await
            .map_err(|error| failed(&error))
    }
}

fn failed(error: &dyn fmt::Display) -> String {
    tracing::warn!(%error, "Matrix sign-in failed");
    "Sign-in failed".to_owned()
}

/// Restores `session` into the account's store once, so the device it names gets its keys
/// there, then closes the store for the session that will open it.
async fn settle(
    homeserver: &Url,
    store: &MatrixStore,
    session: OAuthSession,
) -> Result<(), String> {
    let client = build_client(homeserver.as_str(), Some(store))
        .await
        .map_err(|fault| fault.reason)?;
    client
        .restore_session(session)
        .await
        .map_err(|error| failed(&error))?;
    client
        .encryption()
        .wait_for_e2ee_initialization_tasks()
        .await;
    Ok(())
}
